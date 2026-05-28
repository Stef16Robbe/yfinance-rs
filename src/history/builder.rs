mod actions;
mod adjust;
mod assemble;
mod fetch;

use crate::core::conversions::{string_to_asset_kind, string_to_exchange};
use crate::core::{YfClient, YfError};
use crate::core::{
    client::{CacheMode, RetryConfig},
    currency_resolver::{
        CorporateActionCurrencyEvidence, CurrencyHints, ResolvedCurrencyUnit,
        TradingCurrencyEvidence,
    },
};
use crate::history::wire::MetaNode;
use chrono_tz::Tz;
use paft::domain::Instrument;
use paft::market::action::Action;
use paft::market::requests::history::{Interval, Range};
use paft::market::responses::history::{Candle, HistoryMeta, HistoryResponse};

use actions::extract_actions;
use adjust::cumulative_split_after;
use assemble::assemble_candles;
use fetch::fetch_chart;

/// A builder for fetching historical price data for a single symbol.
///
/// This builder provides fine-grained control over the parameters for a historical
/// data request, including the time range, interval, and data adjustments.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct HistoryBuilder {
    #[doc(hidden)]
    pub(crate) client: YfClient,
    #[doc(hidden)]
    pub(crate) symbol: String,
    #[doc(hidden)]
    pub(crate) range: Option<Range>,
    #[doc(hidden)]
    pub(crate) period: Option<(i64, i64)>,
    #[doc(hidden)]
    pub(crate) interval: Interval,
    #[doc(hidden)]
    pub(crate) auto_adjust: bool,
    #[doc(hidden)]
    pub(crate) include_prepost: bool,
    #[doc(hidden)]
    pub(crate) include_actions: bool,
    pub(crate) cache_mode: CacheMode,
    #[doc(hidden)]
    pub(crate) retry_override: Option<RetryConfig>,
}

impl HistoryBuilder {
    /// Creates a new `HistoryBuilder` for a given symbol.
    pub fn new(client: &YfClient, symbol: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            symbol: symbol.into(),
            range: Some(Range::M6),
            period: None,
            interval: Interval::D1,
            auto_adjust: true,
            include_prepost: false,
            include_actions: true,
            cache_mode: CacheMode::Default,
            retry_override: None,
        }
    }

    /// Sets the cache mode for this specific API call.
    #[must_use]
    pub const fn cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    /// Overrides the default retry policy for this specific API call.
    #[must_use]
    pub fn retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.retry_override = cfg;
        self
    }

    /// Sets a relative time range for the request (e.g., `1y`, `6mo`).
    ///
    /// This will override any previously set period using `between()`.
    #[must_use]
    pub const fn range(mut self, range: Range) -> Self {
        self.period = None;
        self.range = Some(range);
        self
    }

    /// Sets an absolute time period for the request using start and end timestamps.
    ///
    /// This will override any previously set range using `range()`.
    #[must_use]
    pub const fn between(
        mut self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        self.range = None;
        self.period = Some((start.timestamp(), end.timestamp()));
        self
    }

    /// Sets the time interval for each data point (candle).
    #[must_use]
    pub const fn interval(mut self, interval: Interval) -> Self {
        self.interval = interval;
        self
    }

    /// Sets whether to automatically adjust prices for splits and dividends. (Default: `true`)
    #[must_use]
    pub const fn auto_adjust(mut self, yes: bool) -> Self {
        self.auto_adjust = yes;
        self
    }

    /// Sets whether to include pre-market and post-market data for intraday intervals. (Default: `false`)
    #[must_use]
    pub const fn prepost(mut self, yes: bool) -> Self {
        self.include_prepost = yes;
        self
    }

    /// Sets whether to include corporate actions (dividends and splits) in the response. (Default: `true`)
    #[must_use]
    pub const fn actions(mut self, yes: bool) -> Self {
        self.include_actions = yes;
        self
    }

    /// Executes the request and returns only the price candles.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn fetch(self) -> Result<Vec<Candle>, YfError> {
        let resp = self.fetch_full().await?;
        Ok(resp.candles)
    }

    /// Executes the request and returns the full response, including candles, actions, and metadata.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails, the API returns an error,
    /// or the response cannot be parsed.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(
            skip(self),
            err,
            fields(
                symbol = %self.symbol,
                interval = %format!("{:?}", self.interval),
                range = %self
                    .range
                    .as_ref()
                    .map_or_else(|| "period".into(), |r| format!("{r:?}"))
            )
        )
    )]
    pub async fn fetch_full(self) -> Result<HistoryResponse, YfError> {
        // 1) Fetch and parse the /chart payload into owned blocks
        let fetched = fetch_chart(
            &self.client,
            &self.symbol,
            self.range,
            self.period,
            self.interval,
            self.include_actions,
            self.include_prepost,
            self.cache_mode,
            self.retry_override.as_ref(),
        )
        .await?;

        cache_history_instrument(&self.client, &self.symbol, fetched.meta.as_ref()).await?;
        if let Some(meta) = fetched.meta.as_ref() {
            self.client
                .store_currency_hints(
                    &self.symbol,
                    CurrencyHints::from_chart(
                        meta.currency.as_deref(),
                        meta.exchange_name.as_deref(),
                        meta.full_exchange_name.as_deref(),
                        meta.instrument_type.as_deref(),
                    ),
                )
                .await;
        }

        // 2) Corporate actions & split ratios
        let chart_currency = fetched.meta.as_ref().and_then(|m| m.currency.as_deref());
        let currency = if has_complete_ohlc_row(&fetched.quote, fetched.ts.len()) {
            Some(
                self.client
                    .resolve_trading_currency_unit(
                        &self.symbol,
                        None,
                        TradingCurrencyEvidence::ChartMeta(chart_currency),
                        self.cache_mode,
                        self.retry_override.as_ref(),
                    )
                    .await?,
            )
        } else {
            None
        };
        let action_currency = action_default_currency(
            &self.client,
            &self.symbol,
            fetched.events.as_ref(),
            chart_currency,
            currency.as_ref(),
            self.cache_mode,
            self.retry_override.as_ref(),
        )
        .await;

        let (mut actions_out, split_events) =
            extract_actions(fetched.events.as_ref(), action_currency.as_ref());

        // 3) Cumulative split factors after each bar
        let cum_split_after = cumulative_split_after(&fetched.ts, &split_events);

        // 4) Assemble candles (+ raw close) with/without adjustments
        let candles = currency.as_ref().map_or_else(Vec::new, |currency| {
            assemble_candles(
                &fetched.ts,
                &fetched.quote,
                &fetched.adjclose,
                self.auto_adjust,
                &cum_split_after,
                currency,
            )
        });

        // ensure actions sorted (extract_actions already sorts, keep consistent)
        actions_out.sort_by_key(|a| match a {
            Action::Dividend { ts, .. }
            | Action::Split { ts, .. }
            | Action::CapitalGain { ts, .. } => ts.timestamp(),
            _ => i64::MAX,
        });

        // 5) Map metadata
        let meta_out = map_meta(fetched.meta.as_ref());

        Ok(HistoryResponse {
            candles,
            actions: actions_out,
            adjusted: self.auto_adjust,
            meta: meta_out,
            provider: (),
        })
    }
}

