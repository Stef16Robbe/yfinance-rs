// src/core/quotes.rs
use serde::Deserialize;

use crate::{
    YfClient, YfError,
    core::{
        DataQuality, ProjectionContext, ProjectionIssue,
        client::{CacheEndpoint, CacheMode, RetryConfig, normalize_symbols},
        conversions::{
            decimal_from_f64, i64_to_datetime, parse_exchange_str, string_to_asset_kind,
        },
        currency_resolver::{CurrencyHints, ResolvedCurrencyUnit},
        diagnostics::optional_decimal_f64,
        net, quotesummary,
        wire::{JsonDecimal, RawNum, from_raw},
    },
};
use paft::Decimal;
use paft::aggregates::Snapshot;
use paft::domain::{Exchange, Instrument, MarketState};
use paft::fundamentals::statements::Calendar;
use paft::fundamentals::statistics::KeyStatistics;
use paft::market::orderbook::BookLevel;
use paft::market::quote::Quote;

const KEY_STATISTICS_MODULES: &str = "summaryDetail,defaultKeyStatistics";

fn finite_decimal(value: Option<f64>) -> Option<Decimal> {
    value.and_then(decimal_from_f64)
}

// Centralized wire model for the v7 quote API
#[derive(Deserialize)]
pub struct V7Envelope {
    #[serde(rename = "quoteResponse")]
    pub(crate) quote_response: Option<V7QuoteResponse>,
}

