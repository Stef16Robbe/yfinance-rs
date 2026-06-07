//! Public client surface + builder.
//! Internals are split into `auth` (cookie/crumb) and `constants` (UA + defaults).

mod auth;
mod constants;
mod retry;
pub(crate) mod urls;

use crate::core::YfError;
use crate::core::client::constants::DEFAULT_BASE_INSIDER_SEARCH;
use crate::core::currency_resolver::{CurrencyCacheKey, CurrencyHints, ResolvedCurrency};
pub use retry::{Backoff, CacheEndpoint, CacheMode, RetryConfig};
pub(crate) use urls::{SymbolEndpoint, normalize_symbol, normalize_symbols};

use constants::{
    DEFAULT_BASE_CHART, DEFAULT_BASE_QUOTE_API, DEFAULT_COOKIE_URL, DEFAULT_CRUMB_URL, USER_AGENT,
};
use moka::ops::compute::Op;
use moka::sync::Cache as MokaCache;
use reqwest::Client;
use std::collections::HashMap;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use url::Url;

const DEFAULT_CACHE_MAX_ENTRIES: usize = 1024;
const DEFAULT_SIDE_CACHE_MAX_ENTRIES: usize = 4096;
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HttpTimeouts {
    request: Duration,
    connect: Duration,
}

pub(crate) struct BoundedMokaMap<K, V> {
    entries: MokaCache<K, V>,
}

impl<K, V> std::fmt::Debug for BoundedMokaMap<K, V>
where
    K: Eq + Hash + std::fmt::Debug,
    V: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundedMokaMap").finish_non_exhaustive()
    }
}

impl<K, V> BoundedMokaMap<K, V>
where
    K: Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn new(max_entries: usize) -> Self {
        debug_assert!(max_entries > 0);
        Self {
            entries: MokaCache::builder()
                .max_capacity(max_entries.try_into().unwrap_or(u64::MAX))
                .build(),
        }
    }

    pub(crate) fn insert(&self, key: K, value: V) {
        self.entries.insert(key, value);
        self.entries.run_pending_tasks();
    }

    pub(crate) fn get_cloned(&self, key: &K) -> Option<V> {
        self.entries.get(key)
    }

    pub(crate) fn compute_with(&self, key: K, f: impl FnOnce(Option<V>) -> V) -> V {
        let result = self
            .entries
            .entry(key)
            .and_compute_with(|maybe_entry| Op::Put(f(maybe_entry.map(moka::Entry::into_value))));
        self.entries.run_pending_tasks();
        result
            .into_entry()
            .expect("compute always puts a value")
            .into_value()
    }

    pub(crate) fn clear(&self) {
        self.entries.invalidate_all();
        self.entries.run_pending_tasks();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.run_pending_tasks();
        usize::try_from(self.entries.entry_count()).expect("cache entry count fits usize")
    }
}

impl<V> BoundedMokaMap<String, V>
where
    V: Clone + Send + Sync + 'static,
{
    pub(crate) fn get_str(&self, key: &str) -> Option<V> {
        self.entries.get(key)
    }
}

#[derive(Clone, Debug)]
struct CacheEntry {
    body: Arc<str>,
    expires_at: Instant,
}

struct CacheStore {
    entries: MokaCache<String, CacheEntry>,
    default_ttl: Option<Duration>,
    endpoint_ttls: HashMap<CacheEndpoint, Duration>,
    max_entries: usize,
}

impl std::fmt::Debug for CacheStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheStore")
            .field("default_ttl", &self.default_ttl)
            .field("endpoint_ttls", &self.endpoint_ttls)
            .field("max_entries", &self.max_entries)
            .finish_non_exhaustive()
    }
}

impl CacheStore {
    fn new(
        default_ttl: Option<Duration>,
        endpoint_ttls: HashMap<CacheEndpoint, Duration>,
        max_entries: usize,
    ) -> Self {
        Self {
            entries: MokaCache::builder()
                .max_capacity(max_entries.try_into().unwrap_or(u64::MAX))
                .support_invalidation_closures()
                .build(),
            default_ttl,
            endpoint_ttls,
            max_entries,
        }
    }

