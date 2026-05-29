use super::unit::ResolvedCurrencyUnit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CurrencyKind {
    Trading,
    Reporting,
    CorporateAction,
    AnalystEstimate,
}

impl CurrencyKind {
    pub(super) const fn caches_direct_provider(self) -> bool {
        !matches!(self, Self::AnalystEstimate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencySource {
    Override,
    DirectProvider,
    CachedProvider,
    QuoteEnrichment,
    QuoteSummaryEnrichment,
    ListingHeuristic,
    ProfileCountryHeuristic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceStrength {
    Override,
    ProfileHeuristic,
    ListingHeuristic,
    EnrichedProvider,
    DirectProvider,
}

impl EvidenceStrength {
    pub(super) const fn is_provider(self) -> bool {
        matches!(self, Self::EnrichedProvider | Self::DirectProvider)
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedCurrency {
    pub(super) unit: ResolvedCurrencyUnit,
    pub(super) source: CurrencySource,
    pub(super) strength: EvidenceStrength,
}

impl ResolvedCurrency {
    pub(super) const fn new(
        unit: ResolvedCurrencyUnit,
        source: CurrencySource,
        strength: EvidenceStrength,
    ) -> Self {
        Self {
            unit,
            source,
            strength,
        }
    }

    pub(crate) const fn source(&self) -> CurrencySource {
        self.source
    }

    pub(crate) const fn strength(&self) -> EvidenceStrength {
        self.strength
    }

    pub(crate) fn into_unit(self) -> ResolvedCurrencyUnit {
        self.unit
    }
}

impl CurrencySource {
    pub(crate) const fn is_explicit(self) -> bool {
        matches!(self, Self::Override | Self::DirectProvider)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CurrencyCacheKey {
    symbol: String,
    kind: CurrencyKind,
}

impl CurrencyCacheKey {
    pub(super) fn new(symbol: &str, kind: CurrencyKind) -> Self {
        Self {
            symbol: symbol.to_string(),
            kind,
        }
    }
}
