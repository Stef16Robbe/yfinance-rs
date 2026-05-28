mod api;
mod model;
mod wire;

use serde::{Deserialize, Serialize};

/// Tabs for filtering the Yahoo Finance news endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NewsTab {
    /// Only editorial news items.
    #[default]
    News,
    /// All items including press releases.
    All,
    /// Only press releases.
    PressReleases,
}
pub use model::NewsArticle;

use crate::{
    DataQuality, YfClient, YfError, YfResponse,
    core::client::{CacheMode, RetryConfig},
};

pub(crate) const fn tab_as_str(tab: NewsTab) -> &'static str {
    match tab {
        NewsTab::News => "latestNews",
        NewsTab::All => "newsAll",
        NewsTab::PressReleases => "pressRelease",
    }
}

/// A builder for fetching news articles for a specific symbol.
pub struct NewsBuilder {
    client: YfClient,
    symbol: String,
    count: u32,
    tab: NewsTab,
    cache_mode: CacheMode,
    retry_override: Option<RetryConfig>,
    data_quality: DataQuality,
}

impl NewsBuilder {
    /// Creates a new `NewsBuilder` for a given symbol.
    pub fn new(client: &YfClient, symbol: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            symbol: symbol.into(),
            count: 10,
            tab: NewsTab::default(),
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

    /// Sets the maximum number of news articles to return.
    #[must_use]
    pub const fn count(mut self, count: u32) -> Self {
        self.count = count;
        self
    }

    /// Sets the category of news to fetch.
    #[must_use]
    pub const fn tab(mut self, tab: NewsTab) -> Self {
        self.tab = tab;
        self
    }

    /// Executes the request and fetches the news articles.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request to the Yahoo Finance API fails,
    /// if the response cannot be parsed, or if there's a network issue.
    pub async fn fetch(self) -> Result<Vec<NewsArticle>, YfError> {
        Ok(self.fetch_with_diagnostics().await?.into_data())
    }

    /// Executes the request and fetches news articles with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn fetch_with_diagnostics(self) -> Result<YfResponse<Vec<NewsArticle>>, YfError> {
        api::fetch_news(
            &self.client,
            &self.symbol,
            self.count,
            self.tab,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }
}
