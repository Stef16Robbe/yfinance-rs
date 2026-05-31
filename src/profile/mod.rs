//! Public profile types + loading strategy.
//!
//! Internals are split into:
//! - `api`: quoteSummary v10 API path
//! - `debug`: optional debug dump helpers (only in debug builds or with `debug-dumps` feature)

mod api;

#[cfg(feature = "debug-dumps")]
pub(crate) mod debug;

use crate::{
    YfClient, YfError,
    core::{CallOptions, client::CacheMode, conversions::string_to_fund_kind},
};
use paft::fundamentals::profile::FundKind;

mod model;
pub use model::{Address, Company, Fund, Profile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YahooProfileKind {
    Company,
    Fund(FundQuoteKind),
}

impl YahooProfileKind {
    fn from_quote_type(quote_type: &str) -> Result<Self, YfError> {
        match quote_type {
            "EQUITY" => Ok(Self::Company),
            "ETF" => Ok(Self::Fund(FundQuoteKind::Etf)),
            "MUTUALFUND" => Ok(Self::Fund(FundQuoteKind::MutualFund)),
            other => Err(YfError::InvalidParams(format!(
                "unsupported quoteType: {other}"
            ))),
        }
    }

    const fn quote_type(self) -> &'static str {
        match self {
            Self::Company => "EQUITY",
            Self::Fund(fund) => fund.quote_type(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FundQuoteKind {
    Etf,
    MutualFund,
}

impl FundQuoteKind {
    const fn quote_type(self) -> &'static str {
        match self {
            Self::Etf => "ETF",
            Self::MutualFund => "MUTUALFUND",
        }
    }

    const fn fund_kind(self) -> FundKind {
        match self {
            Self::Etf => FundKind::Etf,
            Self::MutualFund => FundKind::MutualFund,
        }
    }
}

fn resolve_fund_kind(
    legal_type: Option<String>,
    quote_kind: FundQuoteKind,
) -> Result<FundKind, YfError> {
    if let Some(kind) = string_to_fund_kind(legal_type)? {
        return Ok(kind);
    }

    match quote_kind {
        FundQuoteKind::MutualFund => Ok(quote_kind.fund_kind()),
        FundQuoteKind::Etf => Err(YfError::MissingData("fundProfile.legalType missing".into())),
    }
}

async fn load_from_quote_summary_value_api(
    client: &YfClient,
    symbol: &str,
    value: serde_json::Value,
) -> Result<Profile, YfError> {
    let root: api::V10Result = serde_json::from_value(value).map_err(YfError::Json)?;
    api::load_from_quote_summary_result(client, symbol, root).await
}

pub(crate) async fn load_profile_from_quote_summary_value(
    client: &YfClient,
    symbol: &str,
    value: serde_json::Value,
    _options: &CallOptions,
) -> Result<Profile, YfError> {
    load_from_quote_summary_value_api(client, symbol, value).await
}

pub(crate) async fn load_profile_with_options(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<Profile, YfError> {
    let symbol = crate::core::client::normalize_symbol(symbol)?;
    api::load_from_quote_summary_api(client, &symbol, options).await
}

/// Loads the profile for a given symbol.
///
/// This function loads the profile from Yahoo's quoteSummary API.
///
/// # Errors
///
/// Returns `YfError` if the network request fails, the response cannot be parsed,
/// or the data for the symbol is not available.
pub async fn load_profile(client: &YfClient, symbol: &str) -> Result<Profile, YfError> {
    let options = CallOptions::default().with_cache_mode(CacheMode::Use);
    load_profile_with_options(client, symbol, &options).await
}
