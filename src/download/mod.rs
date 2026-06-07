use futures::{StreamExt, stream};

use crate::{
    core::client::normalize_symbols,
    core::conversions::f64_from_price_amount,
    core::{
        CallOptions, Candle, HistoryResponse, Interval, ProjectionContext, ProjectionIssue, Range,
        YfClient, YfError, YfResponse,
    },
    history::HistoryBuilder,
};
use paft::market::responses::{
    download::{DownloadEntry, DownloadResponse},
    history::{OhlcPriceBasis, PriceBasis},
};
use paft::money::PriceAmount;
use rust_decimal::prelude::FromPrimitive;
type DateRange = (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>);
type MaybeDateRange = Option<DateRange>;
type DownloadFetchSuccess = (usize, String, YfResponse<HistoryResponse>);
type DownloadFetchFailure = (usize, String, YfError);
type DownloadFetchResult = Result<DownloadFetchSuccess, DownloadFetchFailure>;

/// Maximum number of per-symbol history requests a [`DownloadBuilder`] runs at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DownloadConcurrency(usize);

impl DownloadConcurrency {
    /// Default download concurrency.
    pub const DEFAULT: Self = Self(8);

    /// Builds a validated download concurrency limit.
    ///
    /// # Errors
    ///
    /// Returns `YfError::InvalidParams` if `value` is zero.
    pub fn new(value: usize) -> Result<Self, YfError> {
        if value == 0 {
            return Err(YfError::InvalidParams(
                "download concurrency must be at least 1".into(),
            ));
        }
        Ok(Self(value))
    }

    const fn get(self) -> usize {
        self.0
    }
}

impl Default for DownloadConcurrency {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A builder for downloading historical data for multiple symbols concurrently.
///
/// This provides a convenient way to fetch data for a list of tickers with the same
/// parameters in parallel, similar to `yfinance.download` in Python.
///
/// Many of the configuration methods mirror those on [`HistoryBuilder`].
#[allow(clippy::struct_excessive_bools)]
pub struct DownloadBuilder {
    client: YfClient,
    symbols: Vec<String>,

    // date / time controls
    range: Option<Range>,
    period: Option<(i64, i64)>,
    interval: Interval,

    // behavior flags
    auto_adjust: bool,
    back_adjust: bool,
    include_prepost: bool,
    include_actions: bool,
    rounding: bool,

    options: CallOptions,
    concurrency: DownloadConcurrency,
}

impl DownloadBuilder {
    fn precompute_period_dt(&self) -> Result<MaybeDateRange, YfError> {
        if let Some((p1, p2)) = self.period {
            use chrono::{TimeZone, Utc};
            let start = Utc
                .timestamp_opt(p1, 0)
                .single()
                .ok_or_else(|| YfError::InvalidParams("invalid period1".into()))?;
            let end = Utc
                .timestamp_opt(p2, 0)
                .single()
                .ok_or_else(|| YfError::InvalidParams("invalid period2".into()))?;
            if start >= end {
                return Err(YfError::InvalidDates);
            }
            Ok(Some((start, end)))
        } else {
            Ok(None)
        }
    }

    fn build_history_for_symbol(
        &self,
        sym: &str,
        period_dt: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
        need_adjust_in_fetch: bool,
    ) -> HistoryBuilder {
        let mut hb: HistoryBuilder = HistoryBuilder::new(&self.client, sym.to_string())
            .interval(self.interval)
            .auto_adjust(need_adjust_in_fetch)
            .prepost(self.include_prepost)
            .actions(self.include_actions)
            .data_quality(self.options.data_quality())
            .cache_mode(self.options.cache_mode())
            .retry_policy(self.options.retry_override().cloned());

        if let Some((start, end)) = period_dt {
            hb = hb.between(start, end);
        } else if let Some(r) = self.range {
            hb = hb.range(r);
        } else {
            hb = hb.range(Range::M6);
        }
        hb
    }

    fn apply_back_adjust(&self, rows: &mut [Candle]) {
        if !self.back_adjust {
            return;
        }
        for c in rows.iter_mut() {
            if let Some(rc) = c.close_unadj.as_ref()
                && f64_from_price_amount(rc).is_some_and(f64::is_finite)
            {
                c.ohlc.close = rc.clone();
            }
        }
    }

