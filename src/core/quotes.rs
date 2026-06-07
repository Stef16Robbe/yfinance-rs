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
        diagnostics::optional_decimal_f64,
        models::{FastInfo, MovingAverages},
        net, quotesummary,
        wire::{
            JsonDecimal, RawDate, RawDecimal, RawNum, RawNumU64, de_u64_from_json, from_raw,
            from_raw_date,
        },
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
    pub(crate) symbol: Option<String>,
    #[serde(rename = "quoteType")]
    pub(crate) quote_type: Option<String>,
    #[serde(rename = "shortName")]
    pub(crate) short_name: Option<String>,
    #[serde(rename = "longName")]
    pub(crate) long_name: Option<String>,
    #[serde(rename = "regularMarketPrice")]
    pub(crate) regular_market_price: Option<f64>,
    #[serde(rename = "regularMarketOpen")]
    pub(crate) regular_market_open: Option<f64>,
    #[serde(rename = "regularMarketDayHigh")]
    pub(crate) regular_market_day_high: Option<f64>,
    #[serde(rename = "regularMarketDayLow")]
    pub(crate) regular_market_day_low: Option<f64>,
    #[serde(rename = "regularMarketPreviousClose")]
    pub(crate) regular_market_previous_close: Option<f64>,
    #[serde(
        rename = "regularMarketVolume",
        default,
        deserialize_with = "de_u64_from_json"
    )]
    pub(crate) regular_market_volume: Option<u64>,
    pub(crate) bid: Option<f64>,
    #[serde(rename = "bidSize", default, deserialize_with = "de_u64_from_json")]
    pub(crate) bid_size: Option<u64>,
    pub(crate) ask: Option<f64>,
    #[serde(rename = "askSize", default, deserialize_with = "de_u64_from_json")]
    pub(crate) ask_size: Option<u64>,
    #[serde(rename = "regularMarketTime")]
    pub(crate) regular_market_time: Option<i64>,
    #[serde(
        rename = "averageDailyVolume3Month",
        default,
        deserialize_with = "de_u64_from_json"
    )]
    pub(crate) average_daily_volume_3_month: Option<u64>,
    #[serde(rename = "fiftyDayAverage")]
    pub(crate) fifty_day_average: Option<f64>,
    #[serde(rename = "twoHundredDayAverage")]
    pub(crate) two_hundred_day_average: Option<f64>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    pub(crate) fifty_two_week_high: Option<f64>,
    #[serde(rename = "fiftyTwoWeekLow")]
    pub(crate) fifty_two_week_low: Option<f64>,
    #[serde(rename = "marketCap")]
    pub(crate) market_cap: Option<JsonDecimal>,
    #[serde(
        rename = "sharesOutstanding",
        default,
        deserialize_with = "de_u64_from_json"
    )]
    pub(crate) shares_outstanding: Option<u64>,
    #[serde(rename = "epsTrailingTwelveMonths")]
    pub(crate) eps_trailing_twelve_months: Option<f64>,
    #[serde(rename = "trailingPE")]
    pub(crate) trailing_pe: Option<f64>,
    #[serde(rename = "trailingAnnualDividendYield")]
    pub(crate) trailing_annual_dividend_yield: Option<f64>,
    #[serde(rename = "dividendRate")]
    pub(crate) dividend_rate: Option<f64>,
    #[serde(rename = "dividendYield")]
    pub(crate) dividend_yield: Option<f64>,
    pub(crate) beta: Option<f64>,
    #[serde(rename = "dividendDate")]
    pub(crate) dividend_date: Option<i64>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "financialCurrency")]
    pub(crate) financial_currency: Option<String>,
    #[serde(rename = "fullExchangeName")]
    pub(crate) full_exchange_name: Option<String>,
    pub(crate) exchange: Option<String>,
    pub(crate) market: Option<String>,
    #[serde(rename = "marketCapFigureExchange")]
    pub(crate) market_cap_figure_exchange: Option<String>,
    #[serde(rename = "marketState")]
    pub(crate) market_state: Option<String>,
}

impl V7QuoteNode {
    fn currency_units(&self) -> QuoteCurrencyUnits {
        QuoteCurrencyUnits::from_quote_node(self)
    }

