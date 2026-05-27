// Re-export types from paft explicitly
pub use paft::aggregates::Snapshot as FastInfo;
pub use paft::market::action::Action;
pub use paft::market::quote::Quote;
pub use paft::market::requests::history::{Interval, Range};
pub use paft::market::responses::history::{Candle, HistoryMeta, HistoryResponse};

// Helper functions for converting to string representations
pub(crate) const fn range_as_str(range: Range) -> &'static str {
    range.code()
}

pub(crate) const fn interval_as_str(interval: Interval) -> &'static str {
    interval.code()
}
