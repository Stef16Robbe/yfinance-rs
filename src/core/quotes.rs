// src/core/quotes.rs
use serde::Deserialize;
use url::Url;

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheMode, RetryConfig},
        conversions::{decimal_from_f64, i64_to_datetime, string_to_asset_kind},
        currency_resolver::{CurrencyHints, ResolvedCurrencyUnit},
        net, quotesummary,
        wire::{RawNum, from_raw},
    },
};
use paft::Decimal;
use paft::aggregates::Snapshot;
use paft::domain::Instrument;
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
    #[allow(dead_code)]
    pub(crate) error: Option<serde_json::Value>,
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
    pub(crate) market_cap: Option<f64>,
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

    fn exchange(&self) -> Option<paft::domain::Exchange> {
        crate::core::conversions::string_to_exchange(
            self.full_exchange_name
                .clone()
                .or_else(|| self.exchange.clone())
                .or_else(|| self.market.clone())
                .or_else(|| self.market_cap_figure_exchange.clone()),
        )
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

    fn positive_book_level(&self, price: Option<f64>, size: Option<u64>) -> Option<BookLevel> {
        let price = price.filter(|p| p.is_finite() && *p > 0.0)?;
        let price = self.currency_units().quote_price(price)?;
        Some(BookLevel::new(price, size.map(Decimal::from)))
    }

    fn decimal(value: Option<f64>) -> Option<Decimal> {
        finite_decimal(value)
    }

    fn percent_to_fraction(value: Option<f64>) -> Option<Decimal> {
        Self::decimal(value).map(|v| v / Decimal::from(100))
    }

    fn as_of(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.regular_market_time
            .and_then(|timestamp| i64_to_datetime(timestamp).ok())
    }

    pub(crate) fn to_snapshot(&self) -> Result<Snapshot, YfError> {
        let exchange = self.exchange();
        let currencies = self.currency_units();
        let price = |value: Option<f64>| value.and_then(|value| currencies.quote_price(value));

        Ok(Snapshot {
            instrument: self.instrument(exchange)?,
            name: self.long_name.clone().or_else(|| self.short_name.clone()),
            market_state: self.market_state.as_deref().and_then(|s| s.parse().ok()),
            as_of: self.as_of(),
            last: price(self.regular_market_price),
            previous_close: price(self.regular_market_previous_close),
            open: price(self.regular_market_open),
            day_high: price(self.regular_market_day_high),
            day_low: price(self.regular_market_day_low),
            volume: self.regular_market_volume,
            provider: (),
        })
    }

    pub(crate) fn to_key_statistics(&self) -> KeyStatistics {
        let currencies = self.currency_units();
        let quote_price =
            |value: Option<f64>| value.and_then(|value| currencies.quote_price(value));
        let quote_money =
            |value: Option<f64>| value.and_then(|value| currencies.quote_money(value));
        let financial_price =
            |value: Option<f64>| value.and_then(|value| currencies.financial_price(value));

        KeyStatistics {
            as_of: self.as_of(),
            market_cap: quote_money(self.market_cap),
            shares_outstanding: self.shares_outstanding,
            eps_trailing_twelve_months: financial_price(self.eps_trailing_twelve_months),
            pe_trailing_twelve_months: Self::decimal(self.trailing_pe),
            dividend_per_share_forward: financial_price(self.dividend_rate),
            dividend_yield_trailing: Self::decimal(self.trailing_annual_dividend_yield),
            dividend_yield_forward: Self::percent_to_fraction(self.dividend_yield),
            ex_dividend_date: None,
            fifty_two_week_high: quote_price(self.fifty_two_week_high),
            fifty_two_week_low: quote_price(self.fifty_two_week_low),
            average_daily_volume_3m: self.average_daily_volume_3_month,
            beta: Self::decimal(self.beta),
        }
    }

    pub(crate) fn calendar_fallback(&self) -> Option<Calendar> {
        self.dividend_date
            .and_then(|ts| i64_to_datetime(ts).ok())
            .map(|ts| Calendar {
                earnings_dates: Vec::new(),
                ex_dividend_date: None,
                dividend_payment_date: Some(ts),
            })
    }
}

struct QuoteCurrencyUnits {
    quote: Option<ResolvedCurrencyUnit>,
    quote_major: Option<ResolvedCurrencyUnit>,
    financial: Option<ResolvedCurrencyUnit>,
}

