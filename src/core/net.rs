#[cfg(feature = "test-mode")]
use std::env;

use serde::de::DeserializeOwned;
use url::Url;

use crate::core::{
    CallOptions, YfClient, YfError,
    client::{CacheEndpoint, CacheMode, RetryConfig},
    redaction::RedactedUrl,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMode {
    OptionalCrumb,
    RequiredCrumb,
}

#[derive(Clone, Copy, Debug)]
pub struct AuthFetchConfig<'a> {
    pub auth_mode: AuthMode,
    pub cache_endpoint: CacheEndpoint,
    pub options: &'a CallOptions,
    pub cache_body: Option<&'a str>,
    pub endpoint: &'a str,
    pub fixture_key: &'a str,
    pub ext: &'a str,
    pub retry_on_invalid_crumb_body: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CacheFetchConfig<'a> {
    pub cache_endpoint: CacheEndpoint,
    pub options: &'a CallOptions,
    pub endpoint: &'a str,
    pub fixture_key: &'a str,
    pub ext: &'a str,
}

enum AuthAttempt {
    Success { body: String, url: Url },
    Status { status: u16, url: Url },
    InvalidCrumb,
}

enum CachedAuthAttempt {
    Success { body: String, url: Url },
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
            crate::core::logging::trace_warn!(
                symbol,
                error = %e,
                "failed to write fixture"
            );
        }
    }

    Ok(text)
}

