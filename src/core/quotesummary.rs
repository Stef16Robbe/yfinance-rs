use crate::core::{
    YfClient, YfError,
    client::{CacheEndpoint, CacheMode, RetryConfig, SymbolEndpoint},
    net,
};
use serde::Deserialize;

#[cfg(feature = "debug-dumps")]
use crate::profile::debug::debug_dump_api;

#[derive(Deserialize)]
pub struct V10Envelope {
    #[serde(rename = "quoteSummary")]
    pub(crate) quote_summary: Option<V10QuoteSummary>,
}

#[derive(Deserialize)]
pub struct V10QuoteSummary {
    pub(crate) result: Option<Vec<serde_json::Value>>,
    pub(crate) error: Option<V10Error>,
}

#[derive(Deserialize)]
pub struct V10Error {
    pub(crate) description: String,
}

#[cfg_attr(
    feature = "tracing",
    tracing::instrument(
        skip(client, cache_mode, retry_override),
        err,
        fields(symbol = %symbol, modules = %modules, caller = %caller)
    )
)]
pub async fn fetch(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    caller: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<V10Envelope, YfError> {
    async fn attempt_fetch(
        client: &YfClient,
        symbol: &str,
        modules: &str,
        caller: &str,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<V10Envelope, YfError> {
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
                cache_mode,
                cache_body: None,
                retry_override,
                endpoint: &fixture_endpoint,
                fixture_key: symbol,
                ext: "json",
                retry_on_invalid_crumb_body: true,
            },
            |url| client.http().get(url),
        )
        .await?;

        #[cfg(feature = "debug-dumps")]
        let _ = debug_dump_api(symbol, &text);

        serde_json::from_str(&text).map_err(YfError::Json)
    }

    let env = attempt_fetch(client, symbol, modules, caller, cache_mode, retry_override).await?;

    if let Some(error) = env.quote_summary.as_ref().and_then(|qs| qs.error.as_ref()) {
        #[cfg(feature = "tracing")]
        tracing::event!(tracing::Level::ERROR, description = %error.description, "quoteSummary error");
        return Err(YfError::Api(format!("yahoo error: {}", error.description)));
    }

    Ok(env)
}

pub async fn fetch_module_result<T>(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    caller: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<T, YfError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let env = fetch(client, symbol, modules, caller, cache_mode, retry_override).await?;

    let result_val = env
        .quote_summary
        .and_then(|qs| qs.result)
        .and_then(|mut v| v.pop())
        .ok_or_else(|| YfError::MissingData("empty quoteSummary result".into()))?;

    serde_json::from_value(result_val).map_err(YfError::Json)
}
