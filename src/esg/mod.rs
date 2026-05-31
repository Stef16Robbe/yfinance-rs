mod api;
mod model;
mod wire;

pub use model::{EsgInvolvement, EsgScores, EsgSummary};

use crate::{YfClient, YfError, YfResponse, core::CallOptions};

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

    crate::core::impl_call_option_setters!();

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
