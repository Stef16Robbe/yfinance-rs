use super::{CurrencyHints, hints::CurrencyHintField};
use crate::core::{
    YfClient,
    client::{CacheMode, RetryConfig},
    quotes, quotesummary,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct CurrencyModules {
    #[serde(rename = "financialData")]
    financial_data: Option<FinancialDataCurrency>,
    earnings: Option<EarningsCurrency>,
}

#[derive(Deserialize)]
struct FinancialDataCurrency {
    #[serde(rename = "financialCurrency")]
    financial_currency: Option<String>,
}

#[derive(Deserialize)]
struct EarningsCurrency {
    #[serde(rename = "financialCurrency")]
    financial_currency: Option<String>,
}

impl YfClient {
    pub(super) async fn enrich_quote_hints(
        &self,
        symbol: &str,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) {
        let hints = self.cached_currency_hints(symbol).await;
        if !hints.is_unknown(CurrencyHintField::Quote)
            && !hints.is_unknown(CurrencyHintField::Financial)
        {
            return;
        }

        let symbols = [symbol];
        if let Err(err) = quotes::fetch_v7_quotes(self, &symbols, cache_mode, retry_override).await
            && std::env::var("YF_DEBUG").ok().as_deref() == Some("1")
        {
            eprintln!("YF_DEBUG(currency): failed quote enrichment for {symbol}: {err}");
        }
    }

    pub(super) async fn enrich_quote_summary_reporting_hints(
        &self,
        symbol: &str,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) {
        if !self
            .cached_currency_hints(symbol)
            .await
            .is_unknown(CurrencyHintField::QuoteSummaryFinancial)
        {
            return;
        }

        let Ok(modules) = quotesummary::fetch_module_result::<CurrencyModules>(
            self,
            symbol,
            "financialData,earnings",
            "currency",
            cache_mode,
            retry_override,
        )
        .await
        else {
            return;
        };

        let financial_currency = modules
            .financial_data
            .and_then(|node| node.financial_currency)
            .or_else(|| modules.earnings.and_then(|node| node.financial_currency));

        self.store_currency_hints(
            symbol,
            CurrencyHints::from_quote_summary_financial(financial_currency.as_deref()),
        )
        .await;
    }

    pub(super) async fn enrich_profile_hints(&self, symbol: &str) {
        if !self
            .cached_currency_hints(symbol)
            .await
            .is_unknown(CurrencyHintField::ProfileCountry)
        {
            return;
        }

        let Ok(profile) = crate::profile::load_profile(self, symbol).await else {
            return;
        };

        if let crate::profile::Profile::Company(company) = profile {
            let country = company
                .address
                .as_ref()
                .and_then(|address| address.country.as_deref());
            self.store_currency_hints(symbol, CurrencyHints::from_profile(country, None, None))
                .await;
        }
    }
}
