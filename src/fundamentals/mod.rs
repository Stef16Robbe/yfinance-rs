mod api;
mod model;

mod fetch;
mod wire;

pub use model::{
    BalanceSheetRow, Calendar, CashflowRow, Earnings, EarningsQuarter, EarningsQuarterEps,
    EarningsYear, IncomeStatementRow, ShareCount,
};

use crate::core::{CallOptions, DataQuality, YfClient, YfError, YfResponse};
use chrono::{DateTime, Utc};
use paft::money::Currency;

/// A builder for fetching fundamental financial data (statements, earnings, etc.).
pub struct FundamentalsBuilder {
    client: YfClient,
    symbol: String,
    options: CallOptions,
}

impl FundamentalsBuilder {
    /// Creates a new `FundamentalsBuilder` for a given symbol.
    pub fn new(client: &YfClient, symbol: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            symbol: symbol.into(),
            options: CallOptions::default(),
        }
    }

    crate::core::impl_call_option_setters!();

    /// Fetches the income statement.
    ///
    /// Set `quarterly` to `true` to get quarterly reports, or `false` for annual reports.
    /// Provide `Some(currency)` to override the auto-resolved reporting currency for this call;
    /// pass `None` to enrich currency metadata from Yahoo and infer only when needed.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn income_statement(
        &self,
        quarterly: bool,
        override_currency: Option<Currency>,
    ) -> Result<Vec<IncomeStatementRow>, YfError> {
        Ok(self
            .income_statement_with_diagnostics(quarterly, override_currency)
            .await?
            .into_data())
    }

    /// Fetches the income statement with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn income_statement_with_diagnostics(
        &self,
        quarterly: bool,
        override_currency: Option<Currency>,
    ) -> Result<YfResponse<Vec<IncomeStatementRow>>, YfError> {
        api::income_statement(
            &self.client,
            &self.symbol,
            quarterly,
            override_currency,
            &self.options,
        )
        .await
    }

    /// Fetches the balance sheet.
    ///
    /// Set `quarterly` to `true` to get quarterly reports, or `false` for annual reports.
    /// Provide `Some(currency)` to override the auto-resolved reporting currency for this call;
    /// pass `None` to enrich currency metadata from Yahoo and infer only when needed.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn balance_sheet(
        &self,
        quarterly: bool,
        override_currency: Option<Currency>,
    ) -> Result<Vec<BalanceSheetRow>, YfError> {
        Ok(self
            .balance_sheet_with_diagnostics(quarterly, override_currency)
            .await?
            .into_data())
    }

    /// Fetches the balance sheet with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn balance_sheet_with_diagnostics(
        &self,
        quarterly: bool,
        override_currency: Option<Currency>,
    ) -> Result<YfResponse<Vec<BalanceSheetRow>>, YfError> {
        api::balance_sheet(
            &self.client,
            &self.symbol,
            quarterly,
            override_currency,
            &self.options,
        )
        .await
    }

    /// Fetches the cash flow statement.
    ///
    /// Set `quarterly` to `true` to get quarterly reports, or `false` for annual reports.
    /// Provide `Some(currency)` to override the auto-resolved reporting currency for this call;
    /// pass `None` to enrich currency metadata from Yahoo and infer only when needed.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn cashflow(
        &self,
        quarterly: bool,
        override_currency: Option<Currency>,
    ) -> Result<Vec<CashflowRow>, YfError> {
        Ok(self
            .cashflow_with_diagnostics(quarterly, override_currency)
            .await?
            .into_data())
    }

    /// Fetches the cash flow statement with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn cashflow_with_diagnostics(
        &self,
        quarterly: bool,
        override_currency: Option<Currency>,
    ) -> Result<YfResponse<Vec<CashflowRow>>, YfError> {
        api::cashflow(
            &self.client,
            &self.symbol,
            quarterly,
            override_currency,
            &self.options,
        )
        .await
    }

    /// Fetches earnings history and estimates.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn earnings(&self, override_currency: Option<Currency>) -> Result<Earnings, YfError> {
        Ok(self
            .earnings_with_diagnostics(override_currency)
            .await?
            .into_data())
    }

    /// Fetches earnings history and estimates with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn earnings_with_diagnostics(
        &self,
        override_currency: Option<Currency>,
    ) -> Result<YfResponse<Earnings>, YfError> {
        api::earnings(&self.client, &self.symbol, override_currency, &self.options).await
    }

    /// Fetches corporate calendar events like earnings dates.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn calendar(&self) -> Result<Calendar, YfError> {
        Ok(self.calendar_with_diagnostics().await?.into_data())
    }

    /// Fetches corporate calendar events with projection diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn calendar_with_diagnostics(&self) -> Result<YfResponse<Calendar>, YfError> {
        api::calendar(&self.client, &self.symbol, &self.options).await
    }

    /// Fetches the historical number of shares outstanding.
    ///
    /// If `quarterly` is true, fetches quarterly data, otherwise annual data is fetched.
    /// The default request uses Yahoo's rolling 548-day share-count window, matching
    /// Python yfinance's `get_shares_full(start=None, end=None)`. Use
    /// [`Self::shares_between`] to request an explicit wider window.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the network request fails or the API response cannot be parsed.
    pub async fn shares(&self, quarterly: bool) -> Result<Vec<ShareCount>, YfError> {
        Ok(self.shares_with_diagnostics(quarterly).await?.into_data())
    }

    /// Fetches historical shares outstanding with projection diagnostics.
    ///
    /// The default request uses Yahoo's rolling 548-day share-count window. Use
    /// [`Self::shares_between_with_diagnostics`] to request an explicit wider window.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the request fails or strict data-quality mode rejects a projection issue.
    pub async fn shares_with_diagnostics(
        &self,
        quarterly: bool,
    ) -> Result<YfResponse<Vec<ShareCount>>, YfError> {
        self.shares_window_with_diagnostics(quarterly, None, None)
            .await
    }

    /// Fetches historical shares outstanding within an explicit UTC time window.
    ///
    /// If `quarterly` is true, fetches quarterly data, otherwise annual data is fetched.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the date range is invalid, the network request fails,
    /// or the API response cannot be parsed.
    pub async fn shares_between(
        &self,
        quarterly: bool,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ShareCount>, YfError> {
        Ok(self
            .shares_between_with_diagnostics(quarterly, start, end)
            .await?
            .into_data())
    }

    /// Fetches historical shares outstanding within an explicit UTC time window with diagnostics.
    ///
    /// If `quarterly` is true, fetches quarterly data, otherwise annual data is fetched.
    ///
    /// # Errors
    ///
    /// Returns a `YfError` if the date range is invalid, the request fails,
    /// or strict data-quality mode rejects a projection issue.
    pub async fn shares_between_with_diagnostics(
        &self,
        quarterly: bool,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<YfResponse<Vec<ShareCount>>, YfError> {
        self.shares_window_with_diagnostics(quarterly, Some(start), Some(end))
            .await
    }

    async fn shares_window_with_diagnostics(
        &self,
        quarterly: bool,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<YfResponse<Vec<ShareCount>>, YfError> {
        api::shares(
            &self.client,
            &self.symbol,
            start,
            end,
            quarterly,
            &self.options,
        )
        .await
    }
}

pub(crate) fn calendar_from_quote_summary_raw(
    raw: &serde_json::value::RawValue,
    data_quality: DataQuality,
) -> Result<YfResponse<Calendar>, YfError> {
    api::calendar_from_quote_summary_raw(raw, data_quality)
}
