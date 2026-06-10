use crate::core::{
    CallOptions, YfClient, YfError,
    client::{CacheEndpoint, SymbolEndpoint, normalize_symbol},
    net,
};
use serde_json::value::RawValue;
use std::borrow::Cow;

use serde::Deserialize;

#[cfg(feature = "debug-dumps")]
use crate::profile::debug::debug_dump_api;

#[derive(Deserialize)]
struct BorrowedV10Envelope<'a> {
    #[serde(rename = "quoteSummary", borrow)]
    quote_summary: Option<BorrowedV10QuoteSummary<'a>>,
}

#[derive(Deserialize)]
struct BorrowedV10QuoteSummary<'a> {
    #[serde(borrow)]
    result: Option<Vec<&'a RawValue>>,
    error: Option<BorrowedV10Error<'a>>,
}

#[derive(Deserialize)]
struct BorrowedV10Error<'a> {
    description: Cow<'a, str>,
}

#[cfg_attr(
    feature = "tracing",
    tracing::instrument(
        skip(client, options),
        err,
        fields(symbol = %symbol, modules = %modules, caller = %caller)
    )
)]
pub async fn fetch_body(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    caller: &str,
    options: &CallOptions,
) -> Result<String, YfError> {
    let symbol = normalize_symbol(symbol)?;

    async fn attempt_fetch(
        client: &YfClient,
        symbol: &str,
        modules: &str,
        caller: &str,
        options: &CallOptions,
    ) -> Result<String, YfError> {
        let mut url = client.symbol_url(SymbolEndpoint::QuoteSummary, symbol)?;
        url.query_pairs_mut().append_pair("modules", modules);

        // Create a sanitized key from module names for a unique fixture filename.
        let module_key = modules
            .replace(',', "-")
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "");
        let fixture_endpoint = format!("{caller}_api_{module_key}");
        let (text, _) = net::fetch_text_with_auth_retry(
            client,
            url,
            net::AuthFetchConfig {
                auth_mode: net::AuthMode::RequiredCrumb,
                cache_endpoint: CacheEndpoint::QuoteSummary,
                options,
                cache_body: None,
                endpoint: &fixture_endpoint,
                fixture_key: symbol,
                ext: "json",
                retry_on_invalid_crumb_body: true,
                cache_validator: Some(validate_quote_summary_body),
            },
            |url| client.http().get(url),
        )
        .await?;

        #[cfg(feature = "debug-dumps")]
        let _ = debug_dump_api(symbol, &text);

        Ok(text)
    }

    let text = attempt_fetch(client, &symbol, modules, caller, options).await?;

    validate_quote_summary_body(&text)?;

    Ok(text)
}

fn validate_quote_summary_body(body: &str) -> Result<(), YfError> {
    let env: BorrowedV10Envelope<'_> = serde_json::from_str(body).map_err(YfError::json)?;
    reject_borrowed_quote_summary_error(&env)
}

fn reject_borrowed_quote_summary_error(env: &BorrowedV10Envelope<'_>) -> Result<(), YfError> {
    if let Some(error) = env.quote_summary.as_ref().and_then(|qs| qs.error.as_ref()) {
        crate::core::logging::trace_error!(
            description = %error.description,
            "quoteSummary error"
        );
        return Err(YfError::Api(format!("yahoo error: {}", error.description)));
    }

    Ok(())
}

pub fn module_result_raw_value<'a>(body: &'a str) -> Result<&'a RawValue, YfError> {
    let env: BorrowedV10Envelope<'a> = serde_json::from_str(body).map_err(YfError::json)?;

    reject_borrowed_quote_summary_error(&env)?;

    env.quote_summary
        .and_then(|qs| qs.result)
        .and_then(|mut v| v.pop())
        .ok_or_else(|| YfError::MissingData("empty quoteSummary result".into()))
}

pub fn parse_module_result<'de, T>(body: &'de str) -> Result<T, YfError>
where
    T: serde::Deserialize<'de>,
{
    parse_module_result_raw(module_result_raw_value(body)?)
}

pub fn parse_module_result_raw<'de, T>(raw: &'de RawValue) -> Result<T, YfError>
where
    T: serde::Deserialize<'de>,
{
    serde_json::from_str(raw.get()).map_err(YfError::json)
}

pub async fn fetch_module_result<T>(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    caller: &str,
    options: &CallOptions,
) -> Result<T, YfError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let body = fetch_body(client, symbol, modules, caller, options).await?;

    parse_module_result(&body)
}
