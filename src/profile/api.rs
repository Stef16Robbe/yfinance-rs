//! quoteSummary v10 API path for profiles.

use crate::{
    YfClient, YfError,
    core::{CallOptions, currency_resolver::CurrencyHints, quotesummary, wire::WireValue},
};
use paft::domain::Isin;
use serde::Deserialize;

use super::{Address, Company, Fund, Profile, YahooProfileKind, resolve_fund_kind};

pub async fn load_from_quote_summary_api(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<Profile, YfError> {
    let first: V10Result = quotesummary::fetch_module_result(
        client,
        symbol,
        "assetProfile,quoteType,fundProfile",
        "profile",
        options,
    )
    .await?;

    load_from_quote_summary_result(client, symbol, first).await
}

pub(super) async fn load_from_quote_summary_result(
    client: &YfClient,
    symbol: &str,
    first: V10Result,
) -> Result<Profile, YfError> {
    let kind = first
        .quote_type
        .as_ref()
        .and_then(|q| q.quote_type.as_ref().map(String::as_str))
        .unwrap_or("");

    let name = first
        .quote_type
        .as_ref()
        .and_then(|q| {
            q.long_name
                .as_ref()
                .cloned()
                .or_else(|| q.short_name.as_ref().cloned())
        })
        .unwrap_or_else(|| symbol.to_string());

    match YahooProfileKind::from_quote_type(kind)? {
        YahooProfileKind::Company => {
            let sp = first
                .asset_profile
                .into_option()
                .ok_or_else(|| YfError::MissingData("assetProfile missing".into()))?;
            let exchange = first
                .quote_type
                .as_ref()
                .and_then(|q| q.exchange.as_ref().map(String::as_str));
            let country = sp.country.as_ref().cloned();
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
                street1: sp.address1.into_option(),
                street2: sp.address2.into_option(),
                city: sp.city.into_option(),
                state: sp.state.into_option(),
                country: sp.country.into_option(),
                zip: sp.zip.into_option(),
            };
            // Validate ISIN if present, return None if invalid
            let validated_isin = sp
                .isin
                .into_option()
                .and_then(|isin_str| Isin::new(&isin_str).ok());

            Ok(Profile::Company(Company {
                name,
                sector: sp.sector.into_option(),
                industry: sp.industry.into_option(),
                website: sp.website.into_option(),
                summary: sp.long_business_summary.into_option(),
                address: Some(address),
                isin: validated_isin,
            }))
        }
        YahooProfileKind::Fund(fund_quote_kind) => {
            let fp = first
                .fund_profile
                .into_option()
                .ok_or_else(|| YfError::MissingData("fundProfile missing".into()))?;
            let exchange = first
                .quote_type
                .as_ref()
                .and_then(|q| q.exchange.as_ref().map(String::as_str));
            client
                .store_currency_hints(
                    symbol,
                    CurrencyHints::from_profile(None, exchange, Some(fund_quote_kind.quote_type())),
                )
                .await;

            // Validate ISIN if present, return None if invalid
            let validated_isin = fp
                .isin
                .into_option()
                .and_then(|isin_str| Isin::new(&isin_str).ok());

            Ok(Profile::Fund(Fund {
                name,
                family: fp.family.into_option(),
                kind: resolve_fund_kind(fp.legal_type.into_option(), fund_quote_kind)?,
                isin: validated_isin,
            }))
        }
    }
}

/* --------- Minimal serde mapping for the API JSON --------- */

#[derive(Deserialize)]
pub(super) struct V10Result {
    #[serde(rename = "assetProfile")]
    #[serde(default)]
    asset_profile: WireValue<V10AssetProfile>,
    #[serde(rename = "fundProfile")]
    #[serde(default)]
    fund_profile: WireValue<V10FundProfile>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: WireValue<V10QuoteType>,
}

#[derive(Deserialize)]
struct V10AssetProfile {
    #[serde(default)]
    address1: WireValue<String>,
    #[serde(default)]
    address2: WireValue<String>,
    #[serde(default)]
    city: WireValue<String>,
    #[serde(default)]
    state: WireValue<String>,
    #[serde(default)]
    country: WireValue<String>,
    #[serde(default)]
    zip: WireValue<String>,
    #[serde(default)]
    sector: WireValue<String>,
    #[serde(default)]
    industry: WireValue<String>,
    #[serde(default)]
    website: WireValue<String>,
    #[serde(rename = "longBusinessSummary")]
    #[serde(default)]
    long_business_summary: WireValue<String>,
    #[serde(default)]
    isin: WireValue<String>,
}

#[derive(Deserialize)]
struct V10FundProfile {
    #[serde(rename = "legalType")]
    #[serde(default)]
    legal_type: WireValue<String>,
    #[serde(default)]
    family: WireValue<String>,
    #[serde(default)]
    isin: WireValue<String>,
}

#[derive(Deserialize)]
struct V10QuoteType {
    #[serde(default)]
    exchange: WireValue<String>,

    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: WireValue<String>,
    #[serde(rename = "longName")]
    #[serde(default)]
    long_name: WireValue<String>,
    #[serde(rename = "shortName")]
    #[serde(default)]
    short_name: WireValue<String>,
}