impl QuoteCurrencyUnits {
    fn from_quote_node(node: &V7QuoteNode) -> Self {
        let quote = node
            .currency
            .as_deref()
            .and_then(ResolvedCurrencyUnit::from_code);
        let quote_major = quote.as_ref().map(ResolvedCurrencyUnit::major_unit);
        let financial = node
            .financial_currency
            .as_deref()
            .and_then(nonempty)
            .map_or_else(
                || quote_major.clone(),
                ResolvedCurrencyUnit::major_from_code,
            );

        Self {
            quote,
            quote_major,
            financial,
        }
    }

    fn quote_price(&self, value: f64) -> Option<paft::money::Price> {
        self.quote
            .as_ref()
            .and_then(|currency| currency.price_from_f64(value))
    }

    fn quote_money(&self, value: f64) -> Option<paft::money::Money> {
        self.quote_major
            .as_ref()
            .and_then(|currency| currency.money_from_f64(value))
    }

    fn financial_price(&self, value: f64) -> Option<paft::money::Price> {
        self.financial
            .as_ref()
            .and_then(|currency| currency.price_from_f64(value))
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
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
    // Inner function to attempt the fetch, allowing for an auth retry.
    async fn attempt_fetch(
        client: &YfClient,
        symbols: &[&str],
        crumb: Option<&str>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<(String, Url, Option<u16>), YfError> {
        let mut url = client.base_quote_v7().clone();
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("symbols", &symbols.join(","));
            if let Some(c) = crumb {
                qp.append_pair("crumb", c);
            }
        }

        if cache_mode == CacheMode::Use
            && let Some(body) = client.cache_get(&url).await
        {
            return Ok((body, url, None));
        }

        let resp = client
            .send_with_retry(
                client
                    .http()
                    .get(url.clone())
                    .header("accept", "application/json"),
                retry_override,
            )
            .await?;

        if resp.status().is_success() {
            let body =
                net::get_success_text(resp, &url, "quote_v7", &symbols.join("-"), "json").await?;
            if cache_mode != CacheMode::Bypass {
                client.cache_put(&url, &body, None).await;
            }
            Ok((body, url, None))
        } else {
            Ok((String::new(), url, Some(resp.status().as_u16())))
        }
    }

    // First attempt, without a crumb.
    let (body, url, maybe_status) =
        attempt_fetch(client, symbols, None, cache_mode, retry_override).await?;

    let body_to_parse = if let Some(status_code) = maybe_status {
        // If unauthorized, get a crumb and retry.
        if status_code == 401 || status_code == 403 {
            client.ensure_credentials().await?;
            let crumb = client.crumb().await.ok_or_else(|| {
                YfError::Auth("Crumb is not set after ensuring credentials".into())
            })?;

            // Second attempt, with a crumb.
            let (body, url, maybe_status) =
                attempt_fetch(client, symbols, Some(&crumb), cache_mode, retry_override).await?;

            if let Some(status_code) = maybe_status {
                if status_code == 401 || status_code == 403 {
                    client.clear_crumb().await;
                    client.ensure_credentials().await?;
                    let crumb = client.crumb().await.ok_or_else(|| {
                        YfError::Auth("Crumb is not set after refreshing credentials".into())
                    })?;
                    let (body, url, maybe_status) =
                        attempt_fetch(client, symbols, Some(&crumb), cache_mode, retry_override)
                            .await?;

                    if let Some(status_code) = maybe_status {
                        return Err(net::status_error_code(status_code, &url));
                    }
                    body
                } else {
                    return Err(net::status_error_code(status_code, &url));
                }
            } else {
                body
            }
        } else {
            return Err(net::status_error_code(status_code, &url));
        }
    } else {
        body
    };

    let env: V7Envelope = serde_json::from_str(&body_to_parse)?;
    let nodes = env
        .quote_response
        .ok_or_else(|| YfError::MissingData("v7 quoteResponse missing".into()))?
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
    let exch = crate::core::conversions::string_to_exchange(
        node.full_exchange_name
            .clone()
            .or_else(|| node.exchange.clone())
            .or_else(|| node.market.clone())
            .or_else(|| node.market_cap_figure_exchange.clone()),
    );
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
        let exchange = n.exchange();
        let instrument = n.instrument(exchange)?;
        let currencies = n.currency_units();

        Ok(Self {
            instrument,
            name: n.long_name.clone().or_else(|| n.short_name.clone()),
            price: n
                .regular_market_price
                .and_then(|price| currencies.quote_price(price)),
            bid: n.positive_book_level(n.bid, n.bid_size),
            ask: n.positive_book_level(n.ask, n.ask_size),
            previous_close: n
                .regular_market_previous_close
                .and_then(|price| currencies.quote_price(price)),
            day_volume: n.regular_market_volume,
            market_state: n.market_state.as_deref().and_then(|s| s.parse().ok()),
            as_of: n.as_of(),
            provider: (),
        })
    }
}
