//! Public profile types + loading strategy (API first, then scrape).
//!
//! Internals are split into:
//! - `api`:    quoteSummary v10 API path
//! - `scrape`: HTML scrape + JSON extraction path
//! - `internal`: common utilities for both API and scrape
//! - `debug`:  optional debug dump helpers (only in debug builds or with `debug-dumps` feature)

mod api;
mod scrape;

#[cfg(feature = "debug-dumps")]
pub(crate) mod debug;

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheMode, RetryConfig},
        conversions::string_to_fund_kind,
    },
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

/// Helper to contain the API->Scrape fallback logic.
#[cfg_attr(feature = "tracing", tracing::instrument(skip(client), err, fields(symbol = %symbol)))]
async fn load_with_fallback(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Profile, YfError> {
    match api::load_from_quote_summary_api(client, symbol, cache_mode, retry_override).await {
        Ok(p) => Ok(p),
        Err(e @ YfError::Auth(_)) => Err(e),
        Err(e) => {
            crate::core::logging::trace_warn!(
                error = %e,
                "profile API failed; falling back to scrape"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = &e;
            scrape::load_from_scrape(client, symbol, cache_mode, retry_override).await
        }
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

async fn load_value_with_fallback(
    client: &YfClient,
    symbol: &str,
    value: serde_json::Value,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Profile, YfError> {
    match load_from_quote_summary_value_api(client, symbol, value).await {
        Ok(p) => Ok(p),
        Err(e @ YfError::Auth(_)) => Err(e),
        Err(e) => {
            crate::core::logging::trace_warn!(
                error = %e,
                "profile batched API data failed; falling back to scrape"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = &e;
            scrape::load_from_scrape(client, symbol, cache_mode, retry_override).await
        }
    }
}

pub(crate) async fn load_profile_from_quote_summary_value(
    client: &YfClient,
    symbol: &str,
    value: serde_json::Value,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Profile, YfError> {
    #[cfg(not(feature = "test-mode"))]
    {
        load_value_with_fallback(client, symbol, value, cache_mode, retry_override).await
    }

    #[cfg(feature = "test-mode")]
    {
        use crate::core::client::ApiPreference;
        match client.api_preference() {
            ApiPreference::ApiThenScrape => {
                load_value_with_fallback(client, symbol, value, cache_mode, retry_override).await
            }
            ApiPreference::ApiOnly => {
                load_from_quote_summary_value_api(client, symbol, value).await
            }
            ApiPreference::ScrapeOnly => {
                scrape::load_from_scrape(client, symbol, cache_mode, retry_override).await
            }
        }
    }
}

pub(crate) async fn load_profile_with_options(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Profile, YfError> {
    let symbol = crate::core::client::normalize_symbol(symbol)?;

    #[cfg(not(feature = "test-mode"))]
    {
        load_with_fallback(client, &symbol, cache_mode, retry_override).await
    }

    #[cfg(feature = "test-mode")]
    {
        use crate::core::client::ApiPreference;
        match client.api_preference() {
            ApiPreference::ApiThenScrape => {
                load_with_fallback(client, &symbol, cache_mode, retry_override).await
            }
            ApiPreference::ApiOnly => {
                api::load_from_quote_summary_api(client, &symbol, cache_mode, retry_override).await
            }
            ApiPreference::ScrapeOnly => {
                scrape::load_from_scrape(client, &symbol, cache_mode, retry_override).await
            }
        }
    }
}

/// Loads the profile for a given symbol.
///
/// This function will try to load the profile from the quote summary API first,
/// and fall back to scraping the quote page if the API fails.
///
/// # Errors
///
/// Returns `YfError` if the network request fails, the response cannot be parsed,
/// or the data for the symbol is not available.
pub async fn load_profile(client: &YfClient, symbol: &str) -> Result<Profile, YfError> {
    load_profile_with_options(client, symbol, CacheMode::Use, None).await
}