    const fn back_adjust_price_basis(&self, fetched_basis: OhlcPriceBasis) -> OhlcPriceBasis {
        if !self.back_adjust {
            return fetched_basis;
        }

        let (open, high, low, _) = fetched_basis.fields();
        OhlcPriceBasis::per_field(*open, *high, *low, PriceBasis::raw())
    }

    fn validate_adjustment_flags(&self) -> Result<(), YfError> {
        if self.auto_adjust && self.back_adjust {
            return Err(YfError::InvalidParams(
                "auto_adjust and back_adjust are mutually exclusive; use auto_adjust(false) with back_adjust(true)"
                    .into(),
            ));
        }
        Ok(())
    }

    fn apply_rounding_if_enabled(&self, rows: &mut [Candle]) {
        if !self.rounding {
            return;
        }
        for c in rows {
            if let Some(open) = rounded_price(&c.ohlc.open) {
                c.ohlc.open = open;
            }
            if let Some(high) = rounded_price(&c.ohlc.high) {
                c.ohlc.high = high;
            }
            if let Some(low) = rounded_price(&c.ohlc.low) {
                c.ohlc.low = low;
            }
            if let Some(close) = rounded_price(&c.ohlc.close) {
                c.ohlc.close = close;
            }
        }
    }

    fn process_joined_results(
        &self,
        joined: Vec<(String, YfResponse<HistoryResponse>)>,
        ctx: &mut ProjectionContext,
    ) -> Result<DownloadResponse, YfError> {
        let mut entries: Vec<DownloadEntry> = Vec::with_capacity(joined.len());
        for (sym, response) in joined {
            ctx.extend(response.diagnostics.with_key_prefix(&sym));
            let mut resp = response.data;
            // apply transforms to candles
            self.apply_back_adjust(&mut resp.candles);
            resp.price_basis = self.back_adjust_price_basis(resp.price_basis);
            self.apply_rounding_if_enabled(&mut resp.candles);

            let Some(instrument) = self.client.cached_instrument(&sym) else {
                ctx.dropped_item(
                    "download_entry",
                    Some(sym.as_str()),
                    ProjectionIssue::MissingRequiredField {
                        field: "chart.meta.instrumentType",
                    },
                )?;
                continue;
            };

            entries.push(DownloadEntry {
                instrument,
                history: resp,
                provider: (),
            });
        }
        Ok(DownloadResponse {
            entries,
            provider: (),
        })
    }

    /// Creates a new `DownloadBuilder`.
    #[must_use]
    pub fn new(client: &YfClient) -> Self {
        Self {
            client: client.clone(),
            symbols: Vec::new(),
            range: Some(Range::M6),
            period: None,
            interval: Interval::D1,
            auto_adjust: true,
            back_adjust: false,
            include_prepost: false,
            include_actions: true,
            rounding: false,
            options: CallOptions::default(),
            concurrency: DownloadConcurrency::DEFAULT,
        }
    }

    crate::core::impl_call_option_setters!();

    /// Sets the maximum number of per-symbol history requests to run at once. (Default: `8`)
    #[must_use]
    pub const fn concurrency(mut self, concurrency: DownloadConcurrency) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Replaces the current list of symbols with a new list.
    #[must_use]
    pub fn symbols<I, S>(mut self, syms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.symbols = syms.into_iter().map(std::convert::Into::into).collect();
        self
    }

    /// Adds a single symbol to the list of symbols to download.
    #[must_use]
    pub fn add_symbol(mut self, sym: impl Into<String>) -> Self {
        self.symbols.push(sym.into());
        self
    }

    /// Sets a relative time range for the request (e.g., `1y`, `6mo`).
    #[must_use]
    pub const fn range(mut self, range: Range) -> Self {
        self.period = None;
        self.range = Some(range);
        self
    }

    /// Sets an absolute time period for the request using start and end timestamps.
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
    ///
    /// This is mutually exclusive with [`Self::back_adjust`]. If both are enabled, execution
    /// returns [`YfError::InvalidParams`].
    #[must_use]
    pub const fn auto_adjust(mut self, yes: bool) -> Self {
        self.auto_adjust = yes;
        self
    }

