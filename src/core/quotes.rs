// src/core/quotes.rs
use std::fmt::Write as _;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    YfClient, YfError,
    core::{
        CallOptions, DataQuality, ProjectionContext, ProjectionIssue,
        client::{CacheEndpoint, normalize_symbols},
        conversions::{i64_to_date, i64_to_datetime, quantity_from_u64},
        currency_resolver::{CurrencyHints, ResolvedCurrencyUnit},
        diagnostics::{optional_decimal_f64, optional_wire_cloned, optional_wire_copied},
        models::{FastInfo, MovingAverages},
        net, quotesummary,
        wire::{JsonDecimal, JsonU64, RawDate, RawDecimal, RawNum, RawNumU64, WireValue},
        yahoo_vocab::{first_parsed_yahoo_exchange, parse_yahoo_exchange, parse_yahoo_quote_type},
    },
};
use paft::Decimal;
use paft::aggregates::Snapshot;
use paft::domain::{Exchange, Instrument, MarketState};
use paft::fundamentals::statements::Calendar;
use paft::fundamentals::statistics::KeyStatistics;
use paft::market::orderbook::BookLevel;
use paft::market::quote::Quote;
use paft::money::{Currency, PriceAmount};

const KEY_STATISTICS_MODULES: &str = "summaryDetail,defaultKeyStatistics";

// Centralized wire model for the v7 quote API
#[derive(Deserialize)]
pub struct V7Envelope {
    #[serde(rename = "quoteResponse")]
    pub(crate) quote_response: Option<V7QuoteResponse>,
}

#[derive(Deserialize)]
pub struct V7QuoteResponse {
    pub(crate) result: Option<Vec<Value>>,
    pub(crate) error: Option<V7Error>,
}

#[derive(Deserialize)]
pub struct V7Error {
    pub(crate) description: String,
}

#[derive(Deserialize, Clone)]
pub struct V7QuoteNode {
    #[serde(default)]
    pub(crate) symbol: WireValue<String>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    pub(crate) quote_type: WireValue<String>,
    #[serde(rename = "shortName")]
    #[serde(default)]
    pub(crate) short_name: WireValue<String>,
    #[serde(rename = "longName")]
    #[serde(default)]
    pub(crate) long_name: WireValue<String>,
    #[serde(rename = "regularMarketPrice")]
    #[serde(default)]
    pub(crate) regular_market_price: WireValue<f64>,
    #[serde(rename = "regularMarketOpen")]
    #[serde(default)]
    pub(crate) regular_market_open: WireValue<f64>,
    #[serde(rename = "regularMarketDayHigh")]
    #[serde(default)]
    pub(crate) regular_market_day_high: WireValue<f64>,
    #[serde(rename = "regularMarketDayLow")]
    #[serde(default)]
    pub(crate) regular_market_day_low: WireValue<f64>,
    #[serde(rename = "regularMarketPreviousClose")]
    #[serde(default)]
    pub(crate) regular_market_previous_close: WireValue<f64>,
    #[serde(rename = "regularMarketVolume")]
    #[serde(default)]
    pub(crate) regular_market_volume: WireValue<JsonU64>,
    #[serde(default)]
    pub(crate) bid: WireValue<f64>,
    #[serde(rename = "bidSize")]
    #[serde(default)]
    pub(crate) bid_size: WireValue<JsonU64>,
    #[serde(default)]
    pub(crate) ask: WireValue<f64>,
    #[serde(rename = "askSize")]
    #[serde(default)]
    pub(crate) ask_size: WireValue<JsonU64>,
    #[serde(rename = "regularMarketTime")]
    #[serde(default)]
    pub(crate) regular_market_time: WireValue<i64>,
    #[serde(rename = "averageDailyVolume3Month")]
    #[serde(default)]
    pub(crate) average_daily_volume_3_month: WireValue<JsonU64>,
    #[serde(rename = "fiftyDayAverage")]
    #[serde(default)]
    pub(crate) fifty_day_average: WireValue<f64>,
    #[serde(rename = "twoHundredDayAverage")]
    #[serde(default)]
    pub(crate) two_hundred_day_average: WireValue<f64>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    #[serde(default)]
    pub(crate) fifty_two_week_high: WireValue<f64>,
    #[serde(rename = "fiftyTwoWeekLow")]
    #[serde(default)]
    pub(crate) fifty_two_week_low: WireValue<f64>,
    #[serde(rename = "marketCap")]
    #[serde(default)]
    pub(crate) market_cap: WireValue<JsonDecimal>,
    #[serde(rename = "sharesOutstanding")]
    #[serde(default)]
    pub(crate) shares_outstanding: WireValue<JsonU64>,
    #[serde(rename = "epsTrailingTwelveMonths")]
    #[serde(default)]
    pub(crate) eps_trailing_twelve_months: WireValue<f64>,
    #[serde(rename = "trailingPE")]
    #[serde(default)]
    pub(crate) trailing_pe: WireValue<f64>,
    #[serde(rename = "trailingAnnualDividendYield")]
    #[serde(default)]
    pub(crate) trailing_annual_dividend_yield: WireValue<f64>,
    #[serde(rename = "dividendRate")]
    #[serde(default)]
    pub(crate) dividend_rate: WireValue<f64>,
    #[serde(rename = "dividendYield")]
    #[serde(default)]
    pub(crate) dividend_yield: WireValue<f64>,
    #[serde(default)]
    pub(crate) beta: WireValue<f64>,
    #[serde(rename = "dividendDate")]
    #[serde(default)]
    pub(crate) dividend_date: WireValue<i64>,
    #[serde(default)]
    pub(crate) currency: WireValue<String>,
    #[serde(rename = "financialCurrency")]
    #[serde(default)]
    pub(crate) financial_currency: WireValue<String>,
    #[serde(rename = "fullExchangeName")]
    #[serde(default)]
    pub(crate) full_exchange_name: WireValue<String>,
    #[serde(default)]
    pub(crate) exchange: WireValue<String>,
    #[serde(default)]
    pub(crate) market: WireValue<String>,
    #[serde(rename = "marketCapFigureExchange")]
    #[serde(default)]
    pub(crate) market_cap_figure_exchange: WireValue<String>,
    #[serde(rename = "marketState")]
    #[serde(default)]
    pub(crate) market_state: WireValue<String>,
}

fn wire_str(value: &WireValue<String>) -> Option<&str> {
    value.as_ref().map(String::as_str)
}

fn wire_string(value: &WireValue<String>) -> Option<String> {
    wire_str(value).map(str::to_owned)
}

fn required_wire_str_projection<'a>(
    value: &'a WireValue<String>,
    field: &'static str,
) -> Result<&'a str, ProjectionIssue> {
    match value {
        WireValue::Valid(value) => {
            nonempty(value).ok_or(ProjectionIssue::MissingRequiredField { field })
        }
        WireValue::Missing => Err(ProjectionIssue::MissingRequiredField { field }),
        WireValue::Invalid(details) => Err(ProjectionIssue::InvalidField {
            field,
            details: details.clone(),
        }),
    }
}