/* --- tiny private helper --- */

fn has_complete_ohlc_row(quote: &crate::history::wire::QuoteBlock, len: usize) -> bool {
    (0..len).any(|idx| {
        quote.open.get(idx).and_then(|value| *value).is_some()
            && quote.high.get(idx).and_then(|value| *value).is_some()
            && quote.low.get(idx).and_then(|value| *value).is_some()
            && quote.close.get(idx).and_then(|value| *value).is_some()
    })
}

async fn action_default_currency(
    client: &YfClient,
    symbol: &str,
    events: Option<&crate::history::wire::Events>,
    chart_currency: Option<&str>,
    trading_currency: Option<&ResolvedCurrencyUnit>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Option<ResolvedCurrencyUnit> {
    if !events_need_default_currency(events) {
        return trading_currency.cloned();
    }

    if let Some(currency) = trading_currency {
        return Some(currency.clone());
    }

    client
        .resolve_corporate_action_currency_unit(
            symbol,
            None,
            CorporateActionCurrencyEvidence::ChartMeta(chart_currency),
            cache_mode,
            retry_override,
        )
        .await
        .ok()
}

fn events_need_default_currency(events: Option<&crate::history::wire::Events>) -> bool {
    let Some(events) = events else {
        return false;
    };

    events.dividends.as_ref().is_some_and(|events| {
        events
            .values()
            .any(|event| event.amount.is_some() && is_missing_currency(event.currency.as_deref()))
    }) || events.capital_gains.as_ref().is_some_and(|events| {
        events
            .values()
            .any(|event| event.amount.is_some() && is_missing_currency(event.currency.as_deref()))
    })
}

fn is_missing_currency(currency: Option<&str>) -> bool {
    currency.is_none_or(|currency| currency.trim().is_empty())
}

fn map_meta(m: Option<&MetaNode>) -> Option<HistoryMeta> {
    m.as_ref().map(|mm| HistoryMeta {
        timezone: mm
            .timezone
            .as_ref()
            .and_then(|tz_str| tz_str.parse::<Tz>().ok()),
        utc_offset_seconds: mm.gmtoffset,
    })
}

async fn cache_history_instrument(
    client: &YfClient,
    requested_symbol: &str,
    meta: Option<&MetaNode>,
) -> Result<(), YfError> {
    let Some(meta) = meta else {
        return Ok(());
    };
    let Some(kind) = meta
        .instrument_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(string_to_asset_kind)
        .transpose()?
    else {
        return Ok(());
    };

    let exchange = meta
        .full_exchange_name
        .as_deref()
        .or(meta.exchange_name.as_deref())
        .and_then(|exchange| string_to_exchange(Some(exchange.to_string())));

    let instrument = match exchange {
        Some(exchange) => {
            Instrument::from_symbol_and_exchange(requested_symbol, exchange, kind.clone())
        }
        None => Instrument::from_symbol(requested_symbol, kind),
    };

    let Ok(instrument) = instrument else {
        return Ok(());
    };

    client
        .store_instrument(requested_symbol.to_string(), instrument.clone())
        .await;
    if let Some(provider_symbol) = meta
        .symbol
        .as_deref()
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty() && *symbol != requested_symbol)
    {
        client
            .store_instrument(provider_symbol.to_string(), instrument)
            .await;
    }

    Ok(())
}