    fn exchange_candidates(&self) -> [(&'static str, Option<&str>); 4] {
        [
            ("fullExchangeName", self.full_exchange_name.as_deref()),
            ("exchange", self.exchange.as_deref()),
            ("market", self.market.as_deref()),
            (
                "marketCapFigureExchange",
                self.market_cap_figure_exchange.as_deref(),
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
        let mut failures = Vec::new();
        for (path, value) in self.exchange_candidates() {
            let Some(value) = value.and_then(nonempty) else {
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
                key.map(str::to_owned),
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
        let sym = self
            .symbol
            .as_deref()
            .filter(|symbol| !symbol.trim().is_empty())
            .ok_or(ProjectionIssue::MissingRequiredField { field: "symbol" })?;
        let kind = self
            .quote_type
            .as_deref()
            .ok_or(ProjectionIssue::MissingRequiredField { field: "quoteType" })
            .and_then(|quote_type| {
                parse_yahoo_quote_type(quote_type).map_err(|err| ProjectionIssue::InvalidField {
                    field: "quoteType",
                    details: err.to_string(),
                })
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
                self.symbol.as_deref().unwrap_or_default()
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
        let Some(value) = self.market_state.as_deref().and_then(nonempty) else {
            return Ok(None);
        };
        match value.parse() {
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
        let Some(timestamp) = self.regular_market_time else {
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
        let key = self.symbol.clone();
        let exchange = self.exchange_with_context(ctx, key.as_deref())?;
        let currencies = self.currency_units();
        let currency = currencies
            .quote_currency()
            .map_err(|issue| YfError::InvalidData(format!("invalid snapshot currency: {issue}")))?;

        Ok(Snapshot {
            instrument: self.instrument(exchange)?,
            name: self.long_name.clone().or_else(|| self.short_name.clone()),
            market_state: self.market_state_with_context(ctx, key.clone())?,
            as_of: self.as_of_with_context(ctx, key.clone())?,
            currency,
            last: currencies.quote_price_amount(
                ctx,
                "regularMarketPrice",
                key.clone(),
                self.regular_market_price,
                "snapshot last price",
            )?,
            previous_close: currencies.quote_price_amount(
                ctx,
                "regularMarketPreviousClose",
                key.clone(),
                self.regular_market_previous_close,
                "snapshot previous close",
            )?,
            open: currencies.quote_price_amount(
                ctx,
                "regularMarketOpen",
                key.clone(),
                self.regular_market_open,
                "snapshot open",
            )?,
            day_high: currencies.quote_price_amount(
                ctx,
                "regularMarketDayHigh",
                key.clone(),
                self.regular_market_day_high,
                "snapshot day high",
            )?,
            day_low: currencies.quote_price_amount(
                ctx,
                "regularMarketDayLow",
                key,
                self.regular_market_day_low,
                "snapshot day low",
            )?,
            volume: self.regular_market_volume.and_then(quantity_from_u64),
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
        let key = self.symbol.clone();
        let currencies = self.currency_units();

        Ok(MovingAverages {
            fifty_day: currencies.quote_price(
                ctx,
                "fiftyDayAverage",
                key.clone(),
                self.fifty_day_average,
                "50-day moving average",
            )?,
            two_hundred_day: currencies.quote_price(
                ctx,
                "twoHundredDayAverage",
                key,
                self.two_hundred_day_average,
                "200-day moving average",
            )?,
        })
    }

    pub(crate) fn to_key_statistics_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<KeyStatistics, YfError> {
        let currencies = self.currency_units();
        let key = self.symbol.clone();

        Ok(KeyStatistics {
            as_of: self.as_of_with_context(ctx, key.clone())?,
            market_cap: currencies.quote_money(
                ctx,
                "marketCap",
                key.clone(),
                self.market_cap.map(JsonDecimal::into_decimal),
                "market cap",
            )?,
            shares_outstanding: self.shares_outstanding,
            eps_trailing_twelve_months: currencies.financial_price(
                ctx,
                "epsTrailingTwelveMonths",
                key.clone(),
                self.eps_trailing_twelve_months,
                "trailing EPS",
            )?,
            pe_trailing_twelve_months: optional_decimal_f64(
                ctx,
                "trailingPE",
                key.clone(),
                self.trailing_pe,
                "trailing PE",
            )?,
            dividend_per_share_forward: currencies.quote_major_price(
                ctx,
                "dividendRate",
                key.clone(),
                self.dividend_rate,
                "forward dividend per share",
            )?,
            dividend_yield_trailing: optional_decimal_f64(
                ctx,
                "trailingAnnualDividendYield",
                key.clone(),
                self.trailing_annual_dividend_yield,
                "trailing dividend yield",
            )?,
            // Yahoo v7 returns trailingAnnualDividendYield as a decimal fraction,
            // but dividendYield as percent points. Keep this asymmetry fixture-locked.
            dividend_yield_forward: optional_decimal_f64(
                ctx,
                "dividendYield",
                key.clone(),
                self.dividend_yield,
                "forward dividend yield",
            )?
            .map(|value| value / Decimal::from(100)),
            ex_dividend_date: None,
            fifty_two_week_high: currencies.quote_price(
                ctx,
                "fiftyTwoWeekHigh",
                key.clone(),
                self.fifty_two_week_high,
                "52-week high",
            )?,
            fifty_two_week_low: currencies.quote_price(
                ctx,
                "fiftyTwoWeekLow",
                key.clone(),
                self.fifty_two_week_low,
                "52-week low",
            )?,
            average_daily_volume_3m: self.average_daily_volume_3_month,
            beta: optional_decimal_f64(ctx, "beta", key, self.beta, "beta")?,
        })
    }

    pub(crate) fn calendar_fallback_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<Option<Calendar>, YfError> {
        let Some(timestamp) = self.dividend_date else {
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
                    self.symbol.clone(),
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
    quote_invalid: Option<String>,
    quote_major: Option<ResolvedCurrencyUnit>,
    financial: Option<ResolvedCurrencyUnit>,
    financial_invalid: Option<String>,
}

impl QuoteCurrencyUnits {
    fn from_quote_node(node: &V7QuoteNode) -> Self {
        let (quote, quote_invalid) = parse_currency_unit(node.currency.as_deref(), false);
        let quote_major = quote.as_ref().map(ResolvedCurrencyUnit::major_unit);
        let (financial, financial_invalid) = node
            .financial_currency
            .as_deref()
            .and_then(nonempty)
            .map_or_else(
                || (quote_major.clone(), None),
                |code| parse_currency_unit(Some(code), true),
            );

        Self {
            quote,
            quote_invalid,
            quote_major,
            financial,
            financial_invalid,
        }
    }

    fn from_quote_summary_currency(currency: Option<&str>) -> Self {
        let (quote, quote_invalid) = parse_currency_unit(currency, false);
        let quote_major = quote.as_ref().map(ResolvedCurrencyUnit::major_unit);
        let financial = quote_major.clone();

        Self {
            quote,
            quote_invalid,
            quote_major,
            financial,
            financial_invalid: None,
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
            self.financial_invalid
                .as_ref()
                .or(self.quote_invalid.as_ref())
                .map_or(ProjectionIssue::CurrencyUnresolved, |code| {
                    ProjectionIssue::InvalidCurrency { code: code.clone() }
                })
        })
    }

    fn quote_issue(&self) -> ProjectionIssue {
        self.quote_invalid
            .as_ref()
            .map_or(ProjectionIssue::CurrencyUnresolved, |code| {
                ProjectionIssue::InvalidCurrency { code: code.clone() }
            })
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn parse_currency_unit(
    code: Option<&str>,
    major: bool,
) -> (Option<ResolvedCurrencyUnit>, Option<String>) {
    let Some(code) = code.and_then(nonempty) else {
        return (None, None);
    };
    let unit = if major {
        ResolvedCurrencyUnit::major_from_code(code)
    } else {
        ResolvedCurrencyUnit::from_code(code)
    };
    unit.map_or_else(|| (None, Some(code.to_string())), |unit| (Some(unit), None))
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
    currency: Option<String>,
    beta: Option<RawNum<f64>>,
    #[serde(rename = "marketCap")]
    market_cap: Option<RawDecimal>,
    #[serde(rename = "trailingPE")]
    trailing_pe: Option<RawNum<f64>>,
    #[serde(rename = "dividendRate")]
    dividend_rate: Option<RawNum<f64>>,
    #[serde(rename = "dividendYield")]
    dividend_yield: Option<RawNum<f64>>,
    #[serde(rename = "trailingAnnualDividendYield")]
    trailing_annual_dividend_yield: Option<RawNum<f64>>,
    #[serde(rename = "exDividendDate")]
    ex_dividend_date: Option<RawDate>,
    #[serde(rename = "fiftyDayAverage")]
    fifty_day_average: Option<RawNum<f64>>,
    #[serde(rename = "twoHundredDayAverage")]
    two_hundred_day_average: Option<RawNum<f64>>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    fifty_two_week_high: Option<RawNum<f64>>,
    #[serde(rename = "fiftyTwoWeekLow")]
    fifty_two_week_low: Option<RawNum<f64>>,
    #[serde(rename = "averageVolume")]
    average_volume: Option<RawNumU64>,
}

#[derive(Default, Deserialize)]
struct DefaultKeyStatisticsNode {
    beta: Option<RawNum<f64>>,
    #[serde(rename = "sharesOutstanding")]
    shares_outstanding: Option<RawNumU64>,
    #[serde(rename = "trailingEps")]
    trailing_eps: Option<RawNum<f64>>,
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
        let currencies =
            QuoteCurrencyUnits::from_quote_summary_currency(summary_detail.currency.as_deref());
        let beta = from_raw(summary_detail.beta).or_else(|| from_raw(default_key_statistics.beta));
        let moving_averages = MovingAverages {
            fifty_day: currencies.quote_price(
                ctx,
                "summaryDetail.fiftyDayAverage",
                key.clone(),
                from_raw(summary_detail.fifty_day_average),
                "50-day moving average",
            )?,
            two_hundred_day: currencies.quote_price(
                ctx,
                "summaryDetail.twoHundredDayAverage",
                key.clone(),
                from_raw(summary_detail.two_hundred_day_average),
                "200-day moving average",
            )?,
        };

        let key_statistics = KeyStatistics {
            market_cap: currencies.quote_money(
                ctx,
                "summaryDetail.marketCap",
                key.clone(),
                raw_decimal(summary_detail.market_cap),
                "market cap",
            )?,
            shares_outstanding: raw_u64(default_key_statistics.shares_outstanding),
            eps_trailing_twelve_months: currencies.financial_price(
                ctx,
                "defaultKeyStatistics.trailingEps",
                key.clone(),
                from_raw(default_key_statistics.trailing_eps),
                "trailing EPS",
            )?,
            pe_trailing_twelve_months: optional_decimal_f64(
                ctx,
                "summaryDetail.trailingPE",
                key.clone(),
                from_raw(summary_detail.trailing_pe),
                "trailing PE",
            )?,
            dividend_per_share_forward: currencies.quote_major_price(
                ctx,
                "summaryDetail.dividendRate",
                key.clone(),
                from_raw(summary_detail.dividend_rate),
                "forward dividend per share",
            )?,
            dividend_yield_trailing: optional_decimal_f64(
                ctx,
                "summaryDetail.trailingAnnualDividendYield",
                key.clone(),
                from_raw(summary_detail.trailing_annual_dividend_yield),
                "trailing dividend yield",
            )?,
            dividend_yield_forward: optional_decimal_f64(
                ctx,
                "summaryDetail.dividendYield",
                key.clone(),
                from_raw(summary_detail.dividend_yield),
                "forward dividend yield",
            )?,
            ex_dividend_date: optional_date(
                ctx,
                "summaryDetail.exDividendDate",
                key.clone(),
                from_raw_date(summary_detail.ex_dividend_date),
            )?,
            fifty_two_week_high: currencies.quote_price(
                ctx,
                "summaryDetail.fiftyTwoWeekHigh",
                key.clone(),
                from_raw(summary_detail.fifty_two_week_high),
                "52-week high",
            )?,
            fifty_two_week_low: currencies.quote_price(
                ctx,
                "summaryDetail.fiftyTwoWeekLow",
                key,
                from_raw(summary_detail.fifty_two_week_low),
                "52-week low",
            )?,
            average_daily_volume_3m: raw_u64(summary_detail.average_volume),
            beta: optional_decimal_f64(ctx, "beta", Some(symbol.to_string()), beta, "beta")?,
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

fn raw_decimal(raw: Option<RawDecimal>) -> Option<Decimal> {
    raw.and_then(|value| value.raw)
}

fn raw_u64(raw: Option<RawNumU64>) -> Option<u64> {
    raw.and_then(|value| value.raw)
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

    store_v7_quote_side_effects_from_values(client, symbols, &nodes).await;

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

async fn store_v7_quote_side_effects_from_values(
    client: &YfClient,
    requested_symbols: &[&str],
    values: &[Value],
) {
    let nodes = values
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect::<Vec<_>>();
    store_v7_quote_side_effects(client, requested_symbols, &nodes).await;
}

async fn store_v7_quote_side_effects(
    client: &YfClient,
    requested_symbols: &[&str],
    nodes: &[V7QuoteNode],
) {
    let mut resolved_requests = vec![false; requested_symbols.len()];

    for node in nodes {
        let provider_symbol = nonempty_symbol(node.symbol.as_deref());
        if let Some(symbol) = provider_symbol {
            store_quote_node_hints(client, symbol, node).await;
            store_requested_alias_hints(client, requested_symbols, symbol, node).await;
            store_quote_node_instrument(client, symbol, node).await;
        }

        mark_resolved_requested_symbol(requested_symbols, &mut resolved_requests, provider_symbol);

        if requested_symbols.len() == 1
            && provider_symbol.is_none_or(|symbol| !same_symbol(symbol, requested_symbols[0]))
        {
            store_quote_node_hints(client, requested_symbols[0], node).await;
        }
    }

    for (symbol, resolved) in requested_symbols.iter().zip(resolved_requests) {
        if !resolved {
            client
                .store_currency_hints(
                    symbol,
                    CurrencyHints::from_quote(None, None, None, None, None),
                )
                .await;
        }
    }
}

async fn store_quote_node_hints(client: &YfClient, symbol: &str, node: &V7QuoteNode) {
    client
        .store_currency_hints(
            symbol,
            CurrencyHints::from_quote(
                node.currency.as_deref(),
                node.financial_currency.as_deref(),
                node.exchange.as_deref(),
                node.full_exchange_name.as_deref(),
                node.quote_type.as_deref(),
            ),
        )
        .await;
}

async fn store_quote_node_instrument(client: &YfClient, symbol: &str, node: &V7QuoteNode) {
    let exch = node.exchange();
    let Some(kind) = node
        .quote_type
        .as_deref()
        .and_then(|s| parse_yahoo_quote_type(s).ok())
    else {
        return;
    };

    let inst = match exch {
        Some(ex) => Instrument::from_symbol_and_exchange(symbol, ex, kind),
        None => Instrument::from_symbol(symbol, kind),
    };
    if let Ok(inst) = inst {
        client.store_instrument(symbol.to_string(), inst).await;
    }
}

async fn store_requested_alias_hints(
    client: &YfClient,
    requested_symbols: &[&str],
    provider_symbol: &str,
    node: &V7QuoteNode,
) {
    for requested in requested_symbols {
        if same_symbol(provider_symbol, requested) && !same_cache_key(provider_symbol, requested) {
            store_quote_node_hints(client, requested, node).await;
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
        let key = self.symbol.clone();
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
        let key = self.symbol.clone();
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

        Ok(Quote {
            instrument,
            name: self.long_name.clone().or_else(|| self.short_name.clone()),
            currency,
            price: currencies.quote_price_amount(
                ctx,
                "regularMarketPrice",
                key.clone(),
                self.regular_market_price,
                "quote price",
            )?,
            bid: self.positive_book_level(ctx, "bid", key.clone(), self.bid, self.bid_size)?,
            ask: self.positive_book_level(ctx, "ask", key.clone(), self.ask, self.ask_size)?,
            previous_close: currencies.quote_price_amount(
                ctx,
                "regularMarketPreviousClose",
                key.clone(),
                self.regular_market_previous_close,
                "quote previous close",
            )?,
            day_volume: self.regular_market_volume.and_then(quantity_from_u64),
            market_state: self.market_state_with_context(ctx, key.clone())?,
            as_of: self.as_of_with_context(ctx, key)?,
            provider: (),
        })
    }
}