fn optional_quote_string(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<String>,
) -> Result<Option<String>, YfError> {
    optional_wire_cloned(ctx, path, key, path, value)
}

fn optional_quote_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<f64>,
) -> Result<Option<f64>, YfError> {
    optional_wire_copied(ctx, path, key, path, value)
}

fn optional_quote_i64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<i64>,
) -> Result<Option<i64>, YfError> {
    optional_wire_copied(ctx, path, key, path, value)
}

fn optional_quote_u64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<JsonU64>,
) -> Result<Option<u64>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.map(JsonU64::into_u64))
}

fn optional_quote_decimal(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<JsonDecimal>,
) -> Result<Option<Decimal>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.map(JsonDecimal::into_decimal))
}

struct V7KeyStatisticsFields {
    market_cap: Option<Decimal>,
    shares_outstanding: Option<u64>,
    eps_trailing_twelve_months: Option<f64>,
    trailing_pe: Option<f64>,
    dividend_rate: Option<f64>,
    trailing_annual_dividend_yield: Option<f64>,
    dividend_yield: Option<f64>,
    fifty_two_week_high: Option<f64>,
    fifty_two_week_low: Option<f64>,
    average_daily_volume_3m: Option<u64>,
    beta: Option<f64>,
}

impl V7QuoteNode {
    fn symbol_key(&self) -> Option<String> {
        wire_string(&self.symbol)
    }

    fn currency_units(&self) -> QuoteCurrencyUnits {
        QuoteCurrencyUnits::from_quote_node(self)
    }

    fn exchange_candidates(&self) -> [(&'static str, Option<&str>); 4] {
        [
            ("fullExchangeName", wire_str(&self.full_exchange_name)),
            ("exchange", wire_str(&self.exchange)),
            ("market", wire_str(&self.market)),
            (
                "marketCapFigureExchange",
                wire_str(&self.market_cap_figure_exchange),
            ),
        ]
    }

    fn exchange(&self) -> Option<Exchange> {
        first_parsed_yahoo_exchange(
            self.exchange_candidates()
                .into_iter()
                .map(|(_, value)| value),
        )
    }

    fn exchange_with_context(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<&str>,
    ) -> Result<Option<Exchange>, YfError> {
        let key = key.map(str::to_owned);
        let candidates = [
            (
                "fullExchangeName",
                optional_quote_string(
                    ctx,
                    "fullExchangeName",
                    key.clone(),
                    &self.full_exchange_name,
                )?,
            ),
            (
                "exchange",
                optional_quote_string(ctx, "exchange", key.clone(), &self.exchange)?,
            ),
            (
                "market",
                optional_quote_string(ctx, "market", key.clone(), &self.market)?,
            ),
            (
                "marketCapFigureExchange",
                optional_quote_string(
                    ctx,
                    "marketCapFigureExchange",
                    key.clone(),
                    &self.market_cap_figure_exchange,
                )?,
            ),
        ];
        let mut failures = Vec::new();
        for (path, value) in candidates {
            let Some(value) = value.as_deref().and_then(nonempty) else {
                continue;
            };
            match parse_yahoo_exchange(value) {
                Ok(parsed) => return Ok(Some(parsed)),
                Err(err) => failures.push(ExchangeCandidateFailure {
                    path,
                    value: value.to_string(),
                    reason: err.to_string(),
                }),
            }
        }

        if let Some(first) = failures.first() {
            ctx.omitted_present_field(
                first.path,
                key,
                ProjectionIssue::InvalidField {
                    field: first.path,
                    details: exchange_candidate_failure_details(&failures),
                },
            )?;
        }
        Ok(None)
    }

    fn instrument_projection(
        &self,
        exchange: Option<paft::domain::Exchange>,
    ) -> Result<Instrument, ProjectionIssue> {
        let sym = required_wire_str_projection(&self.symbol, "symbol")?;
        let quote_type = required_wire_str_projection(&self.quote_type, "quoteType")?;
        let kind =
            parse_yahoo_quote_type(quote_type).map_err(|err| ProjectionIssue::InvalidField {
                field: "quoteType",
                details: err.to_string(),
            })?;

        let instrument = match exchange {
            Some(ex) => Instrument::from_symbol_and_exchange(sym, ex, kind),
            None => Instrument::from_symbol(sym, kind),
        };

        instrument.map_err(|err| ProjectionIssue::InvalidField {
            field: "symbol",
            details: err.to_string(),
        })
    }

    fn instrument(&self, exchange: Option<paft::domain::Exchange>) -> Result<Instrument, YfError> {
        self.instrument_projection(exchange)
            .map_err(|issue| self.instrument_error(issue))
    }

    fn instrument_error(&self, issue: ProjectionIssue) -> YfError {
        match issue {
            ProjectionIssue::MissingRequiredField { field } => {
                YfError::MissingData(format!("v7 quote node missing {field}"))
            }
            ProjectionIssue::InvalidField {
                field: "symbol",
                details,
            } => YfError::InvalidData(format!(
                "invalid v7 quote symbol {:?}: {details}",
                wire_str(&self.symbol).unwrap_or_default()
            )),
            ProjectionIssue::InvalidField { field, details } => {
                YfError::InvalidData(format!("invalid v7 quote {field}: {details}"))
            }
            other => YfError::InvalidData(format!("invalid v7 quote instrument: {other}")),
        }
    }

    fn positive_book_level(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<String>,
        price: Option<f64>,
        size: Option<u64>,
    ) -> Result<Option<BookLevel>, YfError> {
        let Some(price) = price.filter(|p| p.is_finite() && *p > 0.0) else {
            return Ok(None);
        };
        let price = self.currency_units().quote_price_amount(
            ctx,
            path,
            key,
            Some(price),
            "quote book level price",
        )?;
        Ok(price.map(|price| BookLevel::new(price, size.and_then(quantity_from_u64))))
    }

    fn market_state_with_context(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<String>,
    ) -> Result<Option<MarketState>, YfError> {
        let Some(value) =
            optional_quote_string(ctx, "marketState", key.clone(), &self.market_state)?
                .and_then(|value| nonempty(&value).map(str::to_owned))
        else {
            return Ok(None);
        };
        match value.as_str().parse() {
            Ok(state) => Ok(Some(state)),
            Err(err) => {
                ctx.omitted_present_field(
                    "marketState",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "marketState",
                        details: err.to_string(),
                    },
                )?;
                Ok(None)
            }
        }
    }

    fn as_of_with_context(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<String>,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, YfError> {
        let Some(timestamp) = optional_quote_i64(
            ctx,
            "regularMarketTime",
            key.clone(),
            &self.regular_market_time,
        )?
        else {
            return Ok(None);
        };
        match i64_to_datetime(timestamp) {
            Ok(timestamp) => Ok(Some(timestamp)),
            Err(err) => {
                ctx.omitted_present_field(
                    "regularMarketTime",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "regularMarketTime",
                        details: err.to_string(),
                    },
                )?;
                Ok(None)
            }
        }
    }

