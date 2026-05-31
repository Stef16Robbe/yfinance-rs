mod api;
mod model;

mod fetch;
mod wire;

pub use model::{
    EarningsTrendRow, PriceTarget, RecommendationRow, RecommendationSummary, UpgradeDowngradeRow,
};

use crate::core::{CallOptions, YfClient, YfError, YfResponse};
use paft::money::Currency;

pub(crate) struct InfoAnalysisParts {
    pub(crate) price_target: Result<YfResponse<PriceTarget>, YfError>,
    pub(crate) recommendation_summary: Result<YfResponse<RecommendationSummary>, YfError>,
}

/// A builder for fetching analyst-related data for a specific symbol.
pub struct AnalysisBuilder {
    client: YfClient,
    symbol: String,
    options: CallOptions,
}

impl AnalysisBuilder {
    /// Creates a new `AnalysisBuilder` for a given symbol.
    pub fn new(client: &YfClient, symbol: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            symbol: symbol.into(),
            options: CallOptions::default(),
        }
    }

    crate::core::impl_call_option_setters!();

    /// Fetches the analyst recommendation trend over time.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the data is malformed.
    pub async fn recommendations(&self) -> Result<Vec<RecommendationRow>, YfError> {
        Ok(self.recommendations_with_diagnostics().await?.into_data())
    }

    /// Fetches analyst recommendation trends with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn recommendations_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<RecommendationRow>>, YfError> {
        api::recommendation_trend(&self.client, &self.symbol, &self.options).await
    }

    /// Fetches a summary of the latest analyst recommendations.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the data is malformed.
    pub async fn recommendations_summary(&self) -> Result<RecommendationSummary, YfError> {
        Ok(self
            .recommendations_summary_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches the latest analyst recommendation summary with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn recommendations_summary_with_diagnostics(
        &self,
    ) -> Result<YfResponse<RecommendationSummary>, YfError> {
        api::recommendation_summary(&self.client, &self.symbol, &self.options).await
    }

    /// Fetches the history of analyst upgrades and downgrades for the symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the data is malformed.
    pub async fn upgrades_downgrades(&self) -> Result<Vec<UpgradeDowngradeRow>, YfError> {
        Ok(self
            .upgrades_downgrades_with_diagnostics()
            .await?
            .into_data())
    }

    /// Fetches analyst upgrades and downgrades with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn upgrades_downgrades_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<UpgradeDowngradeRow>>, YfError> {
        api::upgrades_downgrades(&self.client, &self.symbol, &self.options).await
    }

    /// Fetches the analyst price target summary.
    ///
    /// Provide `Some(currency)` to override the auto-resolved currency for this call;
    /// pass `None` to enrich currency metadata from Yahoo and infer only when needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the data is malformed.
    pub async fn analyst_price_target(
        &self,
        override_currency: Option<Currency>,
    ) -> Result<PriceTarget, YfError> {
        Ok(self
            .analyst_price_target_with_diagnostics(override_currency)
            .await?
            .into_data())
    }

    /// Fetches analyst price targets with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn analyst_price_target_with_diagnostics(
        &self,
        override_currency: Option<Currency>,
    ) -> Result<YfResponse<PriceTarget>, YfError> {
        api::analyst_price_target(&self.client, &self.symbol, override_currency, &self.options)
            .await
    }

    /// Fetches earnings trend data.
    ///
    /// This includes earnings estimates, revenue estimates, EPS trends, and EPS revisions.
    /// Provide `Some(currency)` to override the auto-resolved currency for this call;
    /// pass `None` to enrich currency metadata from Yahoo and infer only when needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the data is malformed.
    pub async fn earnings_trend(
        &self,
        override_currency: Option<Currency>,
    ) -> Result<Vec<EarningsTrendRow>, YfError> {
        Ok(self
            .earnings_trend_with_diagnostics(override_currency)
            .await?
            .into_data())
    }

    /// Fetches earnings trend data with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn earnings_trend_with_diagnostics(
        &self,
        override_currency: Option<Currency>,
    ) -> Result<YfResponse<Vec<EarningsTrendRow>>, YfError> {
        api::earnings_trend(&self.client, &self.symbol, override_currency, &self.options).await
    }
}

pub(crate) async fn price_target_and_recommendation_summary_from_quote_summary_value(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    value: serde_json::Value,
    options: &CallOptions,
) -> Result<InfoAnalysisParts, YfError> {
    api::price_target_and_recommendation_summary_from_quote_summary_value(
        client,
        symbol,
        override_currency,
        value,
        options,
    )
    .await
}