#[must_use]
pub fn status_error_code(status: u16, url: &Url) -> YfError {
    let url = RedactedUrl::new(url).to_string();
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
        .map_err(YfError::from)
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

pub async fn fetch_text_cached(
    client: &YfClient,
    url: &Url,
    config: CacheFetchConfig<'_>,
) -> Result<String, YfError> {
    if config.options.cache_mode().reads(config.cache_endpoint)
        && let Some(text) = client.cache_get(url).await
    {
        return Ok(text);
    }

    let text = fetch_text(
        client,
        client.http().get(url.clone()),
        url,
        config.options.retry_override(),
        config.endpoint,
        config.fixture_key,
        config.ext,
    )
    .await?;

    if config.options.cache_mode().writes(config.cache_endpoint) {
        client
            .cache_put(config.cache_endpoint, url, &text, None)
            .await;
    }

    Ok(text)
}

pub async fn fetch_json_post_cached<T>(
    client: &YfClient,
    url: &Url,
    body_json: &str,
    config: CacheFetchConfig<'_>,
) -> Result<T, YfError>
where
    T: DeserializeOwned,
{
    let cache_key = YfClient::post_cache_key(url, body_json);
    if config.options.cache_mode().reads(config.cache_endpoint)
        && let Some(body) = client.cache_get_key(&cache_key).await
    {
        return serde_json::from_str(&body).map_err(YfError::Json);
    }

    let req = client
        .http()
        .post(url.clone())
        .header("content-type", "application/json")
        .body(body_json.to_string());
    let body = fetch_text(
        client,
        req,
        url,
        config.options.retry_override(),
        config.endpoint,
        config.fixture_key,
        config.ext,
    )
    .await?;

    if config.options.cache_mode().writes(config.cache_endpoint) {
        client
            .cache_put_key(config.cache_endpoint, cache_key, &body, None)
            .await;
    }

    serde_json::from_str(&body).map_err(YfError::Json)
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
    let cache_key = config.cache_body.map_or_else(
        || client_cache_key(&base_url),
        |body| YfClient::post_cache_key(&base_url, body),
    );

    match config.auth_mode {
        AuthMode::OptionalCrumb => {
            match fetch_text_auth_attempt(
                client,
                base_url.clone(),
                &cache_key,
                config,
                &build_request,
                true,
            )
            .await?
            {
                AuthAttempt::Success { body, url } => Ok((body, url)),
                AuthAttempt::Status { status, url } if !is_auth_status(status) => {
                    Err(status_error_code(status, &url))
                }
                AuthAttempt::InvalidCrumb => {
                    retry_with_fresh_crumb(client, base_url, &cache_key, config, &build_request)
                        .await
                }
                AuthAttempt::Status { .. } => {
                    let crumb =
                        ensure_crumb(client, "Crumb is not set after ensuring credentials").await?;
                    let crumb_url = url_with_crumb(base_url.clone(), &crumb);

                    match fetch_text_auth_attempt(
                        client,
                        crumb_url,
                        &cache_key,
                        config,
                        &build_request,
                        true,
                    )
                    .await?
                    {
                        AuthAttempt::Success { body, url } => Ok((body, url)),
                        AuthAttempt::Status { status, url } if !is_auth_status(status) => {
                            Err(status_error_code(status, &url))
                        }
                        AuthAttempt::Status { .. } | AuthAttempt::InvalidCrumb => {
                            retry_with_fresh_crumb(
                                client,
                                base_url,
                                &cache_key,
                                config,
                                &build_request,
                            )
                            .await
                        }
                    }
                }
            }
        }
        AuthMode::RequiredCrumb => {
            if let Some(attempt) =
                read_cached_auth_attempt(client, base_url.clone(), &cache_key, config, true).await
            {
                return match attempt {
                    CachedAuthAttempt::Success { body, url } => Ok((body, url)),
                    CachedAuthAttempt::InvalidCrumb => {
                        retry_with_fresh_crumb(client, base_url, &cache_key, config, &build_request)
                            .await
                    }
                };
            }

            let crumb = ensure_crumb(client, "Crumb is not set").await?;
            let crumb_url = url_with_crumb(base_url.clone(), &crumb);

            match fetch_text_auth_attempt(
                client,
                crumb_url,
                &cache_key,
                config,
                &build_request,
                true,
            )
            .await?
            {
                AuthAttempt::Success { body, url } => Ok((body, url)),
                AuthAttempt::Status { status, url } if !is_auth_status(status) => {
                    Err(status_error_code(status, &url))
                }
                AuthAttempt::Status { .. } | AuthAttempt::InvalidCrumb => {
                    retry_with_fresh_crumb(client, base_url, &cache_key, config, &build_request)
                        .await
                }
            }
        }
    }
}

async fn fetch_text_auth_attempt<F>(
    client: &YfClient,
    url: Url,
    cache_key: &str,
    config: AuthFetchConfig<'_>,
    build_request: &F,
    detect_invalid_crumb_body: bool,
) -> Result<AuthAttempt, YfError>
where
    F: Fn(Url) -> reqwest::RequestBuilder + Send + Sync,
{
    if let Some(attempt) = read_cached_auth_attempt(
        client,
        url.clone(),
        cache_key,
        config,
        detect_invalid_crumb_body,
    )
    .await
    {
        return Ok(match attempt {
            CachedAuthAttempt::Success { body, url } => AuthAttempt::Success { body, url },
            CachedAuthAttempt::InvalidCrumb => AuthAttempt::InvalidCrumb,
        });
    }

    let req = client.with_auth_cookie(build_request(url.clone())).await;
    let resp = client
        .send_with_retry(req, config.options.retry_override())
        .await?;

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

    if config.options.cache_mode().writes(config.cache_endpoint) {
        client
            .cache_put_key(config.cache_endpoint, cache_key.to_string(), &body, None)
            .await;
    }

    Ok(AuthAttempt::Success { body, url })
}

async fn read_cached_auth_attempt(
    client: &YfClient,
    url: Url,
    cache_key: &str,
    config: AuthFetchConfig<'_>,
    detect_invalid_crumb_body: bool,
) -> Option<CachedAuthAttempt> {
    if !config.options.cache_mode().reads(config.cache_endpoint) {
        return None;
    }

    let body = client.cache_get_key(cache_key).await?;
    if should_retry_invalid_crumb_body(config, detect_invalid_crumb_body, &body) {
        client.cache_remove_key(cache_key).await;
        return Some(CachedAuthAttempt::InvalidCrumb);
    }

    Some(CachedAuthAttempt::Success { body, url })
}

async fn retry_with_fresh_crumb<F>(
    client: &YfClient,
    base_url: Url,
    cache_key: &str,
    config: AuthFetchConfig<'_>,
    build_request: &F,
) -> Result<(String, Url), YfError>
where
    F: Fn(Url) -> reqwest::RequestBuilder + Send + Sync,
{
    client.cache_remove_key(cache_key).await;
    client.clear_crumb().await;
    let crumb = ensure_crumb(client, "Crumb is not set after refreshing credentials").await?;
    let crumb_url = url_with_crumb(base_url, &crumb);
    let retry_options = config
        .options
        .clone()
        .with_cache_mode(retry_cache_mode(config));
    let retry_config = AuthFetchConfig {
        options: &retry_options,
        ..config
    };

    match fetch_text_auth_attempt(
        client,
        crumb_url,
        cache_key,
        retry_config,
        build_request,
        true,
    )
    .await?
    {
        AuthAttempt::Success { body, url } => Ok((body, url)),
        AuthAttempt::Status { status, url } => Err(status_error_code(status, &url)),
        AuthAttempt::InvalidCrumb => Err(YfError::Auth(
            "Yahoo returned an invalid crumb response after refreshing credentials".into(),
        )),
    }
}

const fn retry_cache_mode(config: AuthFetchConfig<'_>) -> CacheMode {
    if config.options.cache_mode().writes(config.cache_endpoint) {
        CacheMode::Refresh
    } else {
        CacheMode::Bypass
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

fn client_cache_key(url: &Url) -> String {
    url.as_str().to_string()
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use httpmock::{Method::GET, Mock, MockServer};

    use super::*;

    const INVALID_CRUMB_BODY: &str =
        r#"{"quoteResponse":{"result":null,"error":{"description":"Invalid Crumb"}}}"#;
    const OK_BODY: &str = r#"{"quoteResponse":{"result":[],"error":null}}"#;
    static QUOTE_AUTH_OPTIONS: CallOptions = CallOptions::new().with_cache_mode(CacheMode::Use);

    #[must_use]
    fn server_url(server: &MockServer, path_and_query: &str) -> Url {
        Url::parse(&format!("{}{}", server.base_url(), path_and_query)).expect("valid mock URL")
    }

    #[must_use]
    fn cached_client(server: &MockServer) -> YfClient {
        YfClient::builder()
            .cookie_url(server_url(server, "/consent"))
            .crumb_url(server_url(server, "/v1/test/getcrumb"))
            .cache_ttl(Duration::from_mins(1))
            .build()
            .expect("client builds")
    }

    #[must_use]
    fn quote_auth_config() -> AuthFetchConfig<'static> {
        AuthFetchConfig {
            auth_mode: AuthMode::OptionalCrumb,
            cache_endpoint: CacheEndpoint::Quote,
            options: &QUOTE_AUTH_OPTIONS,
            cache_body: None,
            endpoint: "quote_v7",
            fixture_key: "AAPL",
            ext: "json",
            retry_on_invalid_crumb_body: true,
        }
    }

    #[must_use]
    fn required_quote_summary_auth_config() -> AuthFetchConfig<'static> {
        AuthFetchConfig {
            auth_mode: AuthMode::RequiredCrumb,
            cache_endpoint: CacheEndpoint::QuoteSummary,
            options: &QUOTE_AUTH_OPTIONS,
            cache_body: None,
            endpoint: "quote_summary",
            fixture_key: "AAPL",
            ext: "json",
            retry_on_invalid_crumb_body: true,
        }
    }

    #[must_use]
    fn mock_credentials(server: &'_ MockServer) -> (Mock<'_>, Mock<'_>) {
        let cookie = server.mock(|when, then| {
            when.method(GET).path("/consent");
            then.status(200).header("set-cookie", "A=B; Path=/");
        });
        let crumb = server.mock(|when, then| {
            when.method(GET).path("/v1/test/getcrumb");
            then.status(200).body("fresh-crumb");
        });
        (cookie, crumb)
    }

    #[tokio::test]
    async fn required_crumb_cache_hit_does_not_fetch_credentials() {
        let server = MockServer::start();
        let (cookie, crumb) = mock_credentials(&server);
        let client = cached_client(&server);
        let base_url = server_url(
            &server,
            "/v10/finance/quoteSummary/AAPL?modules=summaryDetail",
        );
        let cache_key = base_url.as_str().to_string();
        client
            .cache_put_key(CacheEndpoint::QuoteSummary, cache_key, OK_BODY, None)
            .await;

        let (body, used_url) = fetch_text_with_auth_retry(
            &client,
            base_url.clone(),
            required_quote_summary_auth_config(),
            |url| client.http().get(url),
        )
        .await
        .expect("cached quoteSummary response succeeds");

        assert_eq!(body, OK_BODY);
        assert_eq!(used_url, base_url);
        assert_eq!(cookie.calls(), 0);
        assert_eq!(crumb.calls(), 0);
    }

    #[tokio::test]
    async fn cached_invalid_crumb_body_is_evicted_before_fresh_crumb_retry() {
        let server = MockServer::start();
        let (_cookie, _crumb) = mock_credentials(&server);
        let api = server.mock(|when, then| {
            when.method(GET)
                .path("/v7/finance/quote")
                .query_param("symbols", "AAPL")
                .query_param("crumb", "fresh-crumb");
            then.status(200)
                .header("content-type", "application/json")
                .body(OK_BODY);
        });

        let client = cached_client(&server);
        let base_url = server_url(&server, "/v7/finance/quote?symbols=AAPL");
        let cache_key = base_url.as_str().to_string();
        client
            .cache_put_key(
                CacheEndpoint::Quote,
                cache_key.clone(),
                INVALID_CRUMB_BODY,
                None,
            )
            .await;

        let (body, used_url) =
            fetch_text_with_auth_retry(&client, base_url, quote_auth_config(), |url| {
                client.http().get(url)
            })
            .await
            .expect("fresh crumb retry succeeds");

        assert_eq!(body, OK_BODY);
        assert!(
            used_url
                .query_pairs()
                .any(|(key, value)| { key == "crumb" && value == "fresh-crumb" })
        );
        assert_eq!(
            client.cache_get_key(&cache_key).await.as_deref(),
            Some(OK_BODY)
        );
        api.assert();
    }

    #[tokio::test]
    async fn invalid_crumb_body_after_refresh_returns_auth_error() {
        let server = MockServer::start();
        let (_cookie, _crumb) = mock_credentials(&server);
        let api = server.mock(|when, then| {
            when.method(GET)
                .path("/v7/finance/quote")
                .query_param("symbols", "AAPL")
                .query_param("crumb", "fresh-crumb");
            then.status(200)
                .header("content-type", "application/json")
                .body(INVALID_CRUMB_BODY);
        });

        let client = cached_client(&server);
        let base_url = server_url(&server, "/v7/finance/quote?symbols=AAPL");
        let cache_key = base_url.as_str().to_string();
        let build_request = |url| client.http().get(url);

        let err = retry_with_fresh_crumb(
            &client,
            base_url,
            &cache_key,
            quote_auth_config(),
            &build_request,
        )
        .await
        .expect_err("invalid crumb body is an auth error");

        match err {
            YfError::Auth(message) => assert!(
                message.contains("invalid crumb response after refreshing credentials"),
                "unexpected auth error: {message}",
            ),
            other => panic!("expected auth error, got {other:?}"),
        }
        assert!(client.cache_get_key(&cache_key).await.is_none());
        api.assert();
    }
}
