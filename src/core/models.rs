// Re-export types from paft explicitly
pub use paft::aggregates::Snapshot;
pub use paft::market::action::Action;
pub use paft::market::quote::Quote;
pub use paft::market::requests::history::{Interval, Range};
pub use paft::market::responses::history::{
    AdjustmentAnchor, AdjustmentMethod, Candle, CorporateActionAdjustmentCause,
    CorporateActionAdjustmentCauses, HistoryMeta, HistoryResponse, OhlcPriceBasis, PriceBasis,
};
use paft::money::Price;
use serde::{Deserialize, Serialize};

/// Fast quote-oriented information for a ticker.
///
/// This mirrors Python yfinance's `Ticker.fast_info` surface while keeping
/// instant-in-time quote data separate from derived moving-average metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FastInfo {
    /// Instant-in-time market snapshot data.
    pub snapshot: Snapshot,
    /// Price moving averages exposed by Yahoo's quote surfaces.
    pub moving_averages: MovingAverages,
}

/// Price moving averages exposed in fast ticker information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MovingAverages {
    /// 50 trading-day average price.
    pub fifty_day: Option<Price>,
    /// 200 trading-day average price.
    pub two_hundred_day: Option<Price>,
}

// Helper functions for converting to string representations
pub(crate) const fn range_as_str(range: Range) -> &'static str {
    range.code()
}

pub(crate) const fn interval_as_str(interval: Interval) -> &'static str {
    interval.code()
}