    /// Sets whether to back-adjust prices.
    ///
    /// Back-adjustment adjusts the Open, High, and Low prices, but keeps the Close price as the
    /// raw, unadjusted close. Call `.auto_adjust(false).back_adjust(true)` to request this mode.
    ///
    /// This is mutually exclusive with [`Self::auto_adjust`]. If both are enabled, execution
    /// returns [`YfError::InvalidParams`].
    #[must_use]
    pub const fn back_adjust(mut self, yes: bool) -> Self {
        self.back_adjust = yes;
        self
    }

    /// Sets whether to include pre-market and post-market data for intraday intervals. (Default: `false`)
    #[must_use]
    pub const fn prepost(mut self, yes: bool) -> Self {
        self.include_prepost = yes;
        self
    }

    /// Sets whether to include corporate actions (dividends and splits) in the result. (Default: `true`)
    #[must_use]
    pub const fn actions(mut self, yes: bool) -> Self {
        self.include_actions = yes;
        self
    }

    /// Sets whether to round prices to 2 decimal places. (Default: `false`)
    #[must_use]
    pub const fn rounding(mut self, yes: bool) -> Self {
        self.rounding = yes;
        self
    }

    /// Executes the download by fetching data for all specified symbols concurrently.
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid or strict data-quality mode rejects a
    /// diagnostic. In best-effort mode, individual symbol fetch failures are returned as
    /// diagnostics while successful symbols are still included in the response.
    pub async fn run(&self) -> Result<DownloadResponse, YfError> {
        Ok(self.run_with_diagnostics().await?.into_data())
    }

    /// Executes the download and returns projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if parameters are invalid or strict data-quality mode rejects a
    /// diagnostic. In best-effort mode, individual symbol fetch failures are returned as
    /// diagnostics while successful symbols are still included in the response.
    pub async fn run_with_diagnostics(&self) -> Result<YfResponse<DownloadResponse>, YfError> {
        self.validate_adjustment_flags()?;

        if self.symbols.is_empty() {
            return Err(YfError::InvalidParams("no symbols specified".into()));
        }
        let symbols = normalize_symbols(self.symbols.iter().map(String::as_str))?;
        let mut ctx = ProjectionContext::new("download", self.options.data_quality());

        let need_adjust_in_fetch = self.auto_adjust || self.back_adjust;
        let period_dt = self.precompute_period_dt()?;

        let results: Vec<DownloadFetchResult> = stream::iter(symbols.into_iter().enumerate())
            .map(|(index, sym)| {
                let hb = self.build_history_for_symbol(&sym, period_dt, need_adjust_in_fetch);

                async move {
                    match hb.fetch_full_with_diagnostics().await {
                        Ok(full) => Ok((index, sym, full)),
                        Err(err) => Err((index, sym, err)),
                    }
                }
            })
            .buffer_unordered(self.concurrency.get())
            .collect()
            .await;

        let mut joined = Vec::with_capacity(results.len());
        let mut failed = Vec::new();
        for result in results {
            match result {
                Ok(success) => joined.push(success),
                Err(failure) => failed.push(failure),
            }
        }

        failed.sort_unstable_by_key(|(index, _, _)| *index);
        for (_, sym, err) in failed {
            ctx.dropped_item(
                "download_entry",
                Some(sym.as_str()),
                ProjectionIssue::ProviderError {
                    message: format!("history fetch failed: {err}"),
                },
            )?;
        }

        joined.sort_unstable_by_key(|(index, _, _)| *index);
        let joined: Vec<(String, YfResponse<HistoryResponse>)> = joined
            .into_iter()
            .map(|(_, sym, full)| (sym, full))
            .collect();

        let response = self.process_joined_results(joined, &mut ctx)?;
        Ok(ctx.finish(response))
    }
}

/* ---------------- internal helpers ---------------- */

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn rounded_price(price: &PriceAmount) -> Option<PriceAmount> {
    let value = f64_from_price_amount(price)?;
    let decimal = rust_decimal::Decimal::from_f64(round2(value))?;
    Some(PriceAmount::new(decimal))
}
