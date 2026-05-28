//! Public client surface + builder.
//! Internals are split into `auth` (cookie/crumb) and `constants` (UA + defaults).

mod auth;
mod constants;
mod retry;
mod urls;

use crate::core::YfError;
use crate::core::client::constants::DEFAULT_BASE_INSIDER_SEARCH;
use crate::core::currency_resolver::{CurrencyCacheKey, CurrencyHints, ResolvedCurrency};
pub use retry::{Backoff, CacheEndpoint, CacheMode, RetryConfig};
pub(crate) use urls::SymbolEndpoint;

use constants::{
    DEFAULT_BASE_CHART, DEFAULT_BASE_QUOTE, DEFAULT_BASE_QUOTE_API, DEFAULT_COOKIE_URL,
    DEFAULT_CRUMB_URL, USER_AGENT,
};
use reqwest::Client;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use url::Url;

const DEFAULT_CACHE_MAX_ENTRIES: usize = 1024;

/// Defines the preferred data source for profile lookups when testing.
///
/// This enum is always available for API compatibility, but only has effect when
/// the `test-mode` feature is enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiPreference {
    /// Try the API first, then fall back to scraping if the API fails. (Default)
    ApiThenScrape,
    /// Use only the `quoteSummary` API.
    ApiOnly,
    /// Use only the HTML scraping method.
    ScrapeOnly,
}

#[derive(Debug)]
struct CacheEntry {
    body: String,
    expires_at: Instant,
    last_access: u64,
}

#[derive(Debug)]
struct CacheStore {
    map: RwLock<HashMap<String, CacheEntry>>,
    default_ttl: Option<Duration>,
    endpoint_ttls: HashMap<CacheEndpoint, Duration>,
    max_entries: usize,
    access_counter: AtomicU64,
}

impl CacheStore {
    fn next_access(&self) -> u64 {
        self.access_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn ttl_for(&self, endpoint: CacheEndpoint, ttl_override: Option<Duration>) -> Option<Duration> {
        ttl_override
            .or_else(|| self.endpoint_ttls.get(&endpoint).copied())
            .or(self.default_ttl)
    }
}

fn retain_unexpired_entries(map: &mut HashMap<String, CacheEntry>, now: Instant) {
    map.retain(|_, entry| now <= entry.expires_at);
}

fn evict_lru_entries(map: &mut HashMap<String, CacheEntry>, max_entries: usize) {
    while map.len() > max_entries {
        let Some(key) = map
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        map.remove(&key);
    }
}

fn url_cache_key(url: &Url) -> String {
    url.as_str().to_string()
}

fn post_cache_key(url: &Url, body: &str) -> String {
    format!("POST {}\n{body}", url.as_str())
}

fn cache_key_url(key: &str) -> &str {
    if let Some(rest) = key.strip_prefix("POST ")
        && let Some((url, _)) = rest.split_once('\n')
    {
        return url;
    }

    key
}

fn cache_key_matches_url(key: &str, url: &Url) -> bool {
    cache_key_url(key) == url.as_str()
}

fn cache_key_has_crumb_query(key: &str) -> bool {
    Url::parse(cache_key_url(key))
        .is_ok_and(|url| url.query_pairs().any(|(name, _)| name == "crumb"))
}

#[derive(Debug, Default)]
struct ClientState {
    cookie: Option<String>,
    crumb: Option<String>,
}

/// The main asynchronous client for interacting with the Yahoo Finance API.
///
/// The client manages an HTTP client, authentication (cookies and crumbs),
/// caching, and retry logic. It is cloneable and designed to be shared
/// across multiple tasks.
///
/// Create a client using [`YfClient::builder()`] or [`YfClient::default()`].
#[derive(Debug, Clone)]
pub struct YfClient {
    http: Client,
    base_chart: Url,
    base_quote: Url,
    base_quote_api: Url,
    base_quote_v7: Url,
    base_options_v7: Url,
    base_stream: Url,
    base_news: Url,
    base_insider_search: Url,
    base_timeseries: Url,
    cookie_url: Url,
    crumb_url: Url,
    user_agent: String,

    state: Arc<RwLock<ClientState>>,
    credential_fetch_lock: Arc<tokio::sync::Mutex<()>>,

    #[cfg(feature = "test-mode")]
    api_preference: ApiPreference,

    retry: RetryConfig,
    pub(crate) currency_cache: Arc<RwLock<HashMap<CurrencyCacheKey, ResolvedCurrency>>>,
    pub(crate) currency_hints: Arc<RwLock<HashMap<String, CurrencyHints>>>,
    // Cache of resolved instruments by original ticker string
    instrument_cache: Arc<RwLock<HashMap<String, paft::domain::Instrument>>>,
    cache: Option<Arc<CacheStore>>,
}

impl Default for YfClient {
    fn default() -> Self {
        Self::builder().build().expect("default client")
    }
}

impl YfClient {
    /// Creates a new builder for a `YfClient`.
    #[must_use]
    pub fn builder() -> YfClientBuilder {
        YfClientBuilder::default()
    }

