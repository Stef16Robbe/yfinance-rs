//! quoteSummary v10 API path for profiles.

use crate::{
    YfClient, YfError,
    core::{
        CallOptions, ProjectionContext, ProjectionIssue, YfResponse,
        currency_resolver::CurrencyHints,
        diagnostics::{WireProjection, optional_projected},
        quotesummary,
        wire::{BufferedWireValue, WireField, WireValue},
    },
};
use paft::domain::Isin;
use serde::Deserialize;

use super::{Address, Company, Fund, Profile, YahooProfileKind, resolve_fund_kind};

pub async fn load_from_quote_summary_api_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<YfResponse<Profile>, YfError> {
    let first: V10Result = quotesummary::fetch_module_result(
        client,
        symbol,
        "assetProfile,quoteType,fundProfile",
        "profile",
        options,
    )
    .await?;

    load_from_quote_summary_result_with_diagnostics(client, symbol, &first, options.data_quality())
}

pub(super) fn load_from_quote_summary_result_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    first: &V10Result,
    data_quality: crate::core::DataQuality,
) -> Result<YfResponse<Profile>, YfError> {
    let mut ctx = ProjectionContext::new("profile", data_quality);
    let base = profile_base(&mut ctx, symbol, first)?;
    let profile = match base.kind {
        YahooProfileKind::Company => {
            map_company_profile(&mut ctx, client, symbol, first, base.name, base.exchange)?
        }
        YahooProfileKind::Fund(fund_quote_kind) => map_fund_profile(
            &mut ctx,
            client,
            symbol,
            first,
            base.name,
            base.exchange,
            fund_quote_kind,
        )?,
    };

    Ok(ctx.finish(profile))
}

struct ProfileBase<'a> {
    kind: YahooProfileKind,
    name: String,
    exchange: Option<&'a str>,
}

fn profile_base<'a>(
    ctx: &mut ProjectionContext,
    symbol: &str,
    first: &'a V10Result,
) -> Result<ProfileBase<'a>, YfError> {
    let Some(quote_type) = module_ref(ctx, "quoteType", "quoteType", &first.quote_type)? else {
        return Err(YfError::MissingData("quoteType missing".into()));
    };

    let kind = quote_type
        .quote_type
        .optional_cloned_field(ctx, "quoteType.quoteType", Some(symbol), "quoteType")?
        .ok_or_else(|| YfError::MissingData("quoteType.quoteType missing".into()))?;
    let long_name = quote_type.long_name.optional_cloned_field(
        ctx,
        "quoteType.longName",
        Some(symbol),
        "longName",
    )?;
    let short_name = quote_type.short_name.optional_cloned_field(
        ctx,
        "quoteType.shortName",
        Some(symbol),
        "shortName",
    )?;
    let exchange = quote_type.exchange.optional_ref_field(
        ctx,
        "quoteType.exchange",
        Some(symbol),
        "exchange",
    )?;
    let name = long_name
        .or(short_name)
        .unwrap_or_else(|| symbol.to_string());

    Ok(ProfileBase {
        kind: YahooProfileKind::from_quote_type(&kind)?,
        name,
        exchange: exchange.map(String::as_str),
    })
}

fn map_company_profile(
    ctx: &mut ProjectionContext,
    client: &YfClient,
    symbol: &str,
    first: &V10Result,
    name: String,
    exchange: Option<&str>,
) -> Result<Profile, YfError> {
    let Some(sp) = module_ref(ctx, "assetProfile", "assetProfile", &first.asset_profile)? else {
        return Err(YfError::MissingData("assetProfile missing".into()));
    };
    let address = map_address(ctx, symbol, sp)?;
    client.store_currency_hints(
        symbol,
        CurrencyHints::from_profile(
            address.country.as_deref(),
            exchange,
            Some(YahooProfileKind::Company.quote_type()),
        ),
    );

    let isin = sp
        .isin
        .optional_cloned_field(ctx, "assetProfile.isin", Some(symbol), "isin")?;
    let isin = optional_isin(ctx, "assetProfile.isin", symbol, isin)?;

    Ok(Profile::Company(Company {
        name,
        sector: sp.sector.optional_cloned_field(
            ctx,
            "assetProfile.sector",
            Some(symbol),
            "sector",
        )?,
        industry: sp.industry.optional_cloned_field(
            ctx,
            "assetProfile.industry",
            Some(symbol),
            "industry",
        )?,
        website: sp.website.optional_cloned_field(
            ctx,
            "assetProfile.website",
            Some(symbol),
            "website",
        )?,
        summary: sp.long_business_summary.optional_cloned_field(
            ctx,
            "assetProfile.longBusinessSummary",
            Some(symbol),
            "longBusinessSummary",
        )?,
        address: Some(address),
        isin,
    }))
}

