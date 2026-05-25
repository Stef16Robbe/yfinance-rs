// src/core/quotes.rs
use serde::Deserialize;
use url::Url;

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheMode, RetryConfig},
        conversions::{
            f64_to_money_with_currency_str, f64_to_price_with_currency_str, i64_to_datetime,
        },
        net,
    },
};
use paft::Decimal;
use paft::aggregates::Snapshot;
use paft::domain::{AssetKind, Instrument};
use paft::fundamentals::statements::Calendar;
use paft::fundamentals::statistics::KeyStatistics;
use paft::market::orderbook::BookLevel;
use paft::market::quote::Quote;
use paft::money::{Currency, IsoCurrency};
use std::str::FromStr;

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
    fn exchange(&self) -> Option<paft::domain::Exchange> {
        crate::core::conversions::string_to_exchange(
            self.full_exchange_name
                .clone()
                .or_else(|| self.exchange.clone())
                .or_else(|| self.market.clone())
                .or_else(|| self.market_cap_figure_exchange.clone()),
        )
    }

    fn instrument(&self, exchange: Option<paft::domain::Exchange>) -> Instrument {
        let sym = self.symbol.as_deref().unwrap_or_default();
        let kind = self
            .quote_type
            .as_deref()
            .and_then(|s| s.parse::<AssetKind>().ok())
            .unwrap_or(AssetKind::Equity);

        exchange
            .map_or_else(
                || Instrument::from_symbol(sym, kind),
                |ex| Instrument::from_symbol_and_exchange(sym, ex, kind),
            )
            .expect("v7 quote node had invalid/missing symbol")
    }

    fn currency(&self) -> Currency {
        self.currency
            .as_deref()
            .and_then(|s| Currency::from_str(s).ok())
            .unwrap_or(Currency::Iso(IsoCurrency::USD))
    }

    fn positive_book_level(&self, price: Option<f64>, size: Option<u64>) -> Option<BookLevel> {
        let price = price.filter(|p| p.is_finite() && *p > 0.0)?;
        Some(BookLevel::new(
            f64_to_price_with_currency_str(price, self.currency.as_deref()),
            size.map(Decimal::from),
        ))
    }

    fn decimal(value: Option<f64>) -> Option<Decimal> {
        value
            .filter(|v| v.is_finite())
            .and_then(|v| Decimal::try_from(v).ok())
    }

    fn percent_to_fraction(value: Option<f64>) -> Option<Decimal> {
        Self::decimal(value).map(|v| v / Decimal::from(100))
    }

    fn as_of(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.regular_market_time.map(i64_to_datetime)
    }

    pub(crate) fn to_snapshot(&self) -> Snapshot {
        let exchange = self.exchange();
        let price = |value: Option<f64>| {
            value.map(|value| f64_to_price_with_currency_str(value, self.currency.as_deref()))
        };

        Snapshot {
            instrument: self.instrument(exchange.clone()),
            name: self.long_name.clone().or_else(|| self.short_name.clone()),
            exchange,
            currency: Some(self.currency()),
            market_state: self.market_state.as_deref().and_then(|s| s.parse().ok()),
            as_of: self.as_of().or_else(|| Some(chrono::Utc::now())),
            last: price(self.regular_market_price),
            previous_close: price(self.regular_market_previous_close),
            open: price(self.regular_market_open),
            day_high: price(self.regular_market_day_high),
            day_low: price(self.regular_market_day_low),
            volume: self.regular_market_volume,
            provider: (),
        }
    }

    pub(crate) fn to_key_statistics(&self) -> KeyStatistics {
        let money = |value: Option<f64>| {
            value.map(|value| f64_to_money_with_currency_str(value, self.currency.as_deref()))
        };
        let price = |value: Option<f64>| {
            value.map(|value| f64_to_price_with_currency_str(value, self.currency.as_deref()))
        };

        KeyStatistics {
            as_of: self.as_of().or_else(|| Some(chrono::Utc::now())),
            market_cap: money(self.market_cap),
            shares_outstanding: self.shares_outstanding,
            eps_trailing_twelve_months: price(self.eps_trailing_twelve_months),
            pe_trailing_twelve_months: Self::decimal(self.trailing_pe),
            dividend_per_share_forward: price(self.dividend_rate),
            dividend_yield_trailing: Self::decimal(self.trailing_annual_dividend_yield),
            dividend_yield_forward: Self::percent_to_fraction(self.dividend_yield),
            ex_dividend_date: None,
            fifty_two_week_high: price(self.fifty_two_week_high),
            fifty_two_week_low: price(self.fifty_two_week_low),
            average_daily_volume_3m: self.average_daily_volume_3_month,
            beta: Self::decimal(self.beta),
        }
    }

    pub(crate) fn calendar_fallback(&self) -> Option<Calendar> {
        self.dividend_date.map(|ts| Calendar {
            earnings_dates: Vec::new(),
            ex_dividend_date: None,
            dividend_payment_date: Some(i64_to_datetime(ts)),
        })
    }
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

        let status = resp.status();
        let body = net::get_text(resp, "quote_v7", &symbols.join("-"), "json").await?;

        if status.is_success() {
            if cache_mode != CacheMode::Bypass {
                client.cache_put(&url, &body, None).await;
            }
            Ok((body, url, None))
        } else {
            Ok((body, url, Some(status.as_u16())))
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
                let url_s = url.to_string();
                return Err(match status_code {
                    404 => YfError::NotFound { url: url_s },
                    429 => YfError::RateLimited { url: url_s },
                    500..=599 => YfError::ServerError {
                        status: status_code,
                        url: url_s,
                    },
                    _ => YfError::Status {
                        status: status_code,
                        url: url_s,
                    },
                });
            }
            body
        } else {
            let url_s = url.to_string();
            return Err(match status_code {
                404 => YfError::NotFound { url: url_s },
                429 => YfError::RateLimited { url: url_s },
                500..=599 => YfError::ServerError {
                    status: status_code,
                    url: url_s,
                },
                _ => YfError::Status {
                    status: status_code,
                    url: url_s,
                },
            });
        }
    } else {
        body
    };

    let env: V7Envelope = serde_json::from_str(&body_to_parse)?;
    let nodes = env
        .quote_response
        .and_then(|qr| qr.result)
        .unwrap_or_default();

    // Populate instrument cache best-effort from v7 quote nodes
    for n in &nodes {
        if let Some(sym) = n.symbol.as_deref() {
            let exch = crate::core::conversions::string_to_exchange(
                n.full_exchange_name
                    .clone()
                    .or_else(|| n.exchange.clone())
                    .or_else(|| n.market.clone())
                    .or_else(|| n.market_cap_figure_exchange.clone()),
            );
            let kind = n
                .quote_type
                .as_deref()
                .and_then(|s| s.parse::<AssetKind>().ok())
                .unwrap_or(AssetKind::Equity);

            let inst = exch.map_or_else(
                || Instrument::from_symbol(sym, kind),
                |ex| Instrument::from_symbol_and_exchange(sym, ex, kind),
            );
            if let Ok(inst) = inst {
                client.store_instrument(sym.to_string(), inst).await;
            }
        }
    }

    Ok(nodes)
}

impl From<V7QuoteNode> for Quote {
    fn from(n: V7QuoteNode) -> Self {
        let exchange = n.exchange();
        let instrument = n.instrument(exchange.clone());

        Self {
            instrument,
            name: n.long_name.clone().or_else(|| n.short_name.clone()),
            price: n
                .regular_market_price
                .map(|price| f64_to_price_with_currency_str(price, n.currency.as_deref())),
            bid: n.positive_book_level(n.bid, n.bid_size),
            ask: n.positive_book_level(n.ask, n.ask_size),
            previous_close: n
                .regular_market_previous_close
                .map(|price| f64_to_price_with_currency_str(price, n.currency.as_deref())),
            day_volume: n.regular_market_volume,
            exchange,
            market_state: n.market_state.and_then(|s| s.parse().ok()),
            provider: (),
        }
    }
}
