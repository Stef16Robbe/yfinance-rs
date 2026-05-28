mod api;
mod model;
mod wire;

pub use model::{
    InsiderRosterHolder, InsiderTransaction, InstitutionalHolder, MajorHolder,
    NetSharePurchaseActivity,
};

use crate::{
    DataQuality, YfClient, YfError, YfResponse,
    core::client::{CacheMode, RetryConfig},
};

/// A builder for fetching holder data for a specific symbol.
pub struct HoldersBuilder {
    client: YfClient,
    symbol: String,
    cache_mode: CacheMode,
    retry_override: Option<RetryConfig>,
    data_quality: DataQuality,
}

impl HoldersBuilder {
    /// Creates a new `HoldersBuilder` for a given symbol.
    pub fn new(client: &YfClient, symbol: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            symbol: symbol.into(),
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

    /// Fetches the major holders breakdown (e.g., % insiders, % institutions).
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn major_holders(&self) -> Result<Vec<MajorHolder>, YfError> {
        Ok(self.major_holders_with_diagnostics().await?.into_data())
    }

    /// Fetches the major holders breakdown with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn major_holders_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<MajorHolder>>, YfError> {
        api::major_holders(
            &self.client,
            &self.symbol,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }

    /// Fetches a list of the top institutional holders.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn institutional_holders(&self) -> Result<Vec<InstitutionalHolder>, YfError> {
        Ok(self
            .institutional_holders_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches institutional holders with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn institutional_holders_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<InstitutionalHolder>>, YfError> {
        api::institutional_holders(
            &self.client,
            &self.symbol,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }

    /// Fetches a list of the top mutual fund holders.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn mutual_fund_holders(&self) -> Result<Vec<InstitutionalHolder>, YfError> {
        Ok(self
            .mutual_fund_holders_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches mutual fund holders with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn mutual_fund_holders_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<InstitutionalHolder>>, YfError> {
        api::mutual_fund_holders(
            &self.client,
            &self.symbol,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }

    /// Fetches a list of recent insider transactions.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn insider_transactions(&self) -> Result<Vec<InsiderTransaction>, YfError> {
        Ok(self
            .insider_transactions_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches insider transactions with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn insider_transactions_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<InsiderTransaction>>, YfError> {
        api::insider_transactions(
            &self.client,
            &self.symbol,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }

    /// Fetches a roster of company insiders and their holdings.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn insider_roster_holders(&self) -> Result<Vec<InsiderRosterHolder>, YfError> {
        Ok(self
            .insider_roster_holders_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches insider roster holders with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn insider_roster_holders_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<InsiderRosterHolder>>, YfError> {
        api::insider_roster_holders(
            &self.client,
            &self.symbol,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }

    /// Fetches a summary of net insider purchase and sale activity.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn net_share_purchase_activity(
        &self,
    ) -> Result<Option<NetSharePurchaseActivity>, YfError> {
        Ok(self
            .net_share_purchase_activity_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches net insider purchase and sale activity with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn net_share_purchase_activity_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Option<NetSharePurchaseActivity>>, YfError> {
        api::net_share_purchase_activity(
            &self.client,
            &self.symbol,
            self.cache_mode,
            self.retry_override.as_ref(),
            self.data_quality,
        )
        .await
    }
}
