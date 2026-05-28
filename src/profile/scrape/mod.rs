//! Scrape the Yahoo quote HTML and extract profile data.

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheEndpoint, CacheMode, RetryConfig, SymbolEndpoint},
        currency_resolver::CurrencyHints,
    },
};
use paft::domain::Isin;
use serde::Deserialize;

use super::{Address, Company, Fund, Profile, YahooProfileKind, resolve_fund_kind};

#[cfg(feature = "debug-dumps")]
use crate::profile::debug::{debug_dump_extracted_json, debug_dump_html};

pub mod extract;
pub mod utils;
use extract::extract_bootstrap_json;

#[allow(clippy::too_many_lines)]
pub async fn load_from_scrape(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Profile, YfError> {
    let mut url = client.symbol_url(SymbolEndpoint::Quote, symbol)?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("p", symbol);
    }

    let body = crate::core::net::fetch_text_cached(
        client,
        &url,
        crate::core::net::CacheFetchConfig {
            cache_endpoint: CacheEndpoint::ProfileHtml,
            cache_mode,
            retry_override,
            endpoint: "profile_html",
            fixture_key: symbol,
            ext: "html",
        },
    )
    .await?;

    #[cfg(feature = "debug-dumps")]
    {
        let _ = debug_dump_html(symbol, &body);
    }

    let json_str = extract_bootstrap_json(&body)?;
    #[cfg(feature = "debug-dumps")]
    {
        let _ = debug_dump_extracted_json(symbol, &json_str);
    }

    let boot: Bootstrap = serde_json::from_str(&json_str).map_err(YfError::Json)?;

    let store = boot.context.dispatcher.stores.quote_summary_store;

    let name = store
        .quote_type
        .as_ref()
        .and_then(|qt| qt.long_name.clone().or_else(|| qt.short_name.clone()))
        .or_else(|| {
            store
                .price
                .as_ref()
                .and_then(|p| p.long_name.clone().or_else(|| p.short_name.clone()))
        })
        .unwrap_or_else(|| symbol.to_string());

    let inferred_kind = if let Some(fp) = store.fund_profile.as_ref() {
        if fp.legal_type.as_deref() == Some("Mutual Fund") {
            Some("MUTUALFUND")
        } else {
            Some("ETF")
        }
    } else if store.summary_profile.is_some() {
        Some("EQUITY")
    } else {
        None
    };
    let kind = store
        .quote_type
        .as_ref()
        .and_then(|qt| qt.kind.as_deref())
        .or(inferred_kind)
        .unwrap_or("");

    crate::core::logging::trace_debug!(
        kind,
        name,
        quote_type_present = store.quote_type.is_some(),
        price_present = store.price.is_some(),
        has_summary_profile = store.summary_profile.is_some(),
        has_fund_profile = store.fund_profile.is_some(),
        "resolved profile kind from scraped Yahoo payload"
    );

    match YahooProfileKind::from_quote_type(kind)? {
        YahooProfileKind::Company => {
            let sp = store
                .summary_profile
                .ok_or_else(|| YfError::MissingData("summaryProfile missing".into()))?;
            let exchange = store
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
            let fp = store
                .fund_profile
                .ok_or_else(|| YfError::MissingData("fundProfile missing".into()))?;
            let exchange = store
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

/* --------- Minimal serde mapping for the bootstrap JSON --------- */

#[derive(Deserialize)]
struct Bootstrap {
    context: Ctx,
}

#[derive(Deserialize)]
struct Ctx {
    dispatcher: Dispatch,
}

#[derive(Deserialize)]
struct Dispatch {
    stores: Stores,
}

#[derive(Deserialize)]
struct Stores {
    #[serde(rename = "QuoteSummaryStore")]
    quote_summary_store: QuoteSummaryStore,
}

#[derive(Deserialize)]
struct QuoteSummaryStore {
    #[serde(rename = "quoteType")]
    quote_type: Option<QuoteTypeNode>,

    #[serde(default)]
    price: Option<PriceNode>,

    #[serde(rename = "summaryProfile")]
    summary_profile: Option<SummaryProfileNode>,

    #[serde(rename = "fundProfile")]
    fund_profile: Option<FundProfileNode>,
}

#[derive(Deserialize)]
struct QuoteTypeNode {
    exchange: Option<String>,

    #[serde(rename = "quoteType")]
    kind: Option<String>,

    #[serde(rename = "longName")]
    long_name: Option<String>,

    #[serde(rename = "shortName")]
    short_name: Option<String>,
}

#[derive(Deserialize)]
struct PriceNode {
    #[serde(rename = "longName")]
    long_name: Option<String>,
    #[serde(rename = "shortName")]
    short_name: Option<String>,
}

#[derive(Deserialize)]
struct SummaryProfileNode {
    address1: Option<String>,
    address2: Option<String>,
    city: Option<String>,
    state: Option<String>,
    country: Option<String>,
    zip: Option<String>,
    sector: Option<String>,
    industry: Option<String>,

    #[serde(rename = "longBusinessSummary")]
    long_business_summary: Option<String>,

    website: Option<String>,
    isin: Option<String>,
}

#[derive(Deserialize)]
struct FundProfileNode {
    #[serde(rename = "legalType")]
    legal_type: Option<String>,
    family: Option<String>,
    isin: Option<String>,
}