    pub(crate) fn to_snapshot_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<Snapshot, YfError> {
        let key = self.symbol_key();
        let exchange = self.exchange_with_context(ctx, key.as_deref())?;
        let currencies = self.currency_units();
        let currency = currencies
            .quote_currency()
            .map_err(|issue| YfError::InvalidData(format!("invalid snapshot currency: {issue}")))?;
        let name = optional_quote_string(ctx, "longName", key.clone(), &self.long_name)?.or(
            optional_quote_string(ctx, "shortName", key.clone(), &self.short_name)?,
        );
        let regular_market_price = optional_quote_f64(
            ctx,
            "regularMarketPrice",
            key.clone(),
            &self.regular_market_price,
        )?;
        let regular_market_previous_close = optional_quote_f64(
            ctx,
            "regularMarketPreviousClose",
            key.clone(),
            &self.regular_market_previous_close,
        )?;
        let regular_market_open = optional_quote_f64(
            ctx,
            "regularMarketOpen",
            key.clone(),
            &self.regular_market_open,
        )?;
        let regular_market_day_high = optional_quote_f64(
            ctx,
            "regularMarketDayHigh",
            key.clone(),
            &self.regular_market_day_high,
        )?;
        let regular_market_day_low = optional_quote_f64(
            ctx,
            "regularMarketDayLow",
            key.clone(),
            &self.regular_market_day_low,
        )?;
        let regular_market_volume = optional_quote_u64(
            ctx,
            "regularMarketVolume",
            key.clone(),
            &self.regular_market_volume,
        )?;

        Ok(Snapshot {
            instrument: self.instrument(exchange)?,
            name,
            market_state: self.market_state_with_context(ctx, key.clone())?,
            as_of: self.as_of_with_context(ctx, key.clone())?,
            currency,
            last: currencies.quote_price_amount(
                ctx,
                "regularMarketPrice",
                key.clone(),
                regular_market_price,
                "snapshot last price",
            )?,
            previous_close: currencies.quote_price_amount(
                ctx,
                "regularMarketPreviousClose",
                key.clone(),
                regular_market_previous_close,
                "snapshot previous close",
            )?,
            open: currencies.quote_price_amount(
                ctx,
                "regularMarketOpen",
                key.clone(),
                regular_market_open,
                "snapshot open",
            )?,
            day_high: currencies.quote_price_amount(
                ctx,
                "regularMarketDayHigh",
                key.clone(),
                regular_market_day_high,
                "snapshot day high",
            )?,
            day_low: currencies.quote_price_amount(
                ctx,
                "regularMarketDayLow",
                key,
                regular_market_day_low,
                "snapshot day low",
            )?,
            volume: regular_market_volume.and_then(quantity_from_u64),
            provider: (),
        })
    }

    pub(crate) fn to_fast_info_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<FastInfo, YfError> {
        Ok(FastInfo {
            snapshot: self.to_snapshot_with_context(ctx)?,
            moving_averages: self.to_moving_averages_with_context(ctx)?,
        })
    }

    pub(crate) fn to_moving_averages_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<MovingAverages, YfError> {
        let key = self.symbol_key();
        let currencies = self.currency_units();
        let fifty_day =
            optional_quote_f64(ctx, "fiftyDayAverage", key.clone(), &self.fifty_day_average)?;
        let two_hundred_day = optional_quote_f64(
            ctx,
            "twoHundredDayAverage",
            key.clone(),
            &self.two_hundred_day_average,
        )?;

        Ok(MovingAverages {
            fifty_day: currencies.quote_price(
                ctx,
                "fiftyDayAverage",
                key.clone(),
                fifty_day,
                "50-day moving average",
            )?,
            two_hundred_day: currencies.quote_price(
                ctx,
                "twoHundredDayAverage",
                key,
                two_hundred_day,
                "200-day moving average",
            )?,
        })
    }

    pub(crate) fn to_key_statistics_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<KeyStatistics, YfError> {
        let currencies = self.currency_units();
        let key = self.symbol_key();
        let fields = self.key_statistics_fields(ctx, key.clone())?;

        Ok(KeyStatistics {
            as_of: self.as_of_with_context(ctx, key.clone())?,
            market_cap: currencies.quote_money(
                ctx,
                "marketCap",
                key.clone(),
                fields.market_cap,
                "market cap",
            )?,
            shares_outstanding: fields.shares_outstanding,
            eps_trailing_twelve_months: currencies.financial_price(
                ctx,
                "epsTrailingTwelveMonths",
                key.clone(),
                fields.eps_trailing_twelve_months,
                "trailing EPS",
            )?,
            pe_trailing_twelve_months: optional_decimal_f64(
                ctx,
                "trailingPE",
                key.clone(),
                fields.trailing_pe,
                "trailing PE",
            )?,
            dividend_per_share_forward: currencies.quote_major_price(
                ctx,
                "dividendRate",
                key.clone(),
                fields.dividend_rate,
                "forward dividend per share",
            )?,
            dividend_yield_trailing: optional_decimal_f64(
                ctx,
                "trailingAnnualDividendYield",
                key.clone(),
                fields.trailing_annual_dividend_yield,
                "trailing dividend yield",
            )?,
            // Yahoo v7 returns trailingAnnualDividendYield as a decimal fraction,
            // but dividendYield as percent points. Keep this asymmetry fixture-locked.
            dividend_yield_forward: optional_decimal_f64(
                ctx,
                "dividendYield",
                key.clone(),
                fields.dividend_yield,
                "forward dividend yield",
            )?
            .map(|value| value / Decimal::from(100)),
            ex_dividend_date: None,
            fifty_two_week_high: currencies.quote_price(
                ctx,
                "fiftyTwoWeekHigh",
                key.clone(),
                fields.fifty_two_week_high,
                "52-week high",
            )?,
            fifty_two_week_low: currencies.quote_price(
                ctx,
                "fiftyTwoWeekLow",
                key.clone(),
                fields.fifty_two_week_low,
                "52-week low",
            )?,
            average_daily_volume_3m: fields.average_daily_volume_3m,
            beta: optional_decimal_f64(ctx, "beta", key, fields.beta, "beta")?,
        })
    }