    fn ttl_for(&self, endpoint: CacheEndpoint, ttl_override: Option<Duration>) -> Option<Duration> {
        ttl_override
            .or_else(|| self.endpoint_ttls.get(&endpoint).copied())
            .or(self.default_ttl)
    }

    fn get_fresh_shared(&self, key: &str, now: Instant) -> Option<Arc<str>> {
        let entry = self.entries.get(key)?;
        if now > entry.expires_at {
            self.remove(key);
            return None;
        }

        Some(entry.body)
    }

    fn insert(&self, key: String, body: String, expires_at: Instant) {
        self.entries.insert(
            key,
            CacheEntry {
                body: Arc::from(body),
                expires_at,
            },
        );
        self.entries.run_pending_tasks();
    }

    fn remove(&self, key: &str) {
        self.entries.invalidate(key);
        self.entries.run_pending_tasks();
    }

    fn clear(&self) {
        self.entries.invalidate_all();
        self.entries.run_pending_tasks();
    }

    fn remove_url(&self, url: &Url) {
        let target = url.as_str().to_string();
        let _ = self
            .entries
            .invalidate_entries_if(move |key, _| cache_key_matches_url_str(key, &target));
        self.entries.run_pending_tasks();
    }

    fn prune_expired(&self, now: Instant) {
        let _ = self
            .entries
            .invalidate_entries_if(move |_, entry| now > entry.expires_at);
        self.entries.run_pending_tasks();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.run_pending_tasks();
        usize::try_from(self.entries.entry_count()).expect("cache entry count fits usize")
    }

    #[cfg(test)]
    fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
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

fn cache_key_matches_url_str(key: &str, url: &str) -> bool {
    cache_key_url(key) == url
}

#[derive(Default)]
struct ClientState {
    cookie: Option<String>,
    crumb: Option<String>,
}

impl std::fmt::Debug for ClientState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientState")
            .field("cookie_present", &self.cookie.is_some())
            .field("crumb_present", &self.crumb.is_some())
            .finish()
    }
}

/// The main asynchronous client for interacting with the Yahoo Finance API.
///
/// The client manages an HTTP client, authentication (cookies and crumbs),
/// caching, and retry logic. It is cloneable and designed to be shared
/// across multiple tasks.
///
/// Create a client using [`YfClient::builder()`] or [`YfClient::default()`].
#[derive(Clone)]
pub struct YfClient {
    http: Client,
    base_chart: Url,
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

    retry: RetryConfig,
    pub(crate) currency_cache: Arc<BoundedMokaMap<CurrencyCacheKey, ResolvedCurrency>>,
    pub(crate) currency_hints: Arc<BoundedMokaMap<String, CurrencyHints>>,
    // Cache of resolved instruments by original ticker string
    instrument_cache: Arc<BoundedMokaMap<String, paft::domain::Instrument>>,
    cache: Option<Arc<CacheStore>>,
}

