/// Currency purpose used by diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YfCurrencyKind {
    /// Trading currency for quoted prices.
    Trading,
    /// Reporting currency for statements and ownership totals.
    Reporting,
    /// Currency for dividends and capital gains.
    CorporateAction,
    /// Currency for analyst estimates.
    AnalystEstimate,
}

/// Currency evidence source used by diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YfCurrencySource {
    /// Explicit caller override.
    Override,
    /// Direct field in the endpoint payload.
    DirectProvider,
    /// Previously cached provider evidence.
    CachedProvider,
    /// v7 quote enrichment.
    QuoteEnrichment,
    /// quoteSummary enrichment.
    QuoteSummaryEnrichment,
    /// Symbol/listing heuristic.
    ListingHeuristic,
    /// Profile country heuristic.
    ProfileCountryHeuristic,
}

/// Currency evidence strength used by diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum YfEvidenceStrength {
    /// Caller-supplied override.
    Override,
    /// Profile-country heuristic.
    ProfileHeuristic,
    /// Listing/exchange heuristic.
    ListingHeuristic,
    /// Provider evidence obtained through enrichment.
    EnrichedProvider,
    /// Direct provider evidence from the requested payload.
    DirectProvider,
}

impl From<crate::core::currency_resolver::CurrencyKind> for YfCurrencyKind {
    fn from(value: crate::core::currency_resolver::CurrencyKind) -> Self {
        match value {
            crate::core::currency_resolver::CurrencyKind::Trading => Self::Trading,
            crate::core::currency_resolver::CurrencyKind::Reporting => Self::Reporting,
            crate::core::currency_resolver::CurrencyKind::CorporateAction => Self::CorporateAction,
            crate::core::currency_resolver::CurrencyKind::AnalystEstimate => Self::AnalystEstimate,
        }
    }
}

impl From<crate::core::currency_resolver::CurrencySource> for YfCurrencySource {
    fn from(value: crate::core::currency_resolver::CurrencySource) -> Self {
        match value {
            crate::core::currency_resolver::CurrencySource::Override => Self::Override,
            crate::core::currency_resolver::CurrencySource::DirectProvider => Self::DirectProvider,
            crate::core::currency_resolver::CurrencySource::CachedProvider => Self::CachedProvider,
            crate::core::currency_resolver::CurrencySource::QuoteEnrichment => {
                Self::QuoteEnrichment
            }
            crate::core::currency_resolver::CurrencySource::QuoteSummaryEnrichment => {
                Self::QuoteSummaryEnrichment
            }
            crate::core::currency_resolver::CurrencySource::ListingHeuristic => {
                Self::ListingHeuristic
            }
            crate::core::currency_resolver::CurrencySource::ProfileCountryHeuristic => {
                Self::ProfileCountryHeuristic
            }
        }
    }
}

impl From<crate::core::currency_resolver::EvidenceStrength> for YfEvidenceStrength {
    fn from(value: crate::core::currency_resolver::EvidenceStrength) -> Self {
        match value {
            crate::core::currency_resolver::EvidenceStrength::Override => Self::Override,
            crate::core::currency_resolver::EvidenceStrength::ProfileHeuristic => {
                Self::ProfileHeuristic
            }
            crate::core::currency_resolver::EvidenceStrength::ListingHeuristic => {
                Self::ListingHeuristic
            }
            crate::core::currency_resolver::EvidenceStrength::EnrichedProvider => {
                Self::EnrichedProvider
            }
            crate::core::currency_resolver::EvidenceStrength::DirectProvider => {
                Self::DirectProvider
            }
        }
    }
}