    fn key_statistics_fields(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<String>,
    ) -> Result<V7KeyStatisticsFields, YfError> {
        Ok(V7KeyStatisticsFields {
            market_cap: optional_quote_decimal(ctx, "marketCap", key.clone(), &self.market_cap)?,
            shares_outstanding: optional_quote_u64(
                ctx,
                "sharesOutstanding",
                key.clone(),
                &self.shares_outstanding,
            )?,
            eps_trailing_twelve_months: optional_quote_f64(
                ctx,
                "epsTrailingTwelveMonths",
                key.clone(),
                &self.eps_trailing_twelve_months,
            )?,
            trailing_pe: optional_quote_f64(ctx, "trailingPE", key.clone(), &self.trailing_pe)?,
            dividend_rate: optional_quote_f64(
                ctx,
                "dividendRate",
                key.clone(),
                &self.dividend_rate,
            )?,
            trailing_annual_dividend_yield: optional_quote_f64(
                ctx,
                "trailingAnnualDividendYield",
                key.clone(),
                &self.trailing_annual_dividend_yield,
            )?,
            dividend_yield: optional_quote_f64(
                ctx,
                "dividendYield",
                key.clone(),
                &self.dividend_yield,
            )?,
            fifty_two_week_high: optional_quote_f64(
                ctx,
                "fiftyTwoWeekHigh",
                key.clone(),
                &self.fifty_two_week_high,
            )?,
            fifty_two_week_low: optional_quote_f64(
                ctx,
                "fiftyTwoWeekLow",
                key.clone(),
                &self.fifty_two_week_low,
            )?,
            average_daily_volume_3m: optional_quote_u64(
                ctx,
                "averageDailyVolume3Month",
                key.clone(),
                &self.average_daily_volume_3_month,
            )?,
            beta: optional_quote_f64(ctx, "beta", key, &self.beta)?,
        })
    }

    pub(crate) fn calendar_fallback_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<Option<Calendar>, YfError> {
        let key = self.symbol_key();
        let Some(timestamp) =
            optional_quote_i64(ctx, "dividendDate", key.clone(), &self.dividend_date)?
        else {
            return Ok(None);
        };
        match i64_to_date(timestamp) {
            Ok(date) => Ok(Some(Calendar {
                earnings_dates: Vec::new(),
                ex_dividend_date: None,
                dividend_payment_date: Some(date),
            })),
            Err(err) => {
                ctx.omitted_present_field(
                    "dividendDate",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "dividendDate",
                        details: err.to_string(),
                    },
                )?;
                Ok(None)
            }
        }
    }
}

struct ExchangeCandidateFailure {
    path: &'static str,
    value: String,
    reason: String,
}

fn exchange_candidate_failure_details(failures: &[ExchangeCandidateFailure]) -> String {
    let mut details = String::from("no exchange candidate parsed");
    for failure in failures {
        let _ = write!(
            details,
            "; {}={:?}: {}",
            failure.path, failure.value, failure.reason
        );
    }
    details
}

#[derive(Clone)]
struct QuoteCurrencyUnits {
    quote: Option<ResolvedCurrencyUnit>,
    quote_issue: Option<ProjectionIssue>,
    quote_major: Option<ResolvedCurrencyUnit>,
    financial: Option<ResolvedCurrencyUnit>,
    financial_issue: Option<ProjectionIssue>,
}

impl QuoteCurrencyUnits {
    fn from_quote_node(node: &V7QuoteNode) -> Self {
        let (quote, quote_issue) = parse_currency_unit(
            wire_str(&node.currency),
            node.currency.invalid_details(),
            "currency",
            false,
        );
        let quote_major = quote.as_ref().map(ResolvedCurrencyUnit::major_unit);
        let (financial, financial_issue) = node.financial_currency.invalid_details().map_or_else(
            || {
                wire_str(&node.financial_currency)
                    .and_then(nonempty)
                    .map_or_else(
                        || (quote_major.clone(), quote_issue.clone()),
                        |code| parse_currency_unit(Some(code), None, "financialCurrency", true),
                    )
            },
            |details| parse_currency_unit(None, Some(details), "financialCurrency", true),
        );

        Self {
            quote,
            quote_issue,
            quote_major,
            financial,
            financial_issue,
        }
    }

    fn from_quote_summary_currency(currency: Option<&str>) -> Self {
        let (quote, quote_issue) = parse_currency_unit(currency, None, "currency", false);
        let quote_major = quote.as_ref().map(ResolvedCurrencyUnit::major_unit);
        let financial = quote_major.clone();

        Self {
            quote,
            quote_issue: quote_issue.clone(),
            quote_major,
            financial,
            financial_issue: quote_issue,
        }
    }

    fn quote_price(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<String>,
        value: Option<f64>,
        target: &'static str,
    ) -> Result<Option<paft::money::Price>, YfError> {
        optional_with_unit(
            ctx,
            path,
            key,
            self.quote_unit(),
            value,
            target,
            ResolvedCurrencyUnit::price_from_f64,
        )
    }

    fn quote_price_amount(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<String>,
        value: Option<f64>,
        target: &'static str,
    ) -> Result<Option<PriceAmount>, YfError> {
        optional_with_unit(
            ctx,
            path,
            key,
            self.quote_unit(),
            value,
            target,
            ResolvedCurrencyUnit::price_amount_from_f64,
        )
    }

    fn quote_money(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<String>,
        value: Option<Decimal>,
        target: &'static str,
    ) -> Result<Option<paft::money::Money>, YfError> {
        optional_with_unit(
            ctx,
            path,
            key,
            self.quote_major_unit(),
            value,
            target,
            |unit, value| unit.money_from_decimal(value).ok(),
        )
    }

    fn quote_major_price(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<String>,
        value: Option<f64>,
        target: &'static str,
    ) -> Result<Option<paft::money::Price>, YfError> {
        optional_with_unit(
            ctx,
            path,
            key,
            self.quote_major_unit(),
            value,
            target,
            ResolvedCurrencyUnit::price_from_f64,
        )
    }

    fn financial_price(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<String>,
        value: Option<f64>,
        target: &'static str,
    ) -> Result<Option<paft::money::Price>, YfError> {
        optional_with_unit(
            ctx,
            path,
            key,
            self.financial_unit(),
            value,
            target,
            ResolvedCurrencyUnit::price_from_f64,
        )
    }

    fn quote_unit(&self) -> Result<&ResolvedCurrencyUnit, ProjectionIssue> {
        self.quote.as_ref().ok_or_else(|| self.quote_issue())
    }

    fn quote_currency(&self) -> Result<Currency, ProjectionIssue> {
        self.quote_unit().map(|unit| unit.currency().clone())
    }

    fn quote_major_unit(&self) -> Result<&ResolvedCurrencyUnit, ProjectionIssue> {
        self.quote_major.as_ref().ok_or_else(|| self.quote_issue())
    }

    fn financial_unit(&self) -> Result<&ResolvedCurrencyUnit, ProjectionIssue> {
        self.financial.as_ref().ok_or_else(|| {
            self.financial_issue
                .clone()
                .unwrap_or(ProjectionIssue::CurrencyUnresolved)
        })
    }

