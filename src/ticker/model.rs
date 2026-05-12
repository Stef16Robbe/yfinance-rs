// Re-export types from paft without using prelude
pub use paft::market::options::{OptionChain, OptionContract};

use chrono::{DateTime, NaiveDate, Utc};
use paft::Decimal;
use paft::domain::{Exchange, Instrument, Isin, MarketState};
use paft::fundamentals::analysis::{PriceTarget, RecommendationSummary};
use paft::fundamentals::esg::EsgScores;
use paft::money::{Currency, Money};
use serde::{Deserialize, Serialize};

/// Detailed instrument profile and market snapshot.
///
/// Includes identification fields, real-time snapshot metrics, intraday and
/// 52-week ranges, as well as a subset of fundamentals. All values are optional
/// to accommodate partially populated data from upstream sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    /// Primary instrument as provided by the data source.
    pub instrument: Instrument,
    /// Human-friendly instrument name.
    pub name: Option<String>,
    /// International Securities Identification Number.
    pub isin: Option<Isin>,
    /// Primary listing exchange, if known.
    pub exchange: Option<Exchange>,
    /// Current market session state.
    pub market_state: Option<MarketState>,
    /// Quote currency for all monetary values in this snapshot.
    pub currency: Option<Currency>,
    /// Most recent traded/quoted price.
    pub last: Option<Money>,
    /// Opening price for the current session.
    pub open: Option<Money>,
    /// Highest traded price observed during the current session.
    pub high: Option<Money>,
    /// Lowest traded price observed during the current session.
    pub low: Option<Money>,
    /// Previous session's official close price.
    pub previous_close: Option<Money>,
    /// Intraday low for the current session.
    pub day_range_low: Option<Money>,
    /// Intraday high for the current session.
    pub day_range_high: Option<Money>,
    /// 52-week low.
    pub fifty_two_week_low: Option<Money>,
    /// 52-week high.
    pub fifty_two_week_high: Option<Money>,
    /// Today's trading volume.
    pub volume: Option<u64>,
    /// Average daily trading volume.
    pub average_volume: Option<u64>,
    /// Market capitalization in the quote currency.
    pub market_cap: Option<Money>,
    /// Number of shares currently outstanding.
    pub shares_outstanding: Option<u64>,
    /// Earnings per share, trailing twelve months.
    pub eps_ttm: Option<Money>,
    /// Price-to-earnings ratio, trailing twelve months.
    pub pe_ttm: Option<Decimal>,
    /// Dividend yield as a fraction.
    pub dividend_yield: Option<Decimal>,
    /// Most recent ex-dividend date.
    pub ex_dividend_date: Option<NaiveDate>,
    /// Analyst price target summary.
    pub price_target: Option<PriceTarget>,
    /// Latest recommendation summary.
    pub recommendation_summary: Option<RecommendationSummary>,
    /// ESG scores.
    pub esg_scores: Option<EsgScores>,
    /// Timestamp when this snapshot was taken.
    #[serde(with = "chrono::serde::ts_seconds_option")]
    pub as_of: Option<DateTime<Utc>>,
}
