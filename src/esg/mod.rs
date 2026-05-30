mod api;
mod model;
mod wire;

pub use model::{EsgInvolvement, EsgScores, EsgSummary};

use crate::{
    DataQuality, YfClient, YfError, YfResponse,
    core::{
        CallOptions,
        client::{CacheMode, RetryConfig},
    },
};

/// A builder for fetching ESG (Environmental, Social, and Governance) data for a specific symbol.
pub struct EsgBuilder {
    client: YfClient,
    symbol: String,
    options: CallOptions,
}

impl EsgBuilder {
    /// Creates a new `EsgBuilder` for a given symbol.
    pub fn new(client: &YfClient, symbol: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            symbol: symbol.into(),
            options: CallOptions::default(),
        }
    }

    /// Sets the cache mode for this specific API call.
    #[must_use]
    pub const fn cache_mode(mut self, mode: CacheMode) -> Self {
        self.options.cache_mode = mode;
        self
    }

    /// Overrides the default retry policy for this specific API call.
    #[must_use]
    pub fn retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.options = self.options.with_retry_policy(cfg);
        self
    }

    /// Sets how provider projection issues are handled.
    #[must_use]
    pub const fn data_quality(mut self, policy: DataQuality) -> Self {
        self.options.data_quality = policy;
        self
    }

    /// Fails when Yahoo data cannot be projected losslessly.
    #[must_use]
    pub const fn strict(self) -> Self {
        self.data_quality(DataQuality::Strict)
    }

    /// Fetches the ESG scores and involvement data for the symbol.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn fetch(&self) -> Result<EsgSummary, YfError> {
        Ok(self.fetch_with_diagnostics().await?.into_data())
    }

    /// Fetches ESG scores and involvement data with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn fetch_with_diagnostics(&self) -> Result<YfResponse<EsgSummary>, YfError> {
        api::fetch_esg_scores(&self.client, &self.symbol, &self.options).await
    }
}