    /* -------- internal getters used by other modules -------- */

    pub(crate) const fn http(&self) -> &Client {
        &self.http
    }

    pub(crate) fn user_agent(&self) -> &str {
        &self.user_agent
    }

    pub(crate) const fn base_quote_v7(&self) -> &Url {
        &self.base_quote_v7
    }

    pub(crate) const fn base_stream(&self) -> &Url {
        &self.base_stream
    }

    pub(crate) const fn base_news(&self) -> &Url {
        &self.base_news
    }

    pub(crate) const fn base_insider_search(&self) -> &Url {
        &self.base_insider_search
    }

    #[cfg(feature = "test-mode")]
    pub(crate) const fn api_preference(&self) -> ApiPreference {
        self.api_preference
    }

    /// Returns `true` if in-memory caching is enabled for this client.
    #[must_use]
    pub const fn cache_enabled(&self) -> bool {
        self.cache.is_some()
    }

    pub(crate) async fn cache_get(&self, url: &Url) -> Option<String> {
        self.cache_get_key(&url_cache_key(url)).await
    }

    pub(crate) async fn cache_get_key(&self, key: &str) -> Option<String> {
        let store = self.cache.as_ref()?;
        let now = Instant::now();
        let mut guard = store.map.write().await;
        retain_unexpired_entries(&mut guard, now);
        let entry = guard.get_mut(key)?;
        entry.last_access = store.next_access();
        let body = entry.body.clone();
        drop(guard);
        Some(body)
    }

    pub(crate) async fn cache_put(
        &self,
        endpoint: CacheEndpoint,
        url: &Url,
        body: &str,
        ttl_override: Option<Duration>,
    ) {
        self.cache_put_key(endpoint, url_cache_key(url), body, ttl_override)
            .await;
    }

    pub(crate) async fn cache_put_key(
        &self,
        endpoint: CacheEndpoint,
        key: String,
        body: &str,
        ttl_override: Option<Duration>,
    ) {
        let store = match &self.cache {
            Some(s) => s.clone(),
            None => return,
        };
        let Some(ttl) = store.ttl_for(endpoint, ttl_override) else {
            return;
        };
        let now = Instant::now();
        let expires_at = now + ttl;
        let entry = CacheEntry {
            body: body.to_string(),
            expires_at,
            last_access: store.next_access(),
        };
        let max_entries = store.max_entries;
        let mut guard = store.map.write().await;
        retain_unexpired_entries(&mut guard, now);
        guard.insert(key, entry);
        evict_lru_entries(&mut guard, max_entries);
        drop(guard);
    }

    pub(crate) async fn cache_remove_key(&self, key: &str) {
        if let Some(store) = &self.cache {
            store.map.write().await.remove(key);
        }
    }

    pub(crate) fn post_cache_key(url: &Url, body: &str) -> String {
        post_cache_key(url, body)
    }

    pub(crate) async fn clear_crumb_cache_entries(&self) {
        if let Some(store) = &self.cache {
            let mut guard = store.map.write().await;
            guard.retain(|key, _| !cache_key_has_crumb_query(key));
        }
    }

    // -------- instrument cache (async) --------
    pub(crate) async fn cached_instrument(&self, key: &str) -> Option<paft::domain::Instrument> {
        let guard = self.instrument_cache.read().await;
        guard.get(key).cloned()
    }

    pub(crate) async fn store_instrument(&self, key: String, inst: paft::domain::Instrument) {
        let mut guard = self.instrument_cache.write().await;
        guard.insert(key, inst);
    }

    /// Clears the entire in-memory cache.
    ///
    /// This is an asynchronous operation that acquires write locks on the URL,
    /// currency, and instrument caches. Currency and instrument caches are cleared
    /// even when URL response caching is disabled.
    pub async fn clear_cache(&self) {
        if let Some(store) = &self.cache {
            let mut guard = store.map.write().await;
            guard.clear();
        }
        self.currency_cache.write().await.clear();
        self.currency_hints.write().await.clear();
        self.instrument_cache.write().await.clear();
    }