    fn quote_issue(&self) -> ProjectionIssue {
        self.quote_issue
            .clone()
            .unwrap_or(ProjectionIssue::CurrencyUnresolved)
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn parse_currency_unit(
    code: Option<&str>,
    invalid_details: Option<&str>,
    field: &'static str,
    major: bool,
) -> (Option<ResolvedCurrencyUnit>, Option<ProjectionIssue>) {
    if let Some(details) = invalid_details {
        return (
            None,
            Some(ProjectionIssue::InvalidField {
                field,
                details: details.to_string(),
            }),
        );
    }

    let Some(code) = code.and_then(nonempty) else {
        return (None, None);
    };
    let unit = if major {
        ResolvedCurrencyUnit::major_from_code(code)
    } else {
        ResolvedCurrencyUnit::from_code(code)
    };
    unit.map_or_else(
        || {
            (
                None,
                Some(ProjectionIssue::InvalidCurrency {
                    code: code.to_string(),
                }),
            )
        },
        |unit| (Some(unit), None),
    )
}

fn optional_with_unit<T, U>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Result<&ResolvedCurrencyUnit, ProjectionIssue>,
    value: Option<T>,
    target: &'static str,
    convert: impl FnOnce(&ResolvedCurrencyUnit, T) -> Option<U>,
) -> Result<Option<U>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let unit = match unit {
        Ok(unit) => unit,
        Err(issue) => {
            ctx.omitted_present_field(path, key, issue)?;
            return Ok(None);
        }
    };
    let Some(converted) = convert(unit, value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(converted))
}

#[derive(Deserialize)]
pub struct QuoteSummaryKeyStatistics {
    #[serde(rename = "summaryDetail")]
    summary_detail: Option<SummaryDetailNode>,
    #[serde(rename = "defaultKeyStatistics")]
    default_key_statistics: Option<DefaultKeyStatisticsNode>,
}

#[derive(Default, Deserialize)]
struct SummaryDetailNode {
    #[serde(default)]
    currency: WireValue<String>,
    #[serde(default)]
    beta: WireValue<RawNum<f64>>,
    #[serde(rename = "marketCap")]
    #[serde(default)]
    market_cap: WireValue<RawDecimal>,
    #[serde(rename = "trailingPE")]
    #[serde(default)]
    trailing_pe: WireValue<RawNum<f64>>,
    #[serde(rename = "dividendRate")]
    #[serde(default)]
    dividend_rate: WireValue<RawNum<f64>>,
    #[serde(rename = "dividendYield")]
    #[serde(default)]
    dividend_yield: WireValue<RawNum<f64>>,
    #[serde(rename = "trailingAnnualDividendYield")]
    #[serde(default)]
    trailing_annual_dividend_yield: WireValue<RawNum<f64>>,
    #[serde(rename = "exDividendDate")]
    #[serde(default)]
    ex_dividend_date: WireValue<RawDate>,
    #[serde(rename = "fiftyDayAverage")]
    #[serde(default)]
    fifty_day_average: WireValue<RawNum<f64>>,
    #[serde(rename = "twoHundredDayAverage")]
    #[serde(default)]
    two_hundred_day_average: WireValue<RawNum<f64>>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    #[serde(default)]
    fifty_two_week_high: WireValue<RawNum<f64>>,
    #[serde(rename = "fiftyTwoWeekLow")]
    #[serde(default)]
    fifty_two_week_low: WireValue<RawNum<f64>>,
    #[serde(rename = "averageVolume")]
    #[serde(default)]
    average_volume: WireValue<RawNumU64>,
}

#[derive(Default, Deserialize)]
struct DefaultKeyStatisticsNode {
    #[serde(default)]
    beta: WireValue<RawNum<f64>>,
    #[serde(rename = "sharesOutstanding")]
    #[serde(default)]
    shares_outstanding: WireValue<RawNumU64>,
    #[serde(rename = "trailingEps")]
    #[serde(default)]
    trailing_eps: WireValue<RawNum<f64>>,
}

fn optional_summary_raw_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<RawNum<f64>>,
) -> Result<Option<f64>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.and_then(|value| value.raw))
}

fn optional_summary_raw_u64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<RawNumU64>,
) -> Result<Option<u64>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.and_then(|value| value.raw))
}

fn optional_summary_raw_decimal(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<RawDecimal>,
) -> Result<Option<Decimal>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.and_then(|value| value.raw))
}

fn optional_summary_raw_date(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<RawDate>,
) -> Result<Option<i64>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.and_then(|value| value.raw))
}

struct QuoteSummaryKeyStatisticsFields {
    summary_currency: Option<String>,
    beta: Option<f64>,
    fifty_day_average: Option<f64>,
    two_hundred_day_average: Option<f64>,
    market_cap: Option<Decimal>,
    shares_outstanding: Option<u64>,
    trailing_eps: Option<f64>,
    trailing_pe: Option<f64>,
    dividend_rate: Option<f64>,
    trailing_annual_dividend_yield: Option<f64>,
    dividend_yield: Option<f64>,
    ex_dividend_date: Option<i64>,
    fifty_two_week_high: Option<f64>,
    fifty_two_week_low: Option<f64>,
    average_volume: Option<u64>,
}

fn quote_summary_key_statistics_fields(
    ctx: &mut ProjectionContext,
    key: Option<String>,
    summary_detail: &SummaryDetailNode,
    default_key_statistics: &DefaultKeyStatisticsNode,
) -> Result<QuoteSummaryKeyStatisticsFields, YfError> {
    let summary_beta =
        optional_summary_raw_f64(ctx, "summaryDetail.beta", key.clone(), &summary_detail.beta)?;
    let default_beta = optional_summary_raw_f64(
        ctx,
        "defaultKeyStatistics.beta",
        key.clone(),
        &default_key_statistics.beta,
    )?;

    Ok(QuoteSummaryKeyStatisticsFields {
        summary_currency: optional_quote_string(
            ctx,
            "summaryDetail.currency",
            key.clone(),
            &summary_detail.currency,
        )?,
        beta: summary_beta.or(default_beta),
        fifty_day_average: optional_summary_raw_f64(
            ctx,
            "summaryDetail.fiftyDayAverage",
            key.clone(),
            &summary_detail.fifty_day_average,
        )?,
        two_hundred_day_average: optional_summary_raw_f64(
            ctx,
            "summaryDetail.twoHundredDayAverage",
            key.clone(),
            &summary_detail.two_hundred_day_average,
        )?,
        market_cap: optional_summary_raw_decimal(
            ctx,
            "summaryDetail.marketCap",
            key.clone(),
            &summary_detail.market_cap,
        )?,
        shares_outstanding: optional_summary_raw_u64(
            ctx,
            "defaultKeyStatistics.sharesOutstanding",
            key.clone(),
            &default_key_statistics.shares_outstanding,
        )?,
        trailing_eps: optional_summary_raw_f64(
            ctx,
            "defaultKeyStatistics.trailingEps",
            key.clone(),
            &default_key_statistics.trailing_eps,
        )?,
        trailing_pe: optional_summary_raw_f64(
            ctx,
            "summaryDetail.trailingPE",
            key.clone(),
            &summary_detail.trailing_pe,
        )?,
        dividend_rate: optional_summary_raw_f64(
            ctx,
            "summaryDetail.dividendRate",
            key.clone(),
            &summary_detail.dividend_rate,
        )?,
        trailing_annual_dividend_yield: optional_summary_raw_f64(
            ctx,
            "summaryDetail.trailingAnnualDividendYield",
            key.clone(),
            &summary_detail.trailing_annual_dividend_yield,
        )?,
        dividend_yield: optional_summary_raw_f64(
            ctx,
            "summaryDetail.dividendYield",
            key.clone(),
            &summary_detail.dividend_yield,
        )?,
        ex_dividend_date: optional_summary_raw_date(
            ctx,
            "summaryDetail.exDividendDate",
            key.clone(),
            &summary_detail.ex_dividend_date,
        )?,
        fifty_two_week_high: optional_summary_raw_f64(
            ctx,
            "summaryDetail.fiftyTwoWeekHigh",
            key.clone(),
            &summary_detail.fifty_two_week_high,
        )?,
        fifty_two_week_low: optional_summary_raw_f64(
            ctx,
            "summaryDetail.fiftyTwoWeekLow",
            key.clone(),
            &summary_detail.fifty_two_week_low,
        )?,
        average_volume: optional_summary_raw_u64(
            ctx,
            "summaryDetail.averageVolume",
            key,
            &summary_detail.average_volume,
        )?,
    })
}

