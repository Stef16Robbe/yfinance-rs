use crate::core::{
    CallOptions, DataQuality, ProjectionContext, Quote, YfClient, YfError, YfResponse,
    client::{CacheMode, RetryConfig},
    quotes as core_quotes,
};

/// Fetches quotes for multiple symbols.
///
/// # Errors
///
/// Returns `YfError` if the network request fails, the response cannot be parsed,
/// or the data for the symbols is not available.
pub async fn quotes<I, S>(client: &YfClient, symbols: I) -> Result<Vec<Quote>, YfError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    QuotesBuilder::new(client).symbols(symbols).fetch().await
}

/// Fetches quotes for multiple symbols with projection diagnostics.
///
/// # Errors
///
/// Returns `YfError` if the network request fails, the response cannot be parsed,
/// or strict data-quality mode rejects a projection issue.
pub async fn quotes_with_diagnostics<I, S>(
    client: &YfClient,
    symbols: I,
) -> Result<YfResponse<Vec<Quote>>, YfError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    QuotesBuilder::new(client)
        .symbols(symbols)
        .fetch_with_diagnostics()
        .await
}

/// A builder for fetching quotes for one or more symbols.
pub struct QuotesBuilder {
    client: YfClient,
    symbols: Vec<String>,
    options: CallOptions,
}

impl QuotesBuilder {
    /// Creates a new `QuotesBuilder`.
    #[must_use]
    pub fn new(client: &YfClient) -> Self {
        Self {
            client: client.clone(),
            symbols: Vec::new(),
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

    /// Fails when Yahoo quote data cannot be projected losslessly.
    #[must_use]
    pub const fn strict(self) -> Self {
        self.data_quality(DataQuality::Strict)
    }

    /// Replaces the current list of symbols with a new list.
    #[must_use]
    pub fn symbols<I, S>(mut self, syms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.symbols = syms.into_iter().map(Into::into).collect();
        self
    }

    /// Adds a single symbol to the list.
    #[must_use]
    pub fn add_symbol(mut self, sym: impl Into<String>) -> Self {
        self.symbols.push(sym.into());
        self
    }

    /// Fetches the quotes for the configured symbols.
    ///
    /// # Errors
    ///
    /// Returns `YfError` if no symbols were provided, the network request fails,
    /// the response cannot be parsed, or data for the symbols is not available.
    pub async fn fetch(&self) -> Result<Vec<crate::core::Quote>, crate::core::YfError> {
        Ok(self.fetch_with_diagnostics().await?.into_data())
    }

    /// Fetches the quotes for the configured symbols with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns `YfError` if no symbols were provided, the network request fails,
    /// the response cannot be parsed, or strict data-quality mode rejects a projection issue.
    pub async fn fetch_with_diagnostics(
        &self,
    ) -> Result<YfResponse<Vec<crate::core::Quote>>, crate::core::YfError> {
        if self.symbols.is_empty() {
            return Err(crate::core::YfError::InvalidParams(
                "symbols list cannot be empty".into(),
            ));
        }

        let symbol_slices: Vec<&str> = self.symbols.iter().map(AsRef::as_ref).collect();
        let results =
            core_quotes::fetch_v7_quote_values(&self.client, &symbol_slices, &self.options).await?;

        let mut ctx = ProjectionContext::new("quotes", self.options.data_quality());
        let mut quotes = Vec::with_capacity(results.len());
        for (idx, result) in results.into_iter().enumerate() {
            let Some(result) =
                core_quotes::quote_node_from_value_with_context(result, idx, &mut ctx)?
            else {
                continue;
            };
            if let Some(quote) = result.to_quote_item_with_context(&mut ctx)? {
                quotes.push(quote);
            }
        }

        Ok(ctx.finish(quotes))
    }
}
