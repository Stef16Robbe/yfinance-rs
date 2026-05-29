use paft::domain::{Exchange, Instrument};
use paft::market::responses::search::{SearchResponse, SearchResult};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::core::ProjectionContext;
use crate::core::client::{CacheEndpoint, CacheMode, RetryConfig};
use crate::core::conversions::string_to_asset_kind;
use crate::{DataQuality, ProjectionIssue, YfClient, YfError, YfResponse};

#[allow(clippy::too_many_lines)]
fn parse_search_body(body: &str, ctx: &mut ProjectionContext) -> Result<SearchResponse, YfError> {
    let env: V1SearchEnvelope = serde_json::from_str(body).map_err(YfError::Json)?;

    let quotes = env
        .quotes
        .ok_or_else(|| YfError::MissingData("search quotes missing".into()))?;
    let mut results = Vec::new();
    for (idx, q) in quotes.into_iter().enumerate() {
        let key = Some(search_quote_diag_key(&q, idx));
        let q = match serde_json::from_value::<V1SearchQuote>(q) {
            Ok(q) => q,
            Err(err) => {
                ctx.dropped_item(
                    "search_result",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "quote",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let key = q.symbol.clone();
        let Some(sym) = q.symbol.map(|sym| sym.trim().to_string()) else {
            ctx.dropped_item(
                "search_result",
                key,
                ProjectionIssue::MissingRequiredField { field: "symbol" },
            )?;
            continue;
        };
        if sym.is_empty() {
            ctx.dropped_item(
                "search_result",
                key,
                ProjectionIssue::MissingRequiredField { field: "symbol" },
            )?;
            continue;
        }
        let exchange_opt = match q
            .exchange
            .as_deref()
            .map(str::trim)
            .filter(|exchange| !exchange.is_empty())
        {
            Some(exchange) => match exchange.parse::<Exchange>() {
                Ok(exchange) => Some(exchange),
                Err(err) => {
                    ctx.omitted_present_field(
                        "quotes[].exchange",
                        Some(sym.clone()),
                        ProjectionIssue::InvalidField {
                            field: "exchange",
                            details: err.to_string(),
                        },
                    )?;
                    None
                }
            },
            None => None,
        };
        let kind = match q
            .quote_type
            .as_deref()
            .map(string_to_asset_kind)
            .transpose()
        {
            Ok(Some(kind)) => kind,
            Ok(None) => {
                ctx.dropped_item(
                    "search_result",
                    Some(sym),
                    ProjectionIssue::MissingRequiredField { field: "quoteType" },
                )?;
                continue;
            }
            Err(err) => {
                ctx.dropped_item(
                    "search_result",
                    Some(sym),
                    ProjectionIssue::InvalidField {
                        field: "quoteType",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };

        let instrument = match exchange_opt {
            Some(exchange) => Instrument::from_symbol_and_exchange(&sym, exchange, kind),
            None => Instrument::from_symbol(&sym, kind),
        };
        let Ok(instrument) = instrument else {
            ctx.dropped_item(
                "search_result",
                Some(sym),
                ProjectionIssue::InvalidField {
                    field: "symbol",
                    details: "invalid instrument symbol".into(),
                },
            )?;
            continue;
        };

        results.push(SearchResult {
            instrument,
            name: q.longname.or(q.shortname),
            provider: (),
        });
    }

    Ok(SearchResponse {
        results,
        provider: (),
    })
}

fn search_quote_diag_key(value: &Value, idx: usize) -> String {
    value
        .get("symbol")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map_or_else(|| format!("quotes[{idx}]"), ToString::to_string)
}

/* ---------------- Public API ---------------- */

/// Searches for symbols matching a query.
///
/// # Errors
///
/// Returns `YfError` if the network request fails or the response cannot be parsed.
pub async fn search(client: &YfClient, query: &str) -> Result<SearchResponse, YfError> {
    SearchBuilder::new(client, query).fetch().await
}

/// A builder for searching for tickers and other assets on Yahoo Finance.
#[derive(Debug)]
pub struct SearchBuilder {
    client: YfClient,
    base: Url,
    query: String,
    quotes_count: Option<u32>,
    news_count: Option<u32>,
    lists_count: Option<u32>,
    lang: Option<String>,
    region: Option<String>,
    cache_mode: CacheMode,
    retry_override: Option<RetryConfig>,
    data_quality: DataQuality,
}

impl SearchBuilder {
    /// Creates a new `SearchBuilder` for a given search query.
    ///
    /// # Panics
    ///
    /// This function will panic if the hardcoded `DEFAULT_BASE_SEARCH_V1` constant
    /// is not a valid URL. This indicates a bug within the crate itself.
    pub fn new(client: &YfClient, query: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            base: Url::parse(DEFAULT_BASE_SEARCH_V1).unwrap(),
            query: query.into(),
            quotes_count: Some(10),
            news_count: Some(0),
            lists_count: Some(0),
            lang: None,
            region: None,
            cache_mode: CacheMode::Default,
            retry_override: None,
            data_quality: DataQuality::BestEffort,
        }
    }

    /// Sets the cache mode for this specific API call.
    #[must_use]
    pub const fn cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    /// Overrides the default retry policy for this specific API call.
    #[must_use]
    pub fn retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.retry_override = cfg;
        self
    }

    /// Sets how provider projection issues are handled.
    #[must_use]
    pub const fn data_quality(mut self, policy: DataQuality) -> Self {
        self.data_quality = policy;
        self
    }

    /// Fails when Yahoo data cannot be projected losslessly.
    #[must_use]
    pub const fn strict(self) -> Self {
        self.data_quality(DataQuality::Strict)
    }

    /// (For testing) Overrides the base URL for the search API.
    #[must_use]
    pub fn search_base(mut self, base: Url) -> Self {
        self.base = base;
        self
    }

    /// Sets the maximum number of quote results to return.
    #[must_use]
    pub const fn quotes_count(mut self, n: u32) -> Self {
        self.quotes_count = Some(n);
        self
    }

    /// Sets the maximum number of news results to return.
    #[must_use]
    pub const fn news_count(mut self, n: u32) -> Self {
        self.news_count = Some(n);
        self
    }

    /// Sets the maximum number of screener list results to return.
    #[must_use]
    pub const fn lists_count(mut self, n: u32) -> Self {
        self.lists_count = Some(n);
        self
    }

    /// Sets the language for the search results.
    #[must_use]
    pub fn lang(mut self, s: impl Into<String>) -> Self {
        self.lang = Some(s.into());
        self
    }

    /// Sets the region for the search results.
    #[must_use]
    pub fn region(mut self, s: impl Into<String>) -> Self {
        self.region = Some(s.into());
        self
    }

    /// Returns the configured language parameter, if any.
    #[must_use]
    pub fn lang_ref(&self) -> Option<&str> {
        self.lang.as_deref()
    }

    /// Returns the configured region parameter, if any.
    #[must_use]
    pub fn region_ref(&self) -> Option<&str> {
        self.region.as_deref()
    }

    /// Executes the search request.
    ///
    /// # Errors
    ///
    /// This method will return an error if the network request fails, the API returns a
    /// non-successful status code, or the response body cannot be parsed as a valid search result.
    pub async fn fetch(&self) -> Result<SearchResponse, crate::core::YfError> {
        Ok(self.fetch_with_diagnostics().await?.into_data())
    }

    /// Executes the search request with projection diagnostics.
    ///
    /// # Errors
    ///
    /// This method will return an error if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn fetch_with_diagnostics(&self) -> Result<YfResponse<SearchResponse>, YfError> {
        let mut ctx = ProjectionContext::new("search", self.data_quality);
        let mut url = self.base.clone();
        Self::append_query_params(
            &mut url,
            &self.query,
            self.quotes_count,
            self.news_count,
            self.lists_count,
            self.lang.as_deref(),
            self.region.as_deref(),
        );

        let (body, _) = crate::core::net::fetch_text_with_auth_retry(
            &self.client,
            url,
            crate::core::net::AuthFetchConfig {
                auth_mode: crate::core::net::AuthMode::OptionalCrumb,
                cache_endpoint: CacheEndpoint::Search,
                cache_mode: self.cache_mode,
                cache_body: None,
                retry_override: self.retry_override.as_ref(),
                endpoint: "search_v1",
                fixture_key: &self.query,
                ext: "json",
                retry_on_invalid_crumb_body: true,
            },
            |url| {
                self.client
                    .http()
                    .get(url)
                    .header("accept", "application/json")
            },
        )
        .await?;

        let data = parse_search_body(&body, &mut ctx)?;
        Ok(ctx.finish(data))
    }

    fn append_query_params(
        url: &mut Url,
        query: &str,
        quotes_count: Option<u32>,
        news_count: Option<u32>,
        lists_count: Option<u32>,
        lang: Option<&str>,
        region: Option<&str>,
    ) {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("q", query);
        if let Some(n) = quotes_count {
            qp.append_pair("quotesCount", &n.to_string());
        }
        if let Some(n) = news_count {
            qp.append_pair("newsCount", &n.to_string());
        }
        if let Some(n) = lists_count {
            qp.append_pair("listsCount", &n.to_string());
        }
        if let Some(l) = lang {
            qp.append_pair("lang", l);
        }
        if let Some(r) = region {
            qp.append_pair("region", r);
        }
    }
}

/* ---------------- Types returned by this module ---------------- */
// Local types removed in favor of paft::market::responses::search::{SearchResponse, SearchResult}

const DEFAULT_BASE_SEARCH_V1: &str = "https://query2.finance.yahoo.com/v1/finance/search";

/* ------------- Minimal serde mapping of /v1/finance/search ------------- */

#[derive(Deserialize)]
struct V1SearchEnvelope {
    #[allow(dead_code)]
    explains: Option<serde_json::Value>,
    #[allow(dead_code)]
    count: Option<i64>,
    quotes: Option<Vec<Value>>,
    #[allow(dead_code)]
    news: Option<serde_json::Value>,
    #[allow(dead_code)]
    nav: Option<serde_json::Value>,
    #[allow(dead_code)]
    lists: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct V1SearchQuote {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    shortname: Option<String>,
    #[serde(default)]
    longname: Option<String>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: Option<String>,
    #[serde(default)]
    exchange: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "exchDisp")]
    #[serde(default)]
    exch_disp: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "typeDisp")]
    #[serde(default)]
    type_disp: Option<String>,
}
