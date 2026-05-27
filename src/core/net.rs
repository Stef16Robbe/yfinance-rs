#[cfg(feature = "test-mode")]
use std::env;

use serde::de::DeserializeOwned;
use url::Url;

use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
};

/// Read the response body as text.
/// In `test-mode`, if `YF_RECORD=1`, the body is saved as a fixture via `net_fixtures`.
#[allow(unused_variables)]
pub async fn get_text(
    resp: reqwest::Response,
    endpoint: &str,
    symbol: &str,
    ext: &str,
) -> Result<String, reqwest::Error> {
    let text = resp.text().await?;

    #[cfg(feature = "test-mode")]
    {
        if env::var("YF_RECORD").ok().as_deref() == Some("1")
            && let Err(e) = crate::core::fixtures::record_fixture(endpoint, symbol, ext, &text)
        {
            eprintln!("YF_RECORD: failed to write fixture for {symbol}: {e}");
        }
    }

    Ok(text)
}

#[must_use]
pub fn status_error_code(status: u16, url: &Url) -> YfError {
    let url = url.to_string();
    match status {
        404 => YfError::NotFound { url },
        429 => YfError::RateLimited { url },
        500..=599 => YfError::ServerError { status, url },
        _ => YfError::Status { status, url },
    }
}

#[must_use]
pub fn status_error(status: reqwest::StatusCode, url: &Url) -> YfError {
    status_error_code(status.as_u16(), url)
}

pub async fn get_success_text(
    resp: reqwest::Response,
    url: &Url,
    endpoint: &str,
    symbol: &str,
    ext: &str,
) -> Result<String, YfError> {
    if !resp.status().is_success() {
        return Err(status_error(resp.status(), url));
    }

    get_text(resp, endpoint, symbol, ext)
        .await
        .map_err(YfError::Http)
}

pub async fn fetch_text(
    client: &YfClient,
    req: reqwest::RequestBuilder,
    url: &Url,
    retry_override: Option<&RetryConfig>,
    endpoint: &str,
    symbol: &str,
    ext: &str,
) -> Result<String, YfError> {
    let resp = client.send_with_retry(req, retry_override).await?;
    get_success_text(resp, url, endpoint, symbol, ext).await
}

pub async fn fetch_json<T>(
    client: &YfClient,
    req: reqwest::RequestBuilder,
    url: &Url,
    retry_override: Option<&RetryConfig>,
    endpoint: &str,
    symbol: &str,
    ext: &str,
) -> Result<T, YfError>
where
    T: DeserializeOwned,
{
    let text = fetch_text(client, req, url, retry_override, endpoint, symbol, ext).await?;
    serde_json::from_str(&text).map_err(YfError::Json)
}

pub async fn fetch_text_cached(
    client: &YfClient,
    url: &Url,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    endpoint: &str,
    symbol: &str,
    ext: &str,
) -> Result<String, YfError> {
    if cache_mode == CacheMode::Use
        && let Some(text) = client.cache_get(url).await
    {
        return Ok(text);
    }

    let text = fetch_text(
        client,
        client.http().get(url.clone()),
        url,
        retry_override,
        endpoint,
        symbol,
        ext,
    )
    .await?;

    if cache_mode != CacheMode::Bypass {
        client.cache_put(url, &text, None).await;
    }

    Ok(text)
}
