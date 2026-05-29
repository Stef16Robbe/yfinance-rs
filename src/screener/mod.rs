//! Yahoo Finance screener support.
//!
//! The screener API is Yahoo-specific by design. Public inputs are strongly
//! typed so unsupported Yahoo fields or values require a crate change instead
//! of silently building invalid provider payloads.

mod builder;
/// Strongly typed Yahoo screener field constants.
pub mod fields;
mod predefined;
/// Strongly typed Yahoo screener query types and vocabularies.
pub mod query;
mod response;

pub use builder::ScreenerBuilder;
pub use fields::{equity_fields, etf_fields, fund_fields};
pub use predefined::PredefinedScreener;
pub use query::{
    Equity, EquityQuery, EquitySector, Etf, EtfCategory, EtfQuery, Fund, FundCategory, FundQuery,
    PercentPoints, Predefined, Rating, Region, ResultOffset, ScreenerCount, ScreenerNumber,
    ScreenerQuery, ScreenerValue, SortDirection, YahooExchangeCode, YahooQuoteType,
};
pub use response::{ScreenerResponse, ScreenerResult};

use crate::{YfClient, YfError, YfResponse};

/// Runs a predefined Yahoo screener.
///
/// # Errors
///
/// Returns `YfError` if the request fails or the response cannot be parsed.
pub async fn screen(
    client: &YfClient,
    screener: PredefinedScreener,
) -> Result<ScreenerResponse, YfError> {
    ScreenerBuilder::predefined(client, screener).fetch().await
}

/// Runs a predefined Yahoo screener with projection diagnostics.
///
/// # Errors
///
/// Returns `YfError` if the request fails or the response cannot be parsed.
pub async fn screen_with_diagnostics(
    client: &YfClient,
    screener: PredefinedScreener,
) -> Result<YfResponse<ScreenerResponse>, YfError> {
    ScreenerBuilder::predefined(client, screener)
        .fetch_with_diagnostics()
        .await
}