fn map_address(
    ctx: &mut ProjectionContext,
    symbol: &str,
    sp: &V10AssetProfile,
) -> Result<Address, YfError> {
    Ok(Address {
        street1: sp.address1.optional_cloned_field(
            ctx,
            "assetProfile.address1",
            Some(symbol),
            "address1",
        )?,
        street2: sp.address2.optional_cloned_field(
            ctx,
            "assetProfile.address2",
            Some(symbol),
            "address2",
        )?,
        city: sp
            .city
            .optional_cloned_field(ctx, "assetProfile.city", Some(symbol), "city")?,
        state: sp
            .state
            .optional_cloned_field(ctx, "assetProfile.state", Some(symbol), "state")?,
        country: sp.country.optional_cloned_field(
            ctx,
            "assetProfile.country",
            Some(symbol),
            "country",
        )?,
        zip: sp
            .zip
            .optional_cloned_field(ctx, "assetProfile.zip", Some(symbol), "zip")?,
    })
}

fn map_fund_profile(
    ctx: &mut ProjectionContext,
    client: &YfClient,
    symbol: &str,
    first: &V10Result,
    name: String,
    exchange: Option<&str>,
    fund_quote_kind: super::FundQuoteKind,
) -> Result<Profile, YfError> {
    let Some(fp) = module_ref(ctx, "fundProfile", "fundProfile", &first.fund_profile)? else {
        return Err(YfError::MissingData("fundProfile missing".into()));
    };
    client.store_currency_hints(
        symbol,
        CurrencyHints::from_profile(None, exchange, Some(fund_quote_kind.quote_type())),
    );

    let isin = fp
        .isin
        .optional_cloned_field(ctx, "fundProfile.isin", Some(symbol), "isin")?;
    let isin = optional_isin(ctx, "fundProfile.isin", symbol, isin)?;
    let legal_type = fp.legal_type.optional_cloned_field(
        ctx,
        "fundProfile.legalType",
        Some(symbol),
        "legalType",
    )?;

    Ok(Profile::Fund(Fund {
        name,
        family: fp.family.optional_cloned_field(
            ctx,
            "fundProfile.family",
            Some(symbol),
            "family",
        )?,
        kind: resolve_fund_kind(legal_type, fund_quote_kind)?,
        isin,
    }))
}

fn module_ref<'a, T>(
    ctx: &mut ProjectionContext,
    feature: &'static str,
    field: &'static str,
    value: &'a impl WireField<T>,
) -> Result<Option<&'a T>, YfError> {
    if let Some(details) = value.invalid_details() {
        ctx.provider_feature_unavailable(
            feature,
            ProjectionIssue::InvalidField {
                field,
                details: details.to_string(),
            },
        )?;
        return Ok(None);
    }

    Ok(value.as_ref())
}

fn optional_isin(
    ctx: &mut ProjectionContext,
    path: &'static str,
    symbol: &str,
    value: Option<String>,
) -> Result<Option<Isin>, YfError> {
    optional_projected(ctx, path, Some(symbol), value, |value| {
        Isin::new(&value).map_err(|err| ProjectionIssue::InvalidField {
            field: "isin",
            details: err.to_string(),
        })
    })
}

/* --------- Minimal serde mapping for the API JSON --------- */

#[derive(Deserialize)]
pub(super) struct V10Result {
    #[serde(rename = "assetProfile")]
    #[serde(default)]
    asset_profile: BufferedWireValue<V10AssetProfile>,
    #[serde(rename = "fundProfile")]
    #[serde(default)]
    fund_profile: BufferedWireValue<V10FundProfile>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: BufferedWireValue<V10QuoteType>,
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
