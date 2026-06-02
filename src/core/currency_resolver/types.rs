use super::unit::ResolvedCurrencyUnit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CurrencyPurpose {
    Trading,
    Reporting,
    CorporateAction,
    AnalystEstimate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CurrencyCacheKind {
    Trading,
    Reporting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyResolutionMode {
    TradingLike,
    ReportingLike,
}

impl CurrencyResolutionMode {
    pub(super) const fn cache_kind(self) -> CurrencyCacheKind {
        match self {
            Self::TradingLike => CurrencyCacheKind::Trading,
            Self::ReportingLike => CurrencyCacheKind::Reporting,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCurrencyCache {
    Store(CurrencyCacheKind),
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrencyResolutionSpec {
    purpose: CurrencyPurpose,
    mode: CurrencyResolutionMode,
    direct_cache: DirectCurrencyCache,
}

impl CurrencyResolutionSpec {
    pub(super) const fn trading() -> Self {
        Self {
            purpose: CurrencyPurpose::Trading,
            mode: CurrencyResolutionMode::TradingLike,
            direct_cache: DirectCurrencyCache::Store(CurrencyCacheKind::Trading),
        }
    }

    pub(super) const fn reporting() -> Self {
        Self {
            purpose: CurrencyPurpose::Reporting,
            mode: CurrencyResolutionMode::ReportingLike,
            direct_cache: DirectCurrencyCache::Store(CurrencyCacheKind::Reporting),
        }
    }

    pub(super) const fn corporate_action() -> Self {
        Self {
            purpose: CurrencyPurpose::CorporateAction,
            mode: CurrencyResolutionMode::TradingLike,
            direct_cache: DirectCurrencyCache::Store(CurrencyCacheKind::Trading),
        }
    }

    pub(super) const fn analyst_estimate() -> Self {
        Self {
            purpose: CurrencyPurpose::AnalystEstimate,
            mode: CurrencyResolutionMode::ReportingLike,
            direct_cache: DirectCurrencyCache::Skip,
        }
    }

    pub(super) const fn analyst_price_target() -> Self {
        Self {
            purpose: CurrencyPurpose::AnalystEstimate,
            mode: CurrencyResolutionMode::TradingLike,
            direct_cache: DirectCurrencyCache::Skip,
        }
    }

    pub(crate) const fn purpose(self) -> CurrencyPurpose {
        self.purpose
    }

    pub(super) const fn mode(self) -> CurrencyResolutionMode {
        self.mode
    }

    pub(super) const fn direct_cache(self) -> DirectCurrencyCache {
        self.direct_cache
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyEvidence {
    Trusted(TrustedCurrencyEvidence),
    Inferred(CurrencyInference),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustedCurrencyEvidence {
    Override,
    Provider {
        source: ProviderCurrencySource,
        acquisition: CurrencyAcquisition,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderCurrencySource {
    Direct(DirectCurrencyField),
    Enriched(CurrencyEnrichmentSource),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCurrencyField {
    ChartMeta,
    OptionsQuote,
    FinancialCurrency,
    TimeseriesCurrencyCode,
    EarningsCurrency,
    RevenueCurrency,
    EpsTrendCurrency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyEnrichmentSource {
    QuoteV7,
    QuoteSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyAcquisition {
    Fresh,
    Cached { from: CurrencyCacheKind },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyInference {
    ListingHeuristic,
    ProfileCountryHeuristic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CurrencyCacheRank {
    ProfileHeuristic,
    ListingHeuristic,
    EnrichedProvider,
    DirectProvider,
    Override,
}

impl CurrencyEvidence {
    const fn cache_rank(self) -> CurrencyCacheRank {
        match self {
            Self::Inferred(CurrencyInference::ProfileCountryHeuristic) => {
                CurrencyCacheRank::ProfileHeuristic
            }
            Self::Inferred(CurrencyInference::ListingHeuristic) => {
                CurrencyCacheRank::ListingHeuristic
            }
            Self::Trusted(TrustedCurrencyEvidence::Provider {
                source: ProviderCurrencySource::Enriched(_),
                ..
            }) => CurrencyCacheRank::EnrichedProvider,
            Self::Trusted(TrustedCurrencyEvidence::Provider {
                source: ProviderCurrencySource::Direct(_),
                ..
            }) => CurrencyCacheRank::DirectProvider,
            Self::Trusted(TrustedCurrencyEvidence::Override) => CurrencyCacheRank::Override,
        }
    }

    pub(super) const fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted(_))
    }

    pub(crate) const fn inference(self) -> Option<CurrencyInference> {
        match self {
            Self::Trusted(_) => None,
            Self::Inferred(inference) => Some(inference),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InvalidCurrencyEvidence {
    path: &'static str,
    code: String,
}

impl InvalidCurrencyEvidence {
    pub(super) fn new(path: &'static str, code: impl Into<String>) -> Self {
        Self {
            path,
            code: code.into(),
        }
    }

    pub(super) const fn path(&self) -> &'static str {
        self.path
    }

    pub(super) fn code(&self) -> &str {
        &self.code
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedCurrency {
    pub(super) unit: ResolvedCurrencyUnit,
    evidence: CurrencyEvidence,
    invalid_evidence: Vec<InvalidCurrencyEvidence>,
}

impl ResolvedCurrency {
    const fn new(unit: ResolvedCurrencyUnit, evidence: CurrencyEvidence) -> Self {
        Self {
            unit,
            evidence,
            invalid_evidence: Vec::new(),
        }
    }

    pub(super) const fn override_currency(unit: ResolvedCurrencyUnit) -> Self {
        Self::new(
            unit,
            CurrencyEvidence::Trusted(TrustedCurrencyEvidence::Override),
        )
    }

    pub(super) const fn direct_provider(
        unit: ResolvedCurrencyUnit,
        field: DirectCurrencyField,
    ) -> Self {
        Self::new(
            unit,
            CurrencyEvidence::Trusted(TrustedCurrencyEvidence::Provider {
                source: ProviderCurrencySource::Direct(field),
                acquisition: CurrencyAcquisition::Fresh,
            }),
        )
    }

    pub(super) const fn quote_enrichment(unit: ResolvedCurrencyUnit) -> Self {
        Self::provider_enrichment(unit, CurrencyEnrichmentSource::QuoteV7)
    }

    pub(super) const fn quote_summary_enrichment(unit: ResolvedCurrencyUnit) -> Self {
        Self::provider_enrichment(unit, CurrencyEnrichmentSource::QuoteSummary)
    }

    const fn provider_enrichment(
        unit: ResolvedCurrencyUnit,
        source: CurrencyEnrichmentSource,
    ) -> Self {
        Self::new(
            unit,
            CurrencyEvidence::Trusted(TrustedCurrencyEvidence::Provider {
                source: ProviderCurrencySource::Enriched(source),
                acquisition: CurrencyAcquisition::Fresh,
            }),
        )
    }

    pub(super) const fn listing_heuristic(unit: ResolvedCurrencyUnit) -> Self {
        Self::new(
            unit,
            CurrencyEvidence::Inferred(CurrencyInference::ListingHeuristic),
        )
    }

    pub(super) const fn profile_country_heuristic(unit: ResolvedCurrencyUnit) -> Self {
        Self::new(
            unit,
            CurrencyEvidence::Inferred(CurrencyInference::ProfileCountryHeuristic),
        )
    }

    pub(super) fn with_invalid_evidence(
        mut self,
        invalid_evidence: impl IntoIterator<Item = InvalidCurrencyEvidence>,
    ) -> Self {
        self.invalid_evidence.extend(invalid_evidence);
        self
    }

    pub(crate) const fn evidence(&self) -> CurrencyEvidence {
        self.evidence
    }

    pub(super) const fn is_trusted(&self) -> bool {
        self.evidence.is_trusted()
    }

    pub(super) fn cache_rank_ge(&self, other: &Self) -> bool {
        self.evidence.cache_rank() >= other.evidence.cache_rank()
    }

    pub(super) const fn with_cached_acquisition(mut self, from: CurrencyCacheKind) -> Self {
        if let CurrencyEvidence::Trusted(TrustedCurrencyEvidence::Provider {
            acquisition, ..
        }) = &mut self.evidence
        {
            *acquisition = CurrencyAcquisition::Cached { from };
        }
        self
    }

    pub(super) fn invalid_evidence(&self) -> &[InvalidCurrencyEvidence] {
        &self.invalid_evidence
    }

    pub(crate) fn into_unit(self) -> ResolvedCurrencyUnit {
        self.unit
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CurrencyCacheKey {
    symbol: String,
    kind: CurrencyCacheKind,
}

impl CurrencyCacheKey {
    pub(super) fn new(symbol: &str, kind: CurrencyCacheKind) -> Self {
        Self {
            symbol: symbol.to_string(),
            kind,
        }
    }
}
