//! quoteSummary v10 API path for profiles.

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheMode, RetryConfig},
        currency_resolver::CurrencyHints,
        quotesummary,
    },
};
use paft::domain::Isin;
use serde::Deserialize;

use super::{Address, Company, Fund, Profile, YahooProfileKind, resolve_fund_kind};

pub async fn load_from_quote_summary_api(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Profile, YfError> {
    let first: V10Result = quotesummary::fetch_module_result(
        client,
        symbol,
        "assetProfile,quoteType,fundProfile",
        "profile",
        cache_mode,
        retry_override,
    )
    .await?;

    let kind = first
        .quote_type
        .as_ref()
        .and_then(|q| q.quote_type.as_deref())
        .unwrap_or("");

    let name = first
        .quote_type
        .as_ref()
        .and_then(|q| q.long_name.clone().or_else(|| q.short_name.clone()))
        .unwrap_or_else(|| symbol.to_string());

    match YahooProfileKind::from_quote_type(kind)? {
        YahooProfileKind::Company => {
            let sp = first
                .asset_profile
                .ok_or_else(|| YfError::MissingData("assetProfile missing".into()))?;
            let exchange = first
                .quote_type
                .as_ref()
                .and_then(|q| q.exchange.as_deref());
            let country = sp.country.clone();
            client
                .store_currency_hints(
                    symbol,
                    CurrencyHints::from_profile(
                        country.as_deref(),
                        exchange,
                        Some(YahooProfileKind::Company.quote_type()),
                    ),
                )
                .await;
            let address = Address {
                street1: sp.address1,
                street2: sp.address2,
                city: sp.city,
                state: sp.state,
                country: sp.country,
                zip: sp.zip,
            };
            // Validate ISIN if present, return None if invalid
            let validated_isin = sp.isin.and_then(|isin_str| Isin::new(&isin_str).ok());

            Ok(Profile::Company(Company {
                name,
                sector: sp.sector,
                industry: sp.industry,
                website: sp.website,
                summary: sp.long_business_summary,
                address: Some(address),
                isin: validated_isin,
            }))
        }
        YahooProfileKind::Fund(fund_quote_kind) => {
            let fp = first
                .fund_profile
                .ok_or_else(|| YfError::MissingData("fundProfile missing".into()))?;
            let exchange = first
                .quote_type
                .as_ref()
                .and_then(|q| q.exchange.as_deref());
            client
                .store_currency_hints(
                    symbol,
                    CurrencyHints::from_profile(None, exchange, Some(fund_quote_kind.quote_type())),
                )
                .await;

            // Validate ISIN if present, return None if invalid
            let validated_isin = fp.isin.and_then(|isin_str| Isin::new(&isin_str).ok());

            Ok(Profile::Fund(Fund {
                name,
                family: fp.family,
                kind: resolve_fund_kind(fp.legal_type, fund_quote_kind)?,
                isin: validated_isin,
            }))
        }
    }
}

/* --------- Minimal serde mapping for the API JSON --------- */

#[derive(Deserialize)]
struct V10Result {
    #[serde(rename = "assetProfile")]
    asset_profile: Option<V10AssetProfile>,
    #[serde(rename = "fundProfile")]
    fund_profile: Option<V10FundProfile>,
    #[serde(rename = "quoteType")]
    quote_type: Option<V10QuoteType>,
}

#[derive(Deserialize)]
struct V10AssetProfile {
    address1: Option<String>,
    address2: Option<String>,
    city: Option<String>,
    state: Option<String>,
    country: Option<String>,
    zip: Option<String>,
    sector: Option<String>,
    industry: Option<String>,
    website: Option<String>,
    #[serde(rename = "longBusinessSummary")]
    long_business_summary: Option<String>,
    isin: Option<String>,
}

#[derive(Deserialize)]
struct V10FundProfile {
    #[serde(rename = "legalType")]
    legal_type: Option<String>,
    family: Option<String>,
    isin: Option<String>,
}

#[derive(Deserialize)]
struct V10QuoteType {
    exchange: Option<String>,

    #[serde(rename = "quoteType")]
    quote_type: Option<String>,
    #[serde(rename = "longName")]
    long_name: Option<String>,
    #[serde(rename = "shortName")]
    short_name: Option<String>,
}