    /// Removes a specific URL-based entry from the in-memory cache.
    ///
    /// This is useful if you know that the data for a specific request has become stale.
    /// It does nothing if caching is disabled for the client.
    pub async fn invalidate_cache_entry(&self, url: &Url) {
        if let Some(store) = &self.cache {
            let mut guard = store.map.write().await;
            guard.retain(|key, _| !cache_key_matches_url(key, url));
        }
    }

    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(
            skip(self, req, override_retry),
            err,
            fields(
                url = %{
                    req.try_clone()
                        .and_then(|builder| builder.build().ok())
                        .map_or_else(
                            || "<uncloneable>".to_string(),
                            |request| crate::core::redaction::RedactedUrl::new(request.url()).to_string(),
                        )
                }
            )
        )
    )]
    pub(crate) async fn send_with_retry(
        &self,
        mut req: reqwest::RequestBuilder,
        override_retry: Option<&RetryConfig>,
    ) -> Result<reqwest::Response, YfError> {
        // Always set User-Agent header explicitly
        req = req.header("User-Agent", &self.user_agent);

        let cfg = override_retry.unwrap_or(&self.retry);
        if !cfg.enabled {
            return Ok(req.send().await?);
        }

        let mut attempt = 0u32;
        loop {
            let Some(cloned_req) = req.try_clone() else {
                return Err(YfError::RequestNotCloneable);
            };
            let response = cloned_req.send().await;

            match response {
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    if cfg.retry_on_status.contains(&code) && attempt < cfg.max_retries {
                        #[cfg(feature = "tracing")]
                        {
                            let backoff = compute_backoff_duration(&cfg.backoff, attempt);
                            tracing::event!(
                                tracing::Level::INFO,
                                attempt,
                                backoff_ms = backoff.as_secs_f64() * 1000.0,
                                status = code,
                                "retrying after status"
                            );
                            tokio::time::sleep(backoff).await;
                        }
                        #[cfg(not(feature = "tracing"))]
                        {
                            sleep_backoff(&cfg.backoff, attempt).await;
                        }
                        attempt += 1;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    let should_retry = (cfg.retry_on_timeout && e.is_timeout())
                        || (cfg.retry_on_connect && e.is_connect());

                    if should_retry && attempt < cfg.max_retries {
                        #[cfg(feature = "tracing")]
                        {
                            let backoff = compute_backoff_duration(&cfg.backoff, attempt);
                            tracing::event!(
                                tracing::Level::INFO,
                                attempt,
                                backoff_ms = backoff.as_secs_f64() * 1000.0,
                                error = %crate::core::redaction::RedactedDisplay::new(&e),
                                timeout = e.is_timeout(),
                                connect = e.is_connect(),
                                "retrying after error"
                            );
                            tokio::time::sleep(backoff).await;
                        }
                        #[cfg(not(feature = "tracing"))]
                        {
                            sleep_backoff(&cfg.backoff, attempt).await;
                        }
                        attempt += 1;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }

    /// Returns a reference to the default `RetryConfig` for this client.
    ///
    /// This config is used for all requests unless overridden on a per-call basis.
    #[must_use]
    pub const fn retry_config(&self) -> &RetryConfig {
        &self.retry
    }
}

/* ----------------------- Builder ----------------------- */

/// A builder for creating and configuring a [`YfClient`].
#[derive(Default)]
pub struct YfClientBuilder {
    user_agent: Option<String>,
    base_chart: Option<Url>,
    base_quote: Option<Url>,
    base_quote_api: Option<Url>,
    base_quote_v7: Option<Url>,
    base_options_v7: Option<Url>,
    base_stream: Option<Url>,
    base_news: Option<Url>,
    base_insider_search: Option<Url>,
    base_timeseries: Option<Url>,
    cookie_url: Option<Url>,
    crumb_url: Option<Url>,

    #[allow(dead_code)]
    api_preference: Option<ApiPreference>,
    #[allow(dead_code)]
    preauth_cookie: Option<String>,
    #[allow(dead_code)]
    preauth_crumb: Option<String>,

    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    retry: Option<RetryConfig>,
    cache_ttl: Option<Duration>,
    cache_ttls: HashMap<CacheEndpoint, Duration>,
    cache_max_entries: Option<NonZeroUsize>,

    // New fields for custom client and proxy configuration
    custom_client: Option<Client>,
    proxy: Option<reqwest::Proxy>,
}

impl YfClientBuilder {
    /// Sets the `User-Agent` header for all HTTP requests and WebSocket connections.
    ///
    /// The user agent is applied consistently across all request types:
    /// - HTTP requests (quotes, history, fundamentals, etc.)
    /// - WebSocket streaming connections
    /// - Authentication requests (cookies, crumbs)
    ///
    /// Defaults to a common desktop browser User-Agent to avoid being blocked.
    /// This setting is applied per-request rather than at the HTTP client level.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Overrides the base URL for quote HTML pages (used for scraping).
    /// Default: `https://finance.yahoo.com/quote/`.
    #[must_use]
    pub fn base_quote(mut self, url: Url) -> Self {
        self.base_quote = Some(url);
        self
    }

    /// Overrides the base URL for the chart API (used for historical data).
    /// Default: `https://query1.finance.yahoo.com/v8/finance/chart/`.
    #[must_use]
    pub fn base_chart(mut self, url: Url) -> Self {
        self.base_chart = Some(url);
        self
    }

    /// Overrides the base URL for the `quoteSummary` API (used for profiles, financials, etc.).
    /// Default: `https://query1.finance.yahoo.com/v10/finance/quoteSummary/`.
    #[must_use]
    pub fn base_quote_api(mut self, url: Url) -> Self {
        self.base_quote_api = Some(url);
        self
    }

    /// Sets a custom base URL for the v7 quote endpoint.
    ///
    /// This is primarily used for testing or to target a different Yahoo Finance region.
    /// If not set, a default URL (`https://query1.finance.yahoo.com/v7/finance/quote`) is used.
    #[must_use]
    pub fn base_quote_v7(mut self, url: Url) -> Self {
        self.base_quote_v7 = Some(url);
        self
    }

    /// Sets a custom base URL for the v7 options endpoint.
    ///
    /// This is primarily used for testing or to target a different Yahoo Finance region.
    /// If not set, a default URL (`https://query1.finance.yahoo.com/v7/finance/options/`) is used.
    #[must_use]
    pub fn base_options_v7(mut self, url: Url) -> Self {
        self.base_options_v7 = Some(url);
        self
    }

    /// Sets a custom base URL for the streaming API.
    #[must_use]
    pub fn base_stream(mut self, url: Url) -> Self {
        self.base_stream = Some(url);
        self
    }

    /// Sets a custom base URL for the news endpoint.
    /// Default: `https://finance.yahoo.com`.
    #[must_use]
    pub fn base_news(mut self, url: Url) -> Self {
        self.base_news = Some(url);
        self
    }

    /// Sets a custom base URL for the Business Insider search (for ISIN lookup).
    #[must_use]
    pub fn base_insider_search(mut self, url: Url) -> Self {
        self.base_insider_search = Some(url);
        self
    }

    /// Sets a custom base URL for the timeseries endpoint.
    #[must_use]
    pub fn base_timeseries(mut self, url: Url) -> Self {
        self.base_timeseries = Some(url);
        self
    }

    /// Overrides the URL used to acquire an initial cookie.
    #[must_use]
    pub fn cookie_url(mut self, url: Url) -> Self {
        self.cookie_url = Some(url);
        self
    }

    /// Overrides the URL used to acquire a crumb for authenticated requests.
    #[must_use]
    pub fn crumb_url(mut self, url: Url) -> Self {
        self.crumb_url = Some(url);
        self
    }

    /// Sets the entire retry configuration.
    ///
    /// Replaces the default retry settings.
    #[must_use]
    pub fn retry_config(mut self, cfg: RetryConfig) -> Self {
        self.retry = Some(cfg);
        self
    }

    /// A convenience method to enable or disable the retry mechanism.
    #[must_use]
    pub fn retry_enabled(mut self, yes: bool) -> Self {
        let mut cfg = self.retry.unwrap_or_default();
        cfg.enabled = yes;
        self.retry = Some(cfg);
        self
    }

    /// Disables in-memory caching for this client.
    #[must_use]
    pub fn no_cache(mut self) -> Self {
        self.cache_ttl = None;
        self.cache_ttls.clear();
        self
    }

    /// (Internal testing only) Chooses which data source path to use for profile lookups.
    ///
    /// This setting only has effect when the `test-mode` feature is enabled.
    /// In normal usage, this setting is ignored.
    #[doc(hidden)]
    #[must_use]
    #[allow(unused_variables, unused_mut)]
    pub const fn _api_preference(mut self, pref: ApiPreference) -> Self {
        #[cfg(feature = "test-mode")]
        {
            self.api_preference = Some(pref);
        }
        self
    }

    /// (Internal testing only) Provides pre-authenticated credentials to bypass the cookie/crumb fetch.
    ///
    /// This setting only has effect when the `test-mode` feature is enabled.
    /// In normal usage, this setting is ignored.
    #[doc(hidden)]
    #[must_use]
    #[allow(unused_variables, unused_mut)]
    pub fn _preauth(mut self, cookie: impl Into<String>, crumb: impl Into<String>) -> Self {
        #[cfg(feature = "test-mode")]
        {
            self.preauth_cookie = Some(cookie.into());
            self.preauth_crumb = Some(crumb.into());
        }
        self
    }

    /// Sets a global timeout for the entire HTTP request.
    ///
    /// Default: none.
    #[must_use]
    pub const fn timeout(mut self, dur: Duration) -> Self {
        self.timeout = Some(dur);
        self
    }

    /// Sets a timeout for the connection phase of an HTTP request.
    ///
    /// Default: none.
    #[must_use]
    pub const fn connect_timeout(mut self, dur: Duration) -> Self {
        self.connect_timeout = Some(dur);
        self
    }

    /// Enables in-memory caching with a default Time-To-Live (TTL) for all responses.
    ///
    /// If neither this nor [`YfClientBuilder::cache_ttl_for`] is set, response
    /// caching is disabled by default. Endpoint-specific TTLs override this
    /// value.
    #[must_use]
    pub const fn cache_ttl(mut self, dur: Duration) -> Self {
        self.cache_ttl = Some(dur);
        self
    }

    /// Sets a Time-To-Live (TTL) for one response-cache endpoint bucket.
    ///
    /// Calling this enables response caching for that endpoint even when no
    /// global [`YfClientBuilder::cache_ttl`] is configured. Endpoints without a
    /// specific TTL are cached only when a global TTL is configured.
    #[must_use]
    pub fn cache_ttl_for(mut self, endpoint: CacheEndpoint, dur: Duration) -> Self {
        self.cache_ttls.insert(endpoint, dur);
        self
    }

    /// Sets the maximum number of in-memory response-cache entries.
    ///
    /// The cache evicts the least recently used entries after expired entries
    /// have been pruned. The default is 1024 entries.
    #[must_use]
    pub const fn cache_max_entries(mut self, max_entries: NonZeroUsize) -> Self {
        self.cache_max_entries = Some(max_entries);
        self
    }

    /// Sets a custom reqwest client for full control over HTTP configuration.
    ///
    /// This allows you to configure advanced features like custom TLS settings,
    /// connection pooling, or other reqwest-specific options. When this is set,
    /// other HTTP-related builder methods (timeout, `connect_timeout`, proxy) are ignored.
    /// Yahoo authentication cookies are still handled by `YfClient`, so custom
    /// clients do not need `reqwest`'s cookie store enabled.
    ///
    /// # Example
    ///
    /// ```rust
    /// use reqwest::Client;
    /// use yfinance_rs::YfClient;
    ///
    /// let custom_client = Client::builder()
    ///     .timeout(std::time::Duration::from_secs(30))
    ///     .build()
    ///     .unwrap();
    ///
    /// let client = YfClient::builder()
    ///     .custom_client(custom_client)
    ///     .build()
    ///     .unwrap();
    /// ```
    #[must_use]
    pub fn custom_client(mut self, client: Client) -> Self {
        self.custom_client = Some(client);
        self
    }

    /// Sets an HTTP proxy for all requests.
    ///
    /// This is a convenience method for setting up proxy configuration without
    /// needing to create a full custom client. If you need more advanced proxy
    /// configuration, use `custom_client()` instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use yfinance_rs::YfClient;
    ///
    /// let client = YfClient::builder()
    ///     .proxy("http://proxy.example.com:8080")
    ///     .build()
    ///     .unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// This method will panic if the proxy URL is invalid. For production code,
    /// consider using `try_proxy()` instead.
    ///
    /// # Panics
    ///
    /// Panics if the proxy URL format is invalid.
    #[must_use]
    pub fn proxy(mut self, proxy_url: &str) -> Self {
        // Validate URL format before creating proxy
        assert!(
            url::Url::parse(proxy_url).is_ok(),
            "invalid proxy URL format: {proxy_url}"
        );
        self.proxy = Some(reqwest::Proxy::http(proxy_url).expect("invalid proxy URL"));
        self
    }

    /// Sets an HTTP proxy for all requests with error handling.
    ///
    /// This is a convenience method for setting up proxy configuration without
    /// needing to create a full custom client. If you need more advanced proxy
    /// configuration, use `custom_client()` instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use yfinance_rs::YfClient;
    ///
    /// let client = YfClient::builder()
    ///     .try_proxy("http://proxy.example.com:8080")?
    ///     .build()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the proxy URL is invalid.
    pub fn try_proxy(mut self, proxy_url: &str) -> Result<Self, YfError> {
        // Validate URL format first
        url::Url::parse(proxy_url)
            .map_err(|e| YfError::InvalidParams(format!("invalid proxy URL format: {e}")))?;

        let proxy = reqwest::Proxy::http(proxy_url)
            .map_err(|e| YfError::InvalidParams(format!("invalid proxy URL: {e}")))?;
        self.proxy = Some(proxy);
        Ok(self)
    }

    /// Sets an HTTPS proxy for all requests.
    ///
    /// This is a convenience method for setting up HTTPS proxy configuration.
    ///
    /// # Example
    ///
    /// ```rust
    /// use yfinance_rs::YfClient;
    ///
    /// let client = YfClient::builder()
    ///     .https_proxy("https://proxy.example.com:8443")
    ///     .build()
    ///     .unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// This method will panic if the proxy URL is invalid. For production code,
    /// consider using `try_https_proxy()` instead.
    ///
    /// # Panics
    ///
    /// Panics if the proxy URL format is invalid.
    #[must_use]
    pub fn https_proxy(mut self, proxy_url: &str) -> Self {
        // Validate URL format before creating proxy
        assert!(
            url::Url::parse(proxy_url).is_ok(),
            "invalid HTTPS proxy URL format: {proxy_url}"
        );
        self.proxy = Some(reqwest::Proxy::https(proxy_url).expect("invalid HTTPS proxy URL"));
        self
    }

    /// Sets an HTTPS proxy for all requests with error handling.
    ///
    /// This is a convenience method for setting up HTTPS proxy configuration.
    ///
    /// # Example
    ///
    /// ```rust
    /// use yfinance_rs::YfClient;
    ///
    /// let client = YfClient::builder()
    ///     .try_https_proxy("https://proxy.example.com:8443")?
    ///     .build()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the proxy URL is invalid.
    pub fn try_https_proxy(mut self, proxy_url: &str) -> Result<Self, YfError> {
        // Validate URL format first
        url::Url::parse(proxy_url)
            .map_err(|e| YfError::InvalidParams(format!("invalid HTTPS proxy URL format: {e}")))?;

        let proxy = reqwest::Proxy::https(proxy_url)
            .map_err(|e| YfError::InvalidParams(format!("invalid HTTPS proxy URL: {e}")))?;
        self.proxy = Some(proxy);
        Ok(self)
    }

    /// Builds the `YfClient`.
    ///
    /// # Errors
    ///
    /// Returns an error if the base URLs are invalid or the HTTP client fails to build.
    pub fn build(self) -> Result<YfClient, YfError> {
        let base_chart = self.base_chart.unwrap_or(Url::parse(DEFAULT_BASE_CHART)?);
        let base_quote = self.base_quote.unwrap_or(Url::parse(DEFAULT_BASE_QUOTE)?);
        let base_quote_api = self
            .base_quote_api
            .unwrap_or(Url::parse(DEFAULT_BASE_QUOTE_API)?);
        let base_quote_v7 = self
            .base_quote_v7
            .unwrap_or(Url::parse(constants::DEFAULT_BASE_QUOTE_V7)?);
        let base_options_v7 = self
            .base_options_v7
            .unwrap_or(Url::parse(constants::DEFAULT_BASE_OPTIONS_V7)?);
        let base_stream = self
            .base_stream
            .unwrap_or(Url::parse(constants::DEFAULT_BASE_STREAM)?);
        let base_news = self
            .base_news
            .unwrap_or(Url::parse(constants::DEFAULT_BASE_NEWS)?);
        let base_insider_search = self
            .base_insider_search
            .unwrap_or(Url::parse(DEFAULT_BASE_INSIDER_SEARCH)?);
        let base_timeseries = self
            .base_timeseries
            .unwrap_or(Url::parse(constants::DEFAULT_BASE_TIMESERIES)?);

        let cookie_url = self.cookie_url.unwrap_or(Url::parse(DEFAULT_COOKIE_URL)?);
        let crumb_url = self.crumb_url.unwrap_or(Url::parse(DEFAULT_CRUMB_URL)?);

        let user_agent = self.user_agent.as_deref().unwrap_or(USER_AGENT).to_string();

        // Use custom client if provided, otherwise build a new one
        let http = if let Some(custom_client) = self.custom_client {
            custom_client
        } else {
            let mut httpb = reqwest::Client::builder().cookie_store(true);

            if let Some(t) = self.timeout {
                httpb = httpb.timeout(t);
            }
            if let Some(ct) = self.connect_timeout {
                httpb = httpb.connect_timeout(ct);
            }
            if let Some(proxy) = self.proxy {
                httpb = httpb.proxy(proxy);
            }

            httpb.build()?
        };

        let initial_state = ClientState {
            cookie: {
                #[cfg(feature = "test-mode")]
                {
                    self.preauth_cookie
                }
                #[cfg(not(feature = "test-mode"))]
                {
                    None
                }
            },
            crumb: {
                #[cfg(feature = "test-mode")]
                {
                    self.preauth_crumb
                }
                #[cfg(not(feature = "test-mode"))]
                {
                    None
                }
            },
        };

        Ok(YfClient {
            http,
            base_chart,
            base_quote,
            base_quote_api,
            base_quote_v7,
            base_options_v7,
            base_stream,
            base_news,
            base_insider_search,
            base_timeseries,
            cookie_url,
            crumb_url,
            user_agent,
            state: Arc::new(RwLock::new(initial_state)),
            credential_fetch_lock: Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(feature = "test-mode")]
            api_preference: self.api_preference.unwrap_or(ApiPreference::ApiThenScrape),
            retry: self.retry.unwrap_or_default(),
            currency_cache: Arc::new(RwLock::new(HashMap::new())),
            currency_hints: Arc::new(RwLock::new(HashMap::new())),
            instrument_cache: Arc::new(RwLock::new(HashMap::new())),
            cache: (self.cache_ttl.is_some() || !self.cache_ttls.is_empty()).then(|| {
                Arc::new(CacheStore {
                    map: RwLock::new(HashMap::new()),
                    default_ttl: self.cache_ttl,
                    endpoint_ttls: self.cache_ttls,
                    max_entries: self
                        .cache_max_entries
                        .map_or(DEFAULT_CACHE_MAX_ENTRIES, NonZeroUsize::get),
                    access_counter: AtomicU64::new(0),
                })
            }),
        })
    }
}

#[cfg(not(feature = "tracing"))]
async fn sleep_backoff(b: &Backoff, attempt: u32) {
    let dur = compute_backoff_duration(b, attempt);
    tokio::time::sleep(dur).await;
}

#[inline]
fn compute_backoff_duration(b: &Backoff, attempt: u32) -> Duration {
    compute_backoff_duration_with_random(b, attempt, random_u128)
}

fn compute_backoff_duration_with_random(
    b: &Backoff,
    attempt: u32,
    random: impl FnMut() -> Option<u128>,
) -> Duration {
    match *b {
        Backoff::Fixed(d) => d,
        Backoff::Exponential {
            base,
            factor,
            max,
            jitter,
        } => {
            let pow = factor.powi(i32::try_from(attempt).unwrap());
            let mut d = Duration::from_secs_f64(base.as_secs_f64() * pow);
            if d > max {
                d = max;
            }
            if jitter {
                d = jitter_duration(d, max, random);
            }
            d
        }
    }
}

fn random_u128() -> Option<u128> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(u128::from_le_bytes(bytes))
}

fn jitter_duration(d: Duration, max: Duration, random: impl FnMut() -> Option<u128>) -> Duration {
    if d.is_zero() {
        return d;
    }

    let lower = d.as_nanos() / 2;
    let upper = d
        .as_nanos()
        .saturating_add(d.as_nanos() / 2)
        .min(max.as_nanos());
    let Some(nanos) = random_nanos_inclusive(lower, upper, random) else {
        return d;
    };

    duration_from_nanos(nanos)
}

fn random_nanos_inclusive(
    lower: u128,
    upper: u128,
    mut random: impl FnMut() -> Option<u128>,
) -> Option<u128> {
    let width = upper - lower + 1;
    let acceptance_zone = u128::MAX - (u128::MAX % width);

    loop {
        let candidate = random()?;
        if candidate < acceptance_zone {
            return Some(lower + candidate % width);
        }
    }
}

fn duration_from_nanos(nanos: u128) -> Duration {
    const NANOS_PER_SEC: u128 = 1_000_000_000;

    let secs = nanos / NANOS_PER_SEC;
    let subsec_nanos = nanos % NANOS_PER_SEC;

    Duration::new(
        u64::try_from(secs).unwrap_or(u64::MAX),
        u32::try_from(subsec_nanos).expect("subsecond nanoseconds are below one second"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached_client() -> YfClient {
        YfClient::builder()
            .cache_ttl(Duration::from_mins(1))
            .build()
            .expect("client builds")
    }

    fn test_url(url: &str) -> Url {
        Url::parse(url).expect("valid test URL")
    }

    fn expired_at() -> Instant {
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("instant supports recent past")
    }

    async fn insert_cache_entry(client: &YfClient, url: &Url, body: &str, expires_at: Instant) {
        let store = client.cache.as_ref().expect("cache is enabled");
        store.map.write().await.insert(
            url.as_str().to_string(),
            CacheEntry {
                body: body.to_string(),
                expires_at,
                last_access: 0,
            },
        );
    }

    #[tokio::test]
    async fn cache_get_removes_expired_entry() {
        let client = cached_client();
        let url = test_url("https://example.test/data?symbol=AAPL");

        insert_cache_entry(&client, &url, "stale", expired_at()).await;

        assert!(client.cache_get(&url).await.is_none());

        let has_entry = {
            let store = client.cache.as_ref().expect("cache is enabled");
            let guard = store.map.read().await;
            guard.contains_key(url.as_str())
        };
        assert!(!has_entry);
    }

    #[tokio::test]
    async fn cache_put_prunes_expired_entries() {
        let client = cached_client();
        let expired_url = test_url("https://example.test/old?symbol=AAPL");
        let fresh_url = test_url("https://example.test/new?symbol=MSFT");

        insert_cache_entry(&client, &expired_url, "stale", expired_at()).await;
        client
            .cache_put(CacheEndpoint::Chart, &fresh_url, "fresh", None)
            .await;

        let (len, has_expired, has_fresh) = {
            let store = client.cache.as_ref().expect("cache is enabled");
            let guard = store.map.read().await;
            (
                guard.len(),
                guard.contains_key(expired_url.as_str()),
                guard.contains_key(fresh_url.as_str()),
            )
        };
        assert_eq!(len, 1);
        assert!(!has_expired);
        assert!(has_fresh);
    }

    #[tokio::test]
    async fn clear_crumb_removes_crumb_cache_entries() {
        let client = cached_client();
        let plain_url = test_url("https://example.test/v7/finance/quote?symbols=AAPL");
        let crumb_url = test_url("https://example.test/v7/finance/quote?symbols=AAPL&crumb=stale");

        client
            .cache_put(CacheEndpoint::Quote, &plain_url, "plain", None)
            .await;
        client
            .cache_put(CacheEndpoint::Quote, &crumb_url, "with crumb", None)
            .await;
        client.state.write().await.crumb = Some("stale".to_string());

        client.clear_crumb().await;

        let (len, has_plain, has_crumb) = {
            let store = client.cache.as_ref().expect("cache is enabled");
            let guard = store.map.read().await;
            (
                guard.len(),
                guard.contains_key(plain_url.as_str()),
                guard.contains_key(crumb_url.as_str()),
            )
        };
        assert_eq!(len, 1);
        assert!(has_plain);
        assert!(!has_crumb);
        assert!(client.crumb().await.is_none());
    }

    #[tokio::test]
    async fn cache_put_evicts_least_recently_used_entry() {
        let client = YfClient::builder()
            .cache_ttl(Duration::from_mins(1))
            .cache_max_entries(NonZeroUsize::new(2).expect("non-zero"))
            .build()
            .expect("client builds");
        let a = test_url("https://example.test/a");
        let b = test_url("https://example.test/b");
        let c = test_url("https://example.test/c");

        client.cache_put(CacheEndpoint::Chart, &a, "a", None).await;
        client.cache_put(CacheEndpoint::Chart, &b, "b", None).await;
        assert_eq!(client.cache_get(&a).await.as_deref(), Some("a"));
        client.cache_put(CacheEndpoint::Chart, &c, "c", None).await;

        assert_eq!(client.cache_get(&a).await.as_deref(), Some("a"));
        assert!(client.cache_get(&b).await.is_none());
        assert_eq!(client.cache_get(&c).await.as_deref(), Some("c"));
    }

    #[tokio::test]
    async fn endpoint_ttl_enables_only_that_endpoint_without_global_ttl() {
        let client = YfClient::builder()
            .cache_ttl_for(CacheEndpoint::Quote, Duration::from_mins(1))
            .build()
            .expect("client builds");
        let quote = test_url("https://example.test/v7/finance/quote?symbols=AAPL");
        let chart = test_url("https://example.test/v8/finance/chart/AAPL");

        client
            .cache_put(CacheEndpoint::Quote, &quote, "quote", None)
            .await;
        client
            .cache_put(CacheEndpoint::Chart, &chart, "chart", None)
            .await;

        assert_eq!(client.cache_get(&quote).await.as_deref(), Some("quote"));
        assert!(client.cache_get(&chart).await.is_none());
    }

    #[test]
    fn exponential_jitter_uses_random_input() {
        let backoff = Backoff::Exponential {
            base: Duration::from_millis(100),
            factor: 2.0,
            max: Duration::from_secs(1),
            jitter: true,
        };

        let low = compute_backoff_duration_with_random(&backoff, 0, || Some(0));
        let high = compute_backoff_duration_with_random(&backoff, 0, || Some(100_000_000));

        assert_eq!(low, Duration::from_millis(50));
        assert_eq!(high, Duration::from_millis(150));
    }

    #[test]
    fn exponential_jitter_respects_max_delay() {
        let backoff = Backoff::Exponential {
            base: Duration::from_secs(1),
            factor: 2.0,
            max: Duration::from_secs(1),
            jitter: true,
        };

        let delay = compute_backoff_duration_with_random(&backoff, 3, || Some(500_000_000));

        assert_eq!(delay, Duration::from_secs(1));
    }
}
