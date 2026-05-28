#[cfg(feature = "test-mode")]
use std::env;

use serde::de::DeserializeOwned;
use url::Url;

use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMode {
    OptionalCrumb,
    RequiredCrumb,
}

#[derive(Clone, Copy, Debug)]
pub struct AuthFetchConfig<'a> {
    pub auth_mode: AuthMode,
    pub cache_mode: CacheMode,
    pub retry_override: Option<&'a RetryConfig>,
    pub endpoint: &'a str,
    pub fixture_key: &'a str,
    pub ext: &'a str,
    pub retry_on_invalid_crumb_body: bool,
}

enum AuthAttempt {
    Success { body: String, url: Url },
    Status { status: u16, url: Url },
    InvalidCrumb,
}

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

pub async fn fetch_text_with_auth_retry<F>(
    client: &YfClient,
    base_url: Url,
    config: AuthFetchConfig<'_>,
    build_request: F,
) -> Result<(String, Url), YfError>
where
    F: Fn(Url) -> reqwest::RequestBuilder + Send + Sync,
{
    match config.auth_mode {
        AuthMode::OptionalCrumb => {
            match fetch_text_auth_attempt(client, base_url.clone(), config, &build_request, true)
                .await?
            {
                AuthAttempt::Success { body, url } => Ok((body, url)),
                AuthAttempt::Status { status, url } if !is_auth_status(status) => {
                    Err(status_error_code(status, &url))
                }
                AuthAttempt::InvalidCrumb => {
                    retry_with_fresh_crumb(client, base_url, config, &build_request).await
                }
                AuthAttempt::Status { .. } => {
                    let crumb =
                        ensure_crumb(client, "Crumb is not set after ensuring credentials").await?;
                    let crumb_url = url_with_crumb(base_url.clone(), &crumb);

                    match fetch_text_auth_attempt(client, crumb_url, config, &build_request, true)
                        .await?
                    {
                        AuthAttempt::Success { body, url } => Ok((body, url)),
                        AuthAttempt::Status { status, url } if !is_auth_status(status) => {
                            Err(status_error_code(status, &url))
                        }
                        AuthAttempt::Status { .. } | AuthAttempt::InvalidCrumb => {
                            retry_with_fresh_crumb(client, base_url, config, &build_request).await
                        }
                    }
                }
            }
        }
        AuthMode::RequiredCrumb => {
            let crumb = ensure_crumb(client, "Crumb is not set").await?;
            let crumb_url = url_with_crumb(base_url.clone(), &crumb);

            match fetch_text_auth_attempt(client, crumb_url, config, &build_request, true).await? {
                AuthAttempt::Success { body, url } => Ok((body, url)),
                AuthAttempt::Status { status, url } if !is_auth_status(status) => {
                    Err(status_error_code(status, &url))
                }
                AuthAttempt::Status { .. } | AuthAttempt::InvalidCrumb => {
                    retry_with_fresh_crumb(client, base_url, config, &build_request).await
                }
            }
        }
    }
}

async fn fetch_text_auth_attempt<F>(
    client: &YfClient,
    url: Url,
    config: AuthFetchConfig<'_>,
    build_request: &F,
    detect_invalid_crumb_body: bool,
) -> Result<AuthAttempt, YfError>
where
    F: Fn(Url) -> reqwest::RequestBuilder + Send + Sync,
{
    if config.cache_mode == CacheMode::Use
        && let Some(body) = client.cache_get(&url).await
    {
        if should_retry_invalid_crumb_body(config, detect_invalid_crumb_body, &body) {
            return Ok(AuthAttempt::InvalidCrumb);
        }

        return Ok(AuthAttempt::Success { body, url });
    }

    let req = client.with_auth_cookie(build_request(url.clone())).await;
    let resp = client.send_with_retry(req, config.retry_override).await?;

    if !resp.status().is_success() {
        return Ok(AuthAttempt::Status {
            status: resp.status().as_u16(),
            url,
        });
    }

    let body =
        get_success_text(resp, &url, config.endpoint, config.fixture_key, config.ext).await?;

    if should_retry_invalid_crumb_body(config, detect_invalid_crumb_body, &body) {
        return Ok(AuthAttempt::InvalidCrumb);
    }

    if config.cache_mode != CacheMode::Bypass {
        client.cache_put(&url, &body, None).await;
    }

    Ok(AuthAttempt::Success { body, url })
}

async fn retry_with_fresh_crumb<F>(
    client: &YfClient,
    base_url: Url,
    config: AuthFetchConfig<'_>,
    build_request: &F,
) -> Result<(String, Url), YfError>
where
    F: Fn(Url) -> reqwest::RequestBuilder + Send + Sync,
{
    client.clear_crumb().await;
    let crumb = ensure_crumb(client, "Crumb is not set after refreshing credentials").await?;
    let crumb_url = url_with_crumb(base_url, &crumb);

    match fetch_text_auth_attempt(client, crumb_url, config, build_request, false).await? {
        AuthAttempt::Success { body, url } => Ok((body, url)),
        AuthAttempt::Status { status, url } => Err(status_error_code(status, &url)),
        AuthAttempt::InvalidCrumb => unreachable!("invalid crumb body detection is disabled"),
    }
}

async fn ensure_crumb(client: &YfClient, missing_message: &str) -> Result<String, YfError> {
    client.ensure_credentials().await?;
    client
        .crumb()
        .await
        .ok_or_else(|| YfError::Auth(missing_message.into()))
}

fn url_with_crumb(mut url: Url, crumb: &str) -> Url {
    url.query_pairs_mut().append_pair("crumb", crumb);
    url
}

fn should_retry_invalid_crumb_body(
    config: AuthFetchConfig<'_>,
    detect_invalid_crumb_body: bool,
    body: &str,
) -> bool {
    config.retry_on_invalid_crumb_body
        && detect_invalid_crumb_body
        && body.to_ascii_lowercase().contains("invalid crumb")
}

const fn is_auth_status(status: u16) -> bool {
    status == 401 || status == 403
}