impl QuoteSummaryKeyStatistics {
    pub fn into_key_statistics_and_moving_averages_with_context(
        self,
        ctx: &mut ProjectionContext,
        symbol: &str,
    ) -> Result<(KeyStatistics, MovingAverages), YfError> {
        let key = Some(symbol.to_string());
        let summary_detail = self.summary_detail.unwrap_or_default();
        let default_key_statistics = self.default_key_statistics.unwrap_or_default();
        let fields = quote_summary_key_statistics_fields(
            ctx,
            key.clone(),
            &summary_detail,
            &default_key_statistics,
        )?;
        let currencies =
            QuoteCurrencyUnits::from_quote_summary_currency(fields.summary_currency.as_deref());
        let moving_averages = MovingAverages {
            fifty_day: currencies.quote_price(
                ctx,
                "summaryDetail.fiftyDayAverage",
                key.clone(),
                fields.fifty_day_average,
                "50-day moving average",
            )?,
            two_hundred_day: currencies.quote_price(
                ctx,
                "summaryDetail.twoHundredDayAverage",
                key.clone(),
                fields.two_hundred_day_average,
                "200-day moving average",
            )?,
        };

        let key_statistics = KeyStatistics {
            market_cap: currencies.quote_money(
                ctx,
                "summaryDetail.marketCap",
                key.clone(),
                fields.market_cap,
                "market cap",
            )?,
            shares_outstanding: fields.shares_outstanding,
            eps_trailing_twelve_months: currencies.financial_price(
                ctx,
                "defaultKeyStatistics.trailingEps",
                key.clone(),
                fields.trailing_eps,
                "trailing EPS",
            )?,
            pe_trailing_twelve_months: optional_decimal_f64(
                ctx,
                "summaryDetail.trailingPE",
                key.clone(),
                fields.trailing_pe,
                "trailing PE",
            )?,
            dividend_per_share_forward: currencies.quote_major_price(
                ctx,
                "summaryDetail.dividendRate",
                key.clone(),
                fields.dividend_rate,
                "forward dividend per share",
            )?,
            dividend_yield_trailing: optional_decimal_f64(
                ctx,
                "summaryDetail.trailingAnnualDividendYield",
                key.clone(),
                fields.trailing_annual_dividend_yield,
                "trailing dividend yield",
            )?,
            dividend_yield_forward: optional_decimal_f64(
                ctx,
                "summaryDetail.dividendYield",
                key.clone(),
                fields.dividend_yield,
                "forward dividend yield",
            )?,
            ex_dividend_date: optional_date(
                ctx,
                "summaryDetail.exDividendDate",
                key.clone(),
                fields.ex_dividend_date,
            )?,
            fifty_two_week_high: currencies.quote_price(
                ctx,
                "summaryDetail.fiftyTwoWeekHigh",
                key.clone(),
                fields.fifty_two_week_high,
                "52-week high",
            )?,
            fifty_two_week_low: currencies.quote_price(
                ctx,
                "summaryDetail.fiftyTwoWeekLow",
                key,
                fields.fifty_two_week_low,
                "52-week low",
            )?,
            average_daily_volume_3m: fields.average_volume,
            beta: optional_decimal_f64(ctx, "beta", Some(symbol.to_string()), fields.beta, "beta")?,
            as_of: None,
        };

        Ok((key_statistics, moving_averages))
    }

    pub fn into_key_statistics_with_context(
        self,
        ctx: &mut ProjectionContext,
        symbol: &str,
    ) -> Result<KeyStatistics, YfError> {
        Ok(self
            .into_key_statistics_and_moving_averages_with_context(ctx, symbol)?
            .0)
    }
}

