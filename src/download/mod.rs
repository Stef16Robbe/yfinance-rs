use futures::{StreamExt, TryStreamExt, stream};

use crate::{
    core::client::{CacheMode, RetryConfig, normalize_symbols},
    core::conversions::f64_from_currency_value,
    core::{
        Candle, DataQuality, HistoryResponse, Interval, ProjectionContext, ProjectionIssue, Range,
        YfClient, YfError, YfResponse,
    },
    history::HistoryBuilder,
};
use paft::market::responses::download::{DownloadEntry, DownloadResponse};
use paft::money::Price;
use rust_decimal::prelude::FromPrimitive;
type DateRange = (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>);
type MaybeDateRange = Option<DateRange>;

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
    repair: bool,

    cache_mode: CacheMode,
    retry_override: Option<RetryConfig>,
    concurrency: DownloadConcurrency,
    data_quality: DataQuality,
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
            .data_quality(self.data_quality)
            .cache_mode(self.cache_mode)
            .retry_policy(self.retry_override.clone());

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
                && f64_from_currency_value(rc).is_some_and(f64::is_finite)
            {
                c.close = rc.clone();
            }
        }
    }

    fn apply_rounding_if_enabled(&self, rows: &mut [Candle]) {
        if !self.rounding {
            return;
        }
        for c in rows {
            if let Some(open) = rounded_price(&c.open) {
                c.open = open;
            }
            if let Some(high) = rounded_price(&c.high) {
                c.high = high;
            }
            if let Some(low) = rounded_price(&c.low) {
                c.low = low;
            }
            if let Some(close) = rounded_price(&c.close) {
                c.close = close;
            }
        }
    }

    fn maybe_repair(
        &self,
        rows: &mut [Candle],
        symbol: &str,
        ctx: &mut ProjectionContext,
    ) -> Result<(), YfError> {
        if !self.repair {
            return Ok(());
        }
        for key in repair_scale_outliers(rows) {
            ctx.repaired_data(
                "candle",
                Some(format!("{symbol}:{key}")),
                "scaled suspicious OHLC outlier",
            )?;
        }
        Ok(())
    }

    async fn process_joined_results(
        &self,
        joined: Vec<(String, YfResponse<HistoryResponse>)>,
        _need_adjust_in_fetch: bool,
        ctx: &mut ProjectionContext,
    ) -> Result<DownloadResponse, YfError> {
        let mut entries: Vec<DownloadEntry> = Vec::with_capacity(joined.len());
        for (sym, response) in joined {
            ctx.extend(response.diagnostics.with_key_prefix(&sym));
            let mut resp = response.data;
            // apply transforms to candles
            self.apply_back_adjust(&mut resp.candles);
            self.maybe_repair(&mut resp.candles, &sym, ctx)?;
            self.apply_rounding_if_enabled(&mut resp.candles);

            let Some(instrument) = self.client.cached_instrument(&sym).await else {
                ctx.dropped_item(
                    "download_entry",
                    Some(sym),
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
            repair: false,
            cache_mode: CacheMode::Default,
            retry_override: None,
            concurrency: DownloadConcurrency::DEFAULT,
            data_quality: DataQuality::BestEffort,
        }
    }

    /// Sets the cache mode for all API calls made by this builder.
    #[must_use]
    pub const fn cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    /// Overrides the default retry policy for all API calls made by this builder.
    #[must_use]
    pub fn retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.retry_override = cfg;
        self
    }

    /// Sets how provider projection issues are handled.
    #[must_use]
    pub const fn data_quality(mut self, policy: DataQuality) -> Self {
        self.data_quality = policy;
        self
    }

    /// Fails when Yahoo data cannot be projected losslessly.
    #[must_use]
    pub const fn strict(self) -> Self {
        self.data_quality(DataQuality::Strict)
    }

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
    #[must_use]
    pub const fn auto_adjust(mut self, yes: bool) -> Self {
        self.auto_adjust = yes;
        self
    }

    /// Sets whether to back-adjust prices.
    ///
    /// Back-adjustment adjusts the Open, High, and Low prices, but keeps the Close price as the
    /// raw, unadjusted close. This forces an internal adjustment even if `auto_adjust` is false.
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

    /// Sets whether to attempt to repair obvious price outliers (e.g., 100x errors). (Default: `false`)
    #[must_use]
    pub const fn repair(mut self, yes: bool) -> Self {
        self.repair = yes;
        self
    }

    /// Executes the download by fetching data for all specified symbols concurrently.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the underlying history requests fail.
    pub async fn run(&self) -> Result<DownloadResponse, YfError> {
        Ok(self.run_with_diagnostics().await?.into_data())
    }

    /// Executes the download and returns projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if any history request fails or strict data-quality mode rejects a projection issue.
    pub async fn run_with_diagnostics(&self) -> Result<YfResponse<DownloadResponse>, YfError> {
        if self.symbols.is_empty() {
            return Err(YfError::InvalidParams("no symbols specified".into()));
        }
        let symbols = normalize_symbols(self.symbols.iter().map(String::as_str))?;
        let mut ctx = ProjectionContext::new("download", self.data_quality);

        let need_adjust_in_fetch = self.auto_adjust || self.back_adjust;
        let period_dt = self.precompute_period_dt()?;

        let mut joined: Vec<(usize, String, YfResponse<HistoryResponse>)> =
            stream::iter(symbols.into_iter().enumerate())
                .map(|(index, sym)| {
                    let hb = self.build_history_for_symbol(&sym, period_dt, need_adjust_in_fetch);

                    async move {
                        let full = hb.fetch_full_with_diagnostics().await?;
                        Ok::<(usize, String, YfResponse<HistoryResponse>), YfError>((
                            index, sym, full,
                        ))
                    }
                })
                .buffer_unordered(self.concurrency.get())
                .try_collect()
                .await?;

        joined.sort_unstable_by_key(|(index, _, _)| *index);
        let joined: Vec<(String, YfResponse<HistoryResponse>)> = joined
            .into_iter()
            .map(|(_, sym, full)| (sym, full))
            .collect();

        let response = self
            .process_joined_results(joined, need_adjust_in_fetch, &mut ctx)
            .await?;
        Ok(ctx.finish(response))
    }
}