#[derive(Deserialize)]
pub struct V7QuoteResponse {
    pub(crate) result: Option<Vec<V7QuoteNode>>,
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
    #[serde(rename = "regularMarketVolume")]
    pub(crate) regular_market_volume: Option<u64>,
    pub(crate) bid: Option<f64>,
    #[serde(rename = "bidSize")]
    pub(crate) bid_size: Option<u64>,
    pub(crate) ask: Option<f64>,
    #[serde(rename = "askSize")]
    pub(crate) ask_size: Option<u64>,
    #[serde(rename = "regularMarketTime")]
    pub(crate) regular_market_time: Option<i64>,
    #[serde(rename = "averageDailyVolume3Month")]
    pub(crate) average_daily_volume_3_month: Option<u64>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    pub(crate) fifty_two_week_high: Option<f64>,
    #[serde(rename = "fiftyTwoWeekLow")]
    pub(crate) fifty_two_week_low: Option<f64>,
    #[serde(rename = "marketCap")]
    pub(crate) market_cap: Option<JsonDecimal>,
    #[serde(rename = "sharesOutstanding")]
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
        self.exchange_candidates()
            .into_iter()
            .find_map(|(_, value)| {
                value
                    .and_then(nonempty)
                    .and_then(|value| parse_exchange_str(value).ok())
            })
    }

    fn exchange_with_context(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<&str>,
    ) -> Result<Option<Exchange>, YfError> {
        let mut exchange = None;
        for (path, value) in self.exchange_candidates() {
            let Some(value) = value.and_then(nonempty) else {
                continue;
            };
            match parse_exchange_str(value) {
                Ok(parsed) if exchange.is_none() => exchange = Some(parsed),
                Ok(_) => {}
                Err(err) => {
                    ctx.omitted_present_field(
                        path,
                        key.map(str::to_owned),
                        ProjectionIssue::InvalidField {
                            field: path,
                            details: err.to_string(),
                        },
                    )?;
                }
            }
        }
        Ok(exchange)
    }

    fn instrument(&self, exchange: Option<paft::domain::Exchange>) -> Result<Instrument, YfError> {
        let sym = self
            .symbol
            .as_deref()
            .filter(|symbol| !symbol.trim().is_empty())
            .ok_or_else(|| YfError::MissingData("v7 quote node missing symbol".into()))?;
        let kind = self
            .quote_type
            .as_deref()
            .ok_or_else(|| YfError::MissingData("v7 quote node missing quoteType".into()))
            .and_then(string_to_asset_kind)?;

        let instrument = match exchange {
            Some(ex) => Instrument::from_symbol_and_exchange(sym, ex, kind),
            None => Instrument::from_symbol(sym, kind),
        };

        instrument
            .map_err(|err| YfError::InvalidData(format!("invalid v7 quote symbol {sym:?}: {err}")))
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
        let price = self.currency_units().quote_price(
            ctx,
            path,
            key,
            Some(price),
            "quote book level price",
        )?;
        Ok(price.map(|price| BookLevel::new(price, size.map(Decimal::from))))
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

        Ok(Snapshot {
            instrument: self.instrument(exchange)?,
            name: self.long_name.clone().or_else(|| self.short_name.clone()),
            market_state: self.market_state_with_context(ctx, key.clone())?,
            as_of: self.as_of_with_context(ctx, key.clone())?,
            last: currencies.quote_price(
                ctx,
                "regularMarketPrice",
                key.clone(),
                self.regular_market_price,
                "snapshot last price",
            )?,
            previous_close: currencies.quote_price(
                ctx,
                "regularMarketPreviousClose",
                key.clone(),
                self.regular_market_previous_close,
                "snapshot previous close",
            )?,
            open: currencies.quote_price(
                ctx,
                "regularMarketOpen",
                key.clone(),
                self.regular_market_open,
                "snapshot open",
            )?,
            day_high: currencies.quote_price(
                ctx,
                "regularMarketDayHigh",
                key.clone(),
                self.regular_market_day_high,
                "snapshot day high",
            )?,
            day_low: currencies.quote_price(
                ctx,
                "regularMarketDayLow",
                key,
                self.regular_market_day_low,
                "snapshot day low",
            )?,
            volume: self.regular_market_volume,
            provider: (),
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
            dividend_per_share_forward: currencies.financial_price(
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
        match i64_to_datetime(timestamp) {
            Ok(timestamp) => Ok(Some(Calendar {
                earnings_dates: Vec::new(),
                ex_dividend_date: None,
                dividend_payment_date: Some(timestamp),
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
struct QuoteSummaryKeyStatistics {
    #[serde(rename = "summaryDetail")]
    summary_detail: Option<SummaryDetailNode>,
    #[serde(rename = "defaultKeyStatistics")]
    default_key_statistics: Option<DefaultKeyStatisticsNode>,
}

#[derive(Deserialize)]
struct SummaryDetailNode {
    beta: Option<RawNum<f64>>,
}

#[derive(Deserialize)]
struct DefaultKeyStatisticsNode {
    beta: Option<RawNum<f64>>,
}

impl QuoteSummaryKeyStatistics {
    fn into_key_statistics(self) -> KeyStatistics {
        let beta = self
            .summary_detail
            .and_then(|node| from_raw(node.beta))
            .or_else(|| {
                self.default_key_statistics
                    .and_then(|node| from_raw(node.beta))
            });

        KeyStatistics {
            beta: finite_decimal(beta),
            ..KeyStatistics::default()
        }
    }
}

pub fn merge_key_statistics(
    mut base: KeyStatistics,
    quote_summary: &KeyStatistics,
) -> KeyStatistics {
    base.beta = base.beta.or(quote_summary.beta);
    base
}

pub async fn fetch_quote_summary_key_statistics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<KeyStatistics, YfError> {
    let root: QuoteSummaryKeyStatistics = quotesummary::fetch_module_result(
        client,
        symbol,
        KEY_STATISTICS_MODULES,
        "key_statistics",
        cache_mode,
        retry_override,
    )
    .await?;

    Ok(root.into_key_statistics())
}

/// Centralized function to fetch one or more quotes from the v7 API.
/// It handles caching, retries, and authentication (crumb).
#[allow(clippy::too_many_lines)]
pub async fn fetch_v7_quotes(
    client: &YfClient,
    symbols: &[&str],
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<V7QuoteNode>, YfError> {
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
            cache_mode,
            cache_body: None,
            retry_override,
            endpoint: "quote_v7",
            fixture_key: &fixture_key,
            ext: "json",
            retry_on_invalid_crumb_body: true,
        },
        |url| client.http().get(url).header("accept", "application/json"),
    )
    .await?;

    let env: V7Envelope = serde_json::from_str(&body_to_parse)?;
    let quote_response = env
        .quote_response
        .ok_or_else(|| YfError::MissingData("v7 quoteResponse missing".into()))?;
    if let Some(error) = quote_response.error.as_ref() {
        crate::core::logging::trace_error!(
            description = %error.description,
            "quoteResponse error"
        );
        return Err(YfError::Api(format!("yahoo error: {}", error.description)));
    }

    let nodes = quote_response
        .result
        .ok_or_else(|| YfError::MissingData("v7 quoteResponse.result missing".into()))?;

    store_v7_quote_side_effects(client, symbols, &nodes).await;

    Ok(nodes)
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
            store_requested_alias_hints(
                client,
                requested_symbols,
                &mut resolved_requests,
                symbol,
                node,
            )
            .await;
            store_quote_node_instrument(client, symbol, node).await;
        }

        if requested_symbols.len() == 1 {
            let requested = requested_symbols[0];
            if provider_symbol.is_none_or(|symbol| !same_symbol(symbol, requested)) {
                store_quote_node_hints(client, requested, node).await;
                resolved_requests[0] = true;
            }
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
        .and_then(|s| string_to_asset_kind(s).ok())
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
    resolved: &mut [bool],
    provider_symbol: &str,
    node: &V7QuoteNode,
) {
    for (idx, requested) in requested_symbols.iter().enumerate() {
        if same_symbol(provider_symbol, requested) {
            resolved[idx] = true;
            if !same_cache_key(provider_symbol, requested) {
                store_quote_node_hints(client, requested, node).await;
            }
        }
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
    pub(crate) fn to_quote_with_context(
        &self,
        ctx: &mut ProjectionContext,
    ) -> Result<Quote, YfError> {
        let key = self.symbol.clone();
        let exchange = self.exchange_with_context(ctx, key.as_deref())?;
        let instrument = self.instrument(exchange)?;
        let currencies = self.currency_units();

        Ok(Quote {
            instrument,
            name: self.long_name.clone().or_else(|| self.short_name.clone()),
            price: currencies.quote_price(
                ctx,
                "regularMarketPrice",
                key.clone(),
                self.regular_market_price,
                "quote price",
            )?,
            bid: self.positive_book_level(ctx, "bid", key.clone(), self.bid, self.bid_size)?,
            ask: self.positive_book_level(ctx, "ask", key.clone(), self.ask, self.ask_size)?,
            previous_close: currencies.quote_price(
                ctx,
                "regularMarketPreviousClose",
                key.clone(),
                self.regular_market_previous_close,
                "quote previous close",
            )?,
            day_volume: self.regular_market_volume,
            market_state: self.market_state_with_context(ctx, key.clone())?,
            as_of: self.as_of_with_context(ctx, key)?,
            provider: (),
        })
    }
}