fn optional_date(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    timestamp: Option<i64>,
) -> Result<Option<chrono::NaiveDate>, YfError> {
    let Some(timestamp) = timestamp else {
        return Ok(None);
    };

    match i64_to_date(timestamp) {
        Ok(date) => Ok(Some(date)),
        Err(err) => {
            ctx.omitted_present_field(
                path,
                key,
                ProjectionIssue::InvalidField {
                    field: path,
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
}

pub fn quote_summary_key_statistics_from_value(
    value: serde_json::Value,
) -> Result<QuoteSummaryKeyStatistics, YfError> {
    serde_json::from_value(value).map_err(YfError::Json)
}

pub fn merge_key_statistics(
    mut base: KeyStatistics,
    quote_summary: &KeyStatistics,
) -> KeyStatistics {
    if base.market_cap.is_none() {
        base.market_cap.clone_from(&quote_summary.market_cap);
    }
    if base.shares_outstanding.is_none() {
        base.shares_outstanding = quote_summary.shares_outstanding;
    }
    if base.eps_trailing_twelve_months.is_none() {
        base.eps_trailing_twelve_months
            .clone_from(&quote_summary.eps_trailing_twelve_months);
    }
    if base.pe_trailing_twelve_months.is_none() {
        base.pe_trailing_twelve_months = quote_summary.pe_trailing_twelve_months;
    }
    if base.dividend_per_share_forward.is_none() {
        base.dividend_per_share_forward
            .clone_from(&quote_summary.dividend_per_share_forward);
    }
    if base.dividend_yield_trailing.is_none() {
        base.dividend_yield_trailing = quote_summary.dividend_yield_trailing;
    }
    if base.dividend_yield_forward.is_none() {
        base.dividend_yield_forward = quote_summary.dividend_yield_forward;
    }
    if base.ex_dividend_date.is_none() {
        base.ex_dividend_date = quote_summary.ex_dividend_date;
    }
    if base.fifty_two_week_high.is_none() {
        base.fifty_two_week_high
            .clone_from(&quote_summary.fifty_two_week_high);
    }
    if base.fifty_two_week_low.is_none() {
        base.fifty_two_week_low
            .clone_from(&quote_summary.fifty_two_week_low);
    }
    if base.average_daily_volume_3m.is_none() {
        base.average_daily_volume_3m = quote_summary.average_daily_volume_3m;
    }
    if base.beta.is_none() {
        base.beta = quote_summary.beta;
    }
    base
}

pub fn merge_moving_averages(
    mut base: MovingAverages,
    quote_summary: &MovingAverages,
) -> MovingAverages {
    if base.fifty_day.is_none() {
        base.fifty_day.clone_from(&quote_summary.fifty_day);
    }
    if base.two_hundred_day.is_none() {
        base.two_hundred_day
            .clone_from(&quote_summary.two_hundred_day);
    }
    base
}

pub async fn fetch_quote_summary_key_statistics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<QuoteSummaryKeyStatistics, YfError> {
    quotesummary::fetch_module_result(
        client,
        symbol,
        KEY_STATISTICS_MODULES,
        "key_statistics",
        options,
    )
    .await
}

/// Centralized function to fetch one or more quotes from the v7 API.
/// It handles caching, retries, and authentication (crumb).
#[allow(clippy::too_many_lines)]
pub async fn fetch_v7_quotes(
    client: &YfClient,
    symbols: &[&str],
    options: &CallOptions,
) -> Result<Vec<V7QuoteNode>, YfError> {
    let values = fetch_v7_quote_values(client, symbols, options).await?;
    let mut ctx = ProjectionContext::new("quote_v7", options.data_quality());
    report_missing_requested_quote_values(symbols, &values, &mut ctx)?;
    let mut nodes = Vec::with_capacity(values.len());
    for (idx, value) in values.into_iter().enumerate() {
        if let Some(node) = quote_node_from_value_with_context(value, idx, &mut ctx)? {
            nodes.push(node);
        }
    }

    Ok(nodes)
}

pub fn report_missing_requested_quote_values(
    requested_symbols: &[&str],
    values: &[Value],
    ctx: &mut ProjectionContext,
) -> Result<(), YfError> {
    let normalized_symbols = normalize_symbols(requested_symbols.iter().copied())?;
    let requested_symbols = normalized_symbols
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut resolved_requests = vec![false; requested_symbols.len()];

    for value in values {
        let provider_symbol = value
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .and_then(|symbol| nonempty_symbol(Some(symbol)));
        mark_resolved_requested_symbol(&requested_symbols, &mut resolved_requests, provider_symbol);
    }

    for (symbol, resolved) in requested_symbols.iter().zip(resolved_requests) {
        if !resolved {
            ctx.dropped_item(
                "quote",
                Some((*symbol).to_string()),
                ProjectionIssue::ProviderUnavailable { feature: "quote" },
            )?;
        }
    }

    Ok(())
}

/// Centralized function to fetch raw quote nodes from the v7 API.
/// Projection callers can then choose strict or best-effort node conversion.
#[allow(clippy::too_many_lines)]
pub async fn fetch_v7_quote_values(
    client: &YfClient,
    symbols: &[&str],
    options: &CallOptions,
) -> Result<Vec<Value>, YfError> {
    if symbols.is_empty() {
        return Err(YfError::InvalidParams(
            "symbols list cannot be empty".into(),
        ));
    }

    let normalized_symbols = normalize_symbols(symbols.iter().copied())?;
    let mut url = client.base_quote_v7().clone();
    url.query_pairs_mut()
        .append_pair("symbols", &normalized_symbols.join(","));
    let fixture_key = normalized_symbols.join("-");

    let (body_to_parse, _) = net::fetch_text_with_auth_retry(
        client,
        url,
        net::AuthFetchConfig {
            auth_mode: net::AuthMode::OptionalCrumb,
            cache_endpoint: CacheEndpoint::Quote,
            options,
            cache_body: None,
            endpoint: "quote_v7",
            fixture_key: &fixture_key,
            ext: "json",
            retry_on_invalid_crumb_body: true,
            cache_validator: Some(validate_v7_quote_body),
        },
        |url| client.http().get(url).header("accept", "application/json"),
    )
    .await?;

    let env: V7Envelope = serde_json::from_str(&body_to_parse)?;
    let quote_response = env
        .quote_response
        .ok_or_else(|| YfError::MissingData("v7 quoteResponse missing".into()))?;
    reject_v7_quote_error(&quote_response)?;

    let nodes = quote_response
        .result
        .ok_or_else(|| YfError::MissingData("v7 quoteResponse.result missing".into()))?;

    store_v7_quote_side_effects_from_values(client, symbols, &nodes);

    Ok(nodes)
}

fn validate_v7_quote_body(body: &str) -> Result<(), YfError> {
    let env: V7Envelope = serde_json::from_str(body).map_err(YfError::Json)?;
    let quote_response = env
        .quote_response
        .ok_or_else(|| YfError::MissingData("v7 quoteResponse missing".into()))?;
    reject_v7_quote_error(&quote_response)
}

fn reject_v7_quote_error(quote_response: &V7QuoteResponse) -> Result<(), YfError> {
    if let Some(error) = quote_response.error.as_ref() {
        crate::core::logging::trace_error!(
            description = %error.description,
            "quoteResponse error"
        );
        return Err(YfError::Api(format!("yahoo error: {}", error.description)));
    }

    Ok(())
}

pub fn quote_node_from_value_with_context(
    value: Value,
    idx: usize,
    ctx: &mut ProjectionContext,
) -> Result<Option<V7QuoteNode>, YfError> {
    let key = Some(quote_node_diag_key_from_value(&value, idx));
    match serde_json::from_value(value) {
        Ok(node) => Ok(Some(node)),
        Err(err) => {
            ctx.dropped_item(
                "quote",
                key,
                ProjectionIssue::InvalidField {
                    field: "quote",
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
}

pub fn first_quote_node_from_values_with_context(
    values: Vec<Value>,
    ctx: &mut ProjectionContext,
) -> Result<Option<V7QuoteNode>, YfError> {
    for (idx, value) in values.into_iter().enumerate() {
        if let Some(node) = quote_node_from_value_with_context(value, idx, ctx)? {
            return Ok(Some(node));
        }
    }

    Ok(None)
}

pub fn required_quote_node_from_values_with_context(
    values: Vec<Value>,
    symbol: &str,
    ctx: &mut ProjectionContext,
) -> Result<V7QuoteNode, YfError> {
    first_quote_node_from_values_with_context(values, ctx)?.ok_or_else(|| {
        YfError::MissingData(format!("no valid quote result found for symbol {symbol}"))
    })
}

fn quote_node_diag_key_from_value(value: &Value, idx: usize) -> String {
    value
        .get("symbol")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map_or_else(|| format!("result[{idx}]"), ToString::to_string)
}

fn store_v7_quote_side_effects_from_values(
    client: &YfClient,
    requested_symbols: &[&str],
    values: &[Value],
) {
    let nodes = values
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect::<Vec<_>>();
    store_v7_quote_side_effects(client, requested_symbols, &nodes);
}

fn store_v7_quote_side_effects(
    client: &YfClient,
    requested_symbols: &[&str],
    nodes: &[V7QuoteNode],
) {
    let mut resolved_requests = vec![false; requested_symbols.len()];

    for node in nodes {
        let provider_symbol = nonempty_symbol(wire_str(&node.symbol));
        if let Some(symbol) = provider_symbol {
            store_quote_node_hints(client, symbol, node);
            store_requested_alias_hints(client, requested_symbols, symbol, node);
            store_quote_node_instrument(client, symbol, node);
        }

        mark_resolved_requested_symbol(requested_symbols, &mut resolved_requests, provider_symbol);

        if requested_symbols.len() == 1
            && provider_symbol.is_none_or(|symbol| !same_symbol(symbol, requested_symbols[0]))
        {
            store_quote_node_hints(client, requested_symbols[0], node);
        }
    }

    for (symbol, resolved) in requested_symbols.iter().zip(resolved_requests) {
        if !resolved {
            client.store_currency_hints(
                symbol,
                CurrencyHints::from_quote(None, None, None, None, None),
            );
        }
    }
}

fn store_quote_node_hints(client: &YfClient, symbol: &str, node: &V7QuoteNode) {
    client.store_currency_hints(
        symbol,
        CurrencyHints::from_quote(
            wire_str(&node.currency),
            wire_str(&node.financial_currency),
            wire_str(&node.exchange),
            wire_str(&node.full_exchange_name),
            wire_str(&node.quote_type),
        ),
    );
}

fn store_quote_node_instrument(client: &YfClient, symbol: &str, node: &V7QuoteNode) {
    let exch = node.exchange();
    let Some(kind) = wire_str(&node.quote_type).and_then(|s| parse_yahoo_quote_type(s).ok()) else {
        return;
    };

    let inst = match exch {
        Some(ex) => Instrument::from_symbol_and_exchange(symbol, ex, kind),
        None => Instrument::from_symbol(symbol, kind),
    };
    if let Ok(inst) = inst {
        client.store_instrument(symbol.to_string(), inst);
    }
}

fn store_requested_alias_hints(
    client: &YfClient,
    requested_symbols: &[&str],
    provider_symbol: &str,
    node: &V7QuoteNode,
) {
    for requested in requested_symbols {
        if same_symbol(provider_symbol, requested) && !same_cache_key(provider_symbol, requested) {
            store_quote_node_hints(client, requested, node);
        }
    }
}

fn mark_resolved_requested_symbol(
    requested_symbols: &[&str],
    resolved: &mut [bool],
    provider_symbol: Option<&str>,
) {
    if let Some(symbol) = provider_symbol {
        for (idx, requested) in requested_symbols.iter().enumerate() {
            if same_symbol(symbol, requested) {
                resolved[idx] = true;
            }
        }
    }

    if requested_symbols.len() == 1
        && provider_symbol.is_none_or(|symbol| !same_symbol(symbol, requested_symbols[0]))
    {
        resolved[0] = true;
    }
}

fn nonempty_symbol(symbol: Option<&str>) -> Option<&str> {
    symbol.map(str::trim).filter(|symbol| !symbol.is_empty())
}

fn same_symbol(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn same_cache_key(left: &str, right: &str) -> bool {
    left.trim() == right.trim()
}

impl TryFrom<V7QuoteNode> for Quote {
    type Error = YfError;

    fn try_from(n: V7QuoteNode) -> Result<Self, Self::Error> {
        let mut ctx = ProjectionContext::new("quote_v7", DataQuality::BestEffort);
        n.to_quote_with_context(&mut ctx)
    }
}

impl V7QuoteNode {
    pub(crate) fn to_quote_item_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<Option<Quote>, YfError> {
        let key = self.symbol_key();
        let exchange = self.exchange_with_context(ctx, key.as_deref())?;
        let instrument = match self.instrument_projection(exchange) {
            Ok(instrument) => instrument,
            Err(reason) => {
                ctx.dropped_item("quote", key, reason)?;
                return Ok(None);
            }
        };
        let currencies = self.currency_units();
        let currency = match currencies.quote_currency() {
            Ok(currency) => currency,
            Err(reason) => {
                ctx.dropped_item("quote", key, reason)?;
                return Ok(None);
            }
        };

        self.quote_from_instrument_with_context(ctx, key, instrument, currency)
            .map(Some)
    }

    pub(crate) fn to_quote_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<Quote, YfError> {
        let key = self.symbol_key();
        let exchange = self.exchange_with_context(ctx, key.as_deref())?;
        let instrument = self.instrument(exchange)?;
        let currency = self
            .currency_units()
            .quote_currency()
            .map_err(|issue| YfError::InvalidData(format!("invalid quote currency: {issue}")))?;

        self.quote_from_instrument_with_context(ctx, key, instrument, currency)
    }

    fn quote_from_instrument_with_context(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<String>,
        instrument: Instrument,
        currency: Currency,
    ) -> Result<Quote, YfError> {
        let currencies = self.currency_units();
        let name = optional_quote_string(ctx, "longName", key.clone(), &self.long_name)?.or(
            optional_quote_string(ctx, "shortName", key.clone(), &self.short_name)?,
        );
        let regular_market_price = optional_quote_f64(
            ctx,
            "regularMarketPrice",
            key.clone(),
            &self.regular_market_price,
        )?;
        let bid = optional_quote_f64(ctx, "bid", key.clone(), &self.bid)?;
        let bid_size = optional_quote_u64(ctx, "bidSize", key.clone(), &self.bid_size)?;
        let ask = optional_quote_f64(ctx, "ask", key.clone(), &self.ask)?;
        let ask_size = optional_quote_u64(ctx, "askSize", key.clone(), &self.ask_size)?;
        let regular_market_previous_close = optional_quote_f64(
            ctx,
            "regularMarketPreviousClose",
            key.clone(),
            &self.regular_market_previous_close,
        )?;
        let regular_market_volume = optional_quote_u64(
            ctx,
            "regularMarketVolume",
            key.clone(),
            &self.regular_market_volume,
        )?;

        Ok(Quote {
            instrument,
            name,
            currency,
            price: currencies.quote_price_amount(
                ctx,
                "regularMarketPrice",
                key.clone(),
                regular_market_price,
                "quote price",
            )?,
            bid: self.positive_book_level(ctx, "bid", key.clone(), bid, bid_size)?,
            ask: self.positive_book_level(ctx, "ask", key.clone(), ask, ask_size)?,
            previous_close: currencies.quote_price_amount(
                ctx,
                "regularMarketPreviousClose",
                key.clone(),
                regular_market_previous_close,
                "quote previous close",
            )?,
            day_volume: regular_market_volume.and_then(quantity_from_u64),
            market_state: self.market_state_with_context(ctx, key.clone())?,
            as_of: self.as_of_with_context(ctx, key)?,
            provider: (),
        })
    }
}
