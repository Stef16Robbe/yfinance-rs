// Re-export types from paft without using prelude
pub use paft::market::options::{OptionChain, OptionContract};

use crate::profile::Profile;
use paft::aggregates::Snapshot;
use paft::fundamentals::analysis::{PriceTarget, RecommendationSummary};
use paft::fundamentals::esg::EsgScores;
use paft::fundamentals::statements::Calendar;
use paft::fundamentals::statistics::KeyStatistics;
use serde::{Deserialize, Serialize};

/// Composed yfinance-style instrument information.
///
/// This mirrors the broad intent of Python yfinance's `Ticker.info` while
/// keeping the data grouped by paft's provider-agnostic domain models instead
/// of exposing Yahoo's raw response shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// Instant-in-time market snapshot data.
    pub snapshot: Snapshot,
    /// Valuation, dividend, volume, and risk statistics.
    pub key_statistics: KeyStatistics,
    /// Company or fund profile, when available.
    pub profile: Option<Profile>,
    /// Corporate calendar, including dividend payment and ex-dividend dates.
    pub calendar: Option<Calendar>,
    /// Analyst price target summary.
    pub price_target: Option<PriceTarget>,
    /// Latest recommendation summary.
    pub recommendation_summary: Option<RecommendationSummary>,
    /// ESG scores.
    pub esg_scores: Option<EsgScores>,
}