/* ---------------- internal helpers ---------------- */

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn rounded_price(price: &Price) -> Option<Price> {
    let value = f64_from_currency_value(price)?;
    let decimal = rust_decimal::Decimal::from_f64(round2(value))?;
    Some(Price::new(decimal, price.currency().clone()))
}

/// Very lightweight "repair" pass:
/// If a bar's close is ~100× the average of its neighbors (or ~1/100),
/// scale that entire bar's OHLC accordingly. Volumes are left unchanged.
fn repair_scale_outliers(rows: &mut [Candle]) -> Vec<String> {
    let mut repaired = Vec::new();
    if rows.len() < 3 {
        return repaired;
    }

    for i in 1..rows.len() - 1 {
        // This is intentionally a single forward pass: once a bar is repaired,
        // its corrected close becomes the previous-neighbor baseline for the
        // next bar.
        // Split rows at i, so left[..i] and right[i..] don't overlap.
        let (left, right) = rows.split_at_mut(i);

        // prev is in the left side (immutable is fine)
        let prev = &left[i - 1];

        // Now split the right side so we can mutably borrow the “current” bar
        // and (immutably) the remainder where “next” lives, without overlap.
        let Some((cur, rem)) = right.split_first_mut() else {
            continue;
        };
        let next = &rem[0]; // safe because len >= 2 overall ⇒ rem has at least one

        let p = &prev.close;
        let n = &next.close;
        let c = &cur.close;

        let Some(p_val) = f64_from_currency_value(p).filter(|v| v.is_finite()) else {
            continue;
        };
        let Some(n_val) = f64_from_currency_value(n).filter(|v| v.is_finite()) else {
            continue;
        };
        let Some(c_val) = f64_from_currency_value(c).filter(|v| v.is_finite()) else {
            continue;
        };

        let baseline = f64::midpoint(p_val, n_val);
        if baseline <= 0.0 {
            continue;
        }

        let ratio = c_val / baseline;

        // ~100× high
        if ratio > 50.0 && ratio < 200.0 {
            let scale = if (80.0..125.0).contains(&ratio) {
                0.01
            } else {
                1.0 / ratio
            };
            if scale_row_prices(cur, scale) {
                repaired.push(cur.ts.to_rfc3339());
            }
            continue;
        }

        // ~100× low
        if ratio > 0.0 && ratio < 0.02 {
            let scale = if (0.008..0.0125).contains(&ratio) {
                100.0
            } else {
                1.0 / ratio
            };
            if scale_row_prices(cur, scale) {
                repaired.push(cur.ts.to_rfc3339());
            }
        }
    }
    repaired
}

fn scale_row_prices(c: &mut Candle, scale: f64) -> bool {
    let Some(scale) = rust_decimal::Decimal::from_f64_retain(scale) else {
        return false;
    };

    let Some(open) = scaled_price(&c.open, scale) else {
        return false;
    };
    let Some(high) = scaled_price(&c.high, scale) else {
        return false;
    };
    let Some(low) = scaled_price(&c.low, scale) else {
        return false;
    };
    let Some(close) = scaled_price(&c.close, scale) else {
        return false;
    };
    let close_unadj = match c.close_unadj.as_ref() {
        Some(close_unadj) => {
            let Some(close_unadj) = scaled_price(close_unadj, scale) else {
                return false;
            };
            Some(close_unadj)
        }
        None => None,
    };

    c.open = open;
    c.high = high;
    c.low = low;
    c.close = close;
    c.close_unadj = close_unadj;
    true
}

fn scaled_price(price: &Price, scale: rust_decimal::Decimal) -> Option<Price> {
    if !f64_from_currency_value(price).is_some_and(f64::is_finite) {
        return None;
    }

    price.try_mul(scale).ok()
}