impl std::fmt::Debug for YfClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YfClient")
            .field(
                "base_chart",
                &crate::core::redaction::RedactedUrl::new(&self.base_chart),
            )
            .field(
                "base_quote_api",
                &crate::core::redaction::RedactedUrl::new(&self.base_quote_api),
            )
            .field(
                "base_quote_v7",
                &crate::core::redaction::RedactedUrl::new(&self.base_quote_v7),
            )
            .field(
                "base_options_v7",
                &crate::core::redaction::RedactedUrl::new(&self.base_options_v7),
            )
            .field(
                "base_stream",
                &crate::core::redaction::RedactedUrl::new(&self.base_stream),
            )
            .field(
                "base_news",
                &crate::core::redaction::RedactedUrl::new(&self.base_news),
            )
            .field(
                "base_insider_search",
                &crate::core::redaction::RedactedUrl::new(&self.base_insider_search),
            )
            .field(
                "base_timeseries",
                &crate::core::redaction::RedactedUrl::new(&self.base_timeseries),
            )
            .field(
                "cookie_url",
                &crate::core::redaction::RedactedUrl::new(&self.cookie_url),
            )
            .field(
                "crumb_url",
                &crate::core::redaction::RedactedUrl::new(&self.crumb_url),
            )
            .field("user_agent", &self.user_agent)
            .field("retry", &self.retry)
            .field("currency_cache", &self.currency_cache)
            .field("currency_hints", &self.currency_hints)
            .field("instrument_cache", &self.instrument_cache)
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
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

    /// Returns `true` if in-memory caching is enabled for this client.
    #[must_use]
    pub const fn cache_enabled(&self) -> bool {
        self.cache.is_some()
    }

    pub(crate) fn cache_get(&self, url: &Url) -> Option<Arc<str>> {
        self.cache_get_key(&url_cache_key(url))
    }

    pub(crate) fn cache_get_key(&self, key: &str) -> Option<Arc<str>> {
        self.cache.as_ref()?.get_fresh_shared(key, Instant::now())
    }

    pub(crate) fn cache_put(
        &self,
        endpoint: CacheEndpoint,
        url: &Url,
        body: &str,
        ttl_override: Option<Duration>,
    ) {
        self.cache_put_key(endpoint, url_cache_key(url), body, ttl_override);
    }

    pub(crate) fn cache_put_key(
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
        store.prune_expired(now);
        store.insert(key, body.to_string(), now + ttl);
    }

    pub(crate) fn cache_remove_key(&self, key: &str) {
        if let Some(store) = &self.cache {
            store.remove(key);
        }
    }

    pub(crate) fn post_cache_key(url: &Url, body: &str) -> String {
        post_cache_key(url, body)
    }

    // -------- instrument cache --------
    pub(crate) fn cached_instrument(&self, key: &str) -> Option<paft::domain::Instrument> {
        self.instrument_cache.get_str(key)
    }

    pub(crate) fn store_instrument(&self, key: String, inst: paft::domain::Instrument) {
        self.instrument_cache.insert(key, inst);
    }

    /// Clears the entire in-memory cache.
    ///
    /// Currency and instrument caches are cleared even when URL response caching is
    /// disabled.
    pub fn clear_cache(&self) {
        if let Some(store) = &self.cache {
            store.clear();
        }
        self.currency_cache.clear();
        self.currency_hints.clear();
        self.instrument_cache.clear();
    }

    /// Removes a specific URL-based entry from the in-memory cache.
    ///
    /// This is useful if you know that the data for a specific request has become stale.
    /// It does nothing if caching is disabled for the client.
    pub fn invalidate_cache_entry(&self, url: &Url) {
        if let Some(store) = &self.cache {
            store.remove_url(url);
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
        cfg.validate()?;
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
    preauth_cookie: Option<String>,
    #[allow(dead_code)]
    preauth_crumb: Option<String>,

    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    retry: Option<RetryConfig>,
    cache_ttl: Option<Duration>,
    cache_ttls: HashMap<CacheEndpoint, Duration>,
    cache_max_entries: Option<NonZeroUsize>,
    side_cache_max_entries: Option<NonZeroUsize>,

    // New fields for custom client and proxy configuration
    custom_client: Option<Client>,
    proxy: Option<reqwest::Proxy>,
}

impl YfClientBuilder {
    const fn http_timeouts(&self) -> HttpTimeouts {
        HttpTimeouts {
            request: match self.timeout {
                Some(timeout) => timeout,
                None => DEFAULT_REQUEST_TIMEOUT,
            },
            connect: match self.connect_timeout {
                Some(timeout) => timeout,
                None => DEFAULT_CONNECT_TIMEOUT,
            },
        }
    }

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
    /// Default for clients built without [`YfClientBuilder::custom_client`]: 30 seconds.
    #[must_use]
    pub const fn timeout(mut self, dur: Duration) -> Self {
        self.timeout = Some(dur);
        self
    }

    /// Sets a timeout for the connection phase of an HTTP request.
    ///
    /// Default for clients built without [`YfClientBuilder::custom_client`]: 10 seconds.
    #[must_use]
    pub const fn connect_timeout(mut self, dur: Duration) -> Self {
        self.connect_timeout = Some(dur);
        self
    }

    /// Sets the default Time-To-Live (TTL) for cached responses.
    ///
    /// This TTL is used when a call's effective [`CacheMode`] allows cache writes.
    /// [`CacheMode::Default`] still bypasses volatile endpoints such as quotes,
    /// options, news, and screeners; use [`CacheMode::Use`] or [`CacheMode::Refresh`]
    /// on those calls to cache them. If neither this nor
    /// [`YfClientBuilder::cache_ttl_for`] is set, response caching is disabled.
    /// Endpoint-specific TTLs override this value.
    #[must_use]
    pub const fn cache_ttl(mut self, dur: Duration) -> Self {
        self.cache_ttl = Some(dur);
        self
    }

    /// Sets a Time-To-Live (TTL) for one response-cache endpoint bucket.
    ///
    /// This TTL is used when that endpoint is cacheable for a given call. It does not
    /// override the call's effective [`CacheMode`]: volatile endpoints such as quotes,
    /// options, news, and screeners still bypass the cache under [`CacheMode::Default`].
    /// Use [`CacheMode::Use`] or [`CacheMode::Refresh`] on those calls to cache them.
    /// Endpoints without a specific TTL are cached only when a global
    /// [`YfClientBuilder::cache_ttl`] is configured.
    #[must_use]
    pub fn cache_ttl_for(mut self, endpoint: CacheEndpoint, dur: Duration) -> Self {
        self.cache_ttls.insert(endpoint, dur);
        self
    }

    /// Sets the maximum number of in-memory response-cache entries.
    ///
    /// The cache removes requested expired entries on read, prunes other expired
    /// entries on writes, and bounds total entries with Moka's concurrent
    /// admission and eviction policy. The default is 1024 entries.
    #[must_use]
    pub const fn cache_max_entries(mut self, max_entries: NonZeroUsize) -> Self {
        self.cache_max_entries = Some(max_entries);
        self
    }

    /// Sets the maximum number of entries for each internal side cache.
    ///
    /// This limit applies independently to the resolved-currency cache,
    /// currency-hints cache, and instrument cache. The default is 4096 entries
    /// per side cache.
    #[must_use]
    pub const fn side_cache_max_entries(mut self, max_entries: NonZeroUsize) -> Self {
        self.side_cache_max_entries = Some(max_entries);
        self
    }

    /// Sets a custom reqwest client for full control over HTTP configuration.
    ///
    /// This allows you to configure advanced features like custom TLS settings,
    /// connection pooling, proxies, DNS overrides, or other reqwest-specific options.
    /// WebSocket streams use this client for their HTTP upgrade request before
    /// handing the upgraded socket to tungstenite. When this is set, other HTTP-related
    /// builder methods (timeout, `connect_timeout`, proxy) are ignored.
    /// Yahoo authentication cookies are still handled by `YfClient`, so custom
    /// clients do not need `reqwest`'s cookie store enabled. Builder-level
    /// default timeouts are not applied to custom clients.
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

    /// Sets a proxy for all HTTP and HTTPS requests, including WebSocket upgrade requests.
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
        self.proxy = Some(reqwest::Proxy::all(proxy_url).expect("invalid proxy URL"));
        self
    }

    /// Sets a proxy for all HTTP and HTTPS requests, including WebSocket upgrade requests.
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

        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| YfError::InvalidParams(format!("invalid proxy URL: {e}")))?;
        self.proxy = Some(proxy);
        Ok(self)
    }

    /// Sets a proxy for HTTPS requests, including `wss://` WebSocket upgrade requests.
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

    /// Sets a proxy for HTTPS requests, including `wss://` WebSocket upgrade requests.
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
        let timeouts = self.http_timeouts();
        let base_chart = self.base_chart.unwrap_or(Url::parse(DEFAULT_BASE_CHART)?);
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
            // Yahoo auth stores and sends the required cookie explicitly.
            let mut httpb = reqwest::Client::builder();

            httpb = httpb.timeout(timeouts.request);
            httpb = httpb.connect_timeout(timeouts.connect);
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

        let retry = self.retry.unwrap_or_default();
        retry.validate()?;
        let side_cache_max_entries = self
            .side_cache_max_entries
            .map_or(DEFAULT_SIDE_CACHE_MAX_ENTRIES, NonZeroUsize::get);

        Ok(YfClient {
            http,
            base_chart,
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
            retry,
            currency_cache: Arc::new(BoundedMokaMap::new(side_cache_max_entries)),
            currency_hints: Arc::new(BoundedMokaMap::new(side_cache_max_entries)),
            instrument_cache: Arc::new(BoundedMokaMap::new(side_cache_max_entries)),
            cache: (self.cache_ttl.is_some() || !self.cache_ttls.is_empty()).then(|| {
                Arc::new(CacheStore::new(
                    self.cache_ttl,
                    self.cache_ttls,
                    self.cache_max_entries
                        .map_or(DEFAULT_CACHE_MAX_ENTRIES, NonZeroUsize::get),
                ))
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
            let exponent = i32::try_from(attempt).unwrap_or(i32::MAX);
            let raw_secs = base.as_secs_f64() * factor.powi(exponent);
            let secs = if raw_secs.is_finite() {
                raw_secs.min(max.as_secs_f64())
            } else {
                max.as_secs_f64()
            };
            let mut d = Duration::try_from_secs_f64(secs).unwrap_or(max);
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
    use httpmock::{Method::GET, MockServer};

    fn cached_client() -> YfClient {
        YfClient::builder()
            .cache_ttl(Duration::from_mins(1))
            .build()
            .expect("client builds")
    }

    fn test_url(url: &str) -> Url {
        Url::parse(url).expect("valid test URL")
    }

    #[test]
    fn client_state_debug_reports_presence_without_values() {
        let state = ClientState {
            cookie: Some("cookie-secret".to_string()),
            crumb: Some("crumb-secret".to_string()),
        };

        let debug = format!("{state:?}");

        assert!(debug.contains("cookie_present: true"));
        assert!(debug.contains("crumb_present: true"));
        assert!(!debug.contains("cookie-secret"));
        assert!(!debug.contains("crumb-secret"));
    }

    #[test]
    fn yf_client_debug_omits_credentials_and_redacts_auth_query_params() {
        let client = YfClient::builder()
            .cookie_url(test_url(
                "https://example.test/cookie?cookie=cookie-secret&symbols=AAPL",
            ))
            .crumb_url(test_url(
                "https://example.test/getcrumb?crumb=crumb-secret&api_key=key-secret",
            ))
            .build()
            .expect("client builds");

        {
            let mut state = client.state.blocking_write();
            state.cookie = Some("state-cookie-secret".to_string());
            state.crumb = Some("state-crumb-secret".to_string());
        }

        let debug = format!("{client:?}");

        assert!(debug.contains("cookie=REDACTED"));
        assert!(debug.contains("crumb=REDACTED"));
        assert!(debug.contains("api_key=REDACTED"));
        assert!(!debug.contains("cookie-secret"));
        assert!(!debug.contains("crumb-secret"));
        assert!(!debug.contains("key-secret"));
        assert!(!debug.contains("state-cookie-secret"));
        assert!(!debug.contains("state-crumb-secret"));
        assert!(!debug.contains("cookie_present"));
        assert!(!debug.contains("crumb_present"));
    }

    fn expired_at() -> Instant {
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("instant supports recent past")
    }

    #[derive(Debug)]
    struct CloneCountingKey {
        value: String,
        clones: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl CloneCountingKey {
        fn new(value: &str, clones: &Arc<std::sync::atomic::AtomicUsize>) -> Self {
            Self {
                value: value.to_string(),
                clones: Arc::clone(clones),
            }
        }
    }

    impl Clone for CloneCountingKey {
        fn clone(&self) -> Self {
            self.clones
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Self {
                value: self.value.clone(),
                clones: Arc::clone(&self.clones),
            }
        }
    }

    impl PartialEq for CloneCountingKey {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value
        }
    }

    impl Eq for CloneCountingKey {}

    impl Hash for CloneCountingKey {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.value.hash(state);
        }
    }

    #[test]
    fn bounded_moka_map_hit_does_not_clone_keys() {
        let clones = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cache = BoundedMokaMap::new(2);
        cache.insert(CloneCountingKey::new("AAPL", &clones), "aapl");
        cache.insert(CloneCountingKey::new("MSFT", &clones), "msft");

        clones.store(0, std::sync::atomic::Ordering::Relaxed);

        assert_eq!(
            cache.get_cloned(&CloneCountingKey::new("AAPL", &clones)),
            Some("aapl")
        );
        assert_eq!(clones.load(std::sync::atomic::Ordering::Relaxed), 0);

        cache.insert(CloneCountingKey::new("GOOGL", &clones), "googl");

        assert!(cache.len() <= 2);
    }

    #[test]
    fn builder_defaults_to_bounded_http_timeouts() {
        assert_eq!(
            YfClientBuilder::default().http_timeouts(),
            HttpTimeouts {
                request: DEFAULT_REQUEST_TIMEOUT,
                connect: DEFAULT_CONNECT_TIMEOUT,
            }
        );
    }

    #[test]
    fn builder_http_timeouts_are_overridable() {
        let request = Duration::from_secs(7);
        let connect = Duration::from_secs(2);

        assert_eq!(
            YfClientBuilder::default()
                .timeout(request)
                .connect_timeout(connect)
                .http_timeouts(),
            HttpTimeouts { request, connect }
        );
    }

    #[tokio::test]
    async fn builder_default_http_client_does_not_store_ambient_cookies() {
        let server = MockServer::start();
        let set_cookie = server.mock(|when, then| {
            when.method(GET).path("/sets-cookie");
            then.status(200)
                .header("set-cookie", "TRACK=1; Path=/")
                .body("ok");
        });
        let no_cookie = server.mock(|when, then| {
            when.method(GET).path("/no-cookie").header_missing("cookie");
            then.status(200).body("ok");
        });

        let client = YfClient::builder().build().expect("client builds");

        client
            .http
            .get(server.url("/sets-cookie"))
            .send()
            .await
            .expect("set-cookie request succeeds")
            .error_for_status()
            .expect("set-cookie response is successful");
        client
            .http
            .get(server.url("/no-cookie"))
            .send()
            .await
            .expect("follow-up request succeeds")
            .error_for_status()
            .expect("follow-up response is successful");

        set_cookie.assert();
        no_cookie.assert();
    }

    fn insert_cache_entry(client: &YfClient, url: &Url, body: &str, expires_at: Instant) {
        let store = client.cache.as_ref().expect("cache is enabled");
        store.insert(url.as_str().to_string(), body.to_string(), expires_at);
    }

    #[test]
    fn cache_get_removes_expired_entry() {
        let client = cached_client();
        let url = test_url("https://example.test/data?symbol=AAPL");

        insert_cache_entry(&client, &url, "stale", expired_at());

        assert!(client.cache_get(&url).is_none());

        let has_entry = {
            let store = client.cache.as_ref().expect("cache is enabled");
            store.contains_key(url.as_str())
        };
        assert!(!has_entry);
    }

    #[test]
    fn cache_get_does_not_prune_unrelated_expired_entries() {
        let client = cached_client();
        let expired_url = test_url("https://example.test/old?symbol=AAPL");
        let fresh_url = test_url("https://example.test/new?symbol=MSFT");

        insert_cache_entry(&client, &expired_url, "stale", expired_at());
        insert_cache_entry(
            &client,
            &fresh_url,
            "fresh",
            Instant::now() + Duration::from_mins(1),
        );

        assert_eq!(client.cache_get(&fresh_url).as_deref(), Some("fresh"));

        let (has_expired, has_fresh) = {
            let store = client.cache.as_ref().expect("cache is enabled");
            (
                store.contains_key(expired_url.as_str()),
                store.contains_key(fresh_url.as_str()),
            )
        };
        assert!(has_expired);
        assert!(has_fresh);
    }

    #[test]
    fn cache_get_fresh_hits_do_not_require_async_lock() {
        let client = cached_client();
        let url = test_url("https://example.test/hit?symbol=AAPL");

        insert_cache_entry(
            &client,
            &url,
            "fresh",
            Instant::now() + Duration::from_mins(1),
        );

        let hits: Vec<_> = (0..32).map(|_| client.cache_get(&url)).collect();

        assert!(hits.iter().all(|hit| hit.as_deref() == Some("fresh")));
    }

    #[test]
    fn cache_get_reuses_cached_body_allocation() {
        let client = cached_client();
        let url = test_url("https://example.test/large?symbol=AAPL");

        client.cache_put(CacheEndpoint::Chart, &url, "large-body", None);

        let first = client
            .cache_get(&url)
            .expect("first cache hit should return body");
        let second = client
            .cache_get(&url)
            .expect("second cache hit should return body");

        assert_eq!(first.as_ref(), "large-body");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn cache_put_prunes_expired_entries() {
        let client = cached_client();
        let expired_url = test_url("https://example.test/old?symbol=AAPL");
        let fresh_url = test_url("https://example.test/new?symbol=MSFT");

        insert_cache_entry(&client, &expired_url, "stale", expired_at());
        client.cache_put(CacheEndpoint::Chart, &fresh_url, "fresh", None);

        let (len, has_expired, has_fresh) = {
            let store = client.cache.as_ref().expect("cache is enabled");
            (
                store.len(),
                store.contains_key(expired_url.as_str()),
                store.contains_key(fresh_url.as_str()),
            )
        };
        assert_eq!(len, 1);
        assert!(!has_expired);
        assert!(has_fresh);
    }

    #[test]
    fn cache_put_bounds_entry_count() {
        let client = YfClient::builder()
            .cache_ttl(Duration::from_mins(1))
            .cache_max_entries(NonZeroUsize::new(2).expect("non-zero"))
            .build()
            .expect("client builds");
        let a = test_url("https://example.test/a");
        let b = test_url("https://example.test/b");
        let c = test_url("https://example.test/c");

        client.cache_put(CacheEndpoint::Chart, &a, "a", None);
        client.cache_put(CacheEndpoint::Chart, &b, "b", None);
        client.cache_put(CacheEndpoint::Chart, &c, "c", None);

        let store = client.cache.as_ref().expect("cache is enabled");
        assert!(store.len() <= 2);
    }

    fn test_instrument(symbol: &str) -> paft::domain::Instrument {
        paft::domain::Instrument::from_symbol(symbol, paft::domain::AssetKind::Equity)
            .expect("valid test instrument")
    }

    #[test]
    fn instrument_side_cache_bounds_entry_count() {
        let client = YfClient::builder()
            .side_cache_max_entries(NonZeroUsize::new(2).expect("non-zero"))
            .build()
            .expect("client builds");

        client.store_instrument("AAPL".to_string(), test_instrument("AAPL"));
        client.store_instrument("MSFT".to_string(), test_instrument("MSFT"));
        assert!(client.cached_instrument("AAPL").is_some());

        client.store_instrument("GOOGL".to_string(), test_instrument("GOOGL"));

        assert!(client.instrument_cache.len() <= 2);
    }

    #[test]
    fn endpoint_ttl_enables_only_that_endpoint_without_global_ttl() {
        let client = YfClient::builder()
            .cache_ttl_for(CacheEndpoint::Quote, Duration::from_mins(1))
            .build()
            .expect("client builds");
        let quote = test_url("https://example.test/v7/finance/quote?symbols=AAPL");
        let chart = test_url("https://example.test/v8/finance/chart/AAPL");

        client.cache_put(CacheEndpoint::Quote, &quote, "quote", None);
        client.cache_put(CacheEndpoint::Chart, &chart, "chart", None);

        assert_eq!(client.cache_get(&quote).as_deref(), Some("quote"));
        assert!(client.cache_get(&chart).is_none());
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

    #[test]
    fn exponential_backoff_saturates_huge_attempts_at_max_delay() {
        let backoff = Backoff::Exponential {
            base: Duration::from_millis(100),
            factor: 2.0,
            max: Duration::from_secs(3),
            jitter: false,
        };

        let delay = compute_backoff_duration_with_random(&backoff, u32::MAX, || None);

        assert_eq!(delay, Duration::from_secs(3));
    }
}
