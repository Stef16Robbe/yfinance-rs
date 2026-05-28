use super::{
    AnalystEstimateCurrencyEvidence, CorporateActionCurrencyEvidence, CurrencyKind,
    ReportingCurrencyEvidence, ResolvedCurrency, ResolvedCurrencyUnit, TradingCurrencyEvidence,
    hints::CurrencyHintField,
    inference,
    types::{CurrencySource, EvidenceStrength},
};
use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
};
use paft::money::Currency;

#[derive(Clone, Copy)]
struct HintEvidence {
    field: CurrencyHintField,
    source: CurrencySource,
    strength: EvidenceStrength,
}

impl HintEvidence {
    const fn new(
        field: CurrencyHintField,
        source: CurrencySource,
        strength: EvidenceStrength,
    ) -> Self {
        Self {
            field,
            source,
            strength,
        }
    }
}

#[derive(Clone, Copy)]
struct DirectCurrencyEvidence<'a> {
    code: Option<&'a str>,
    label: &'static str,
}

impl<'a> DirectCurrencyEvidence<'a> {
    const fn new(code: Option<&'a str>, label: &'static str) -> Self {
        Self { code, label }
    }
}

const TRADING_CACHED_HINTS: [HintEvidence; 1] = [HintEvidence::new(
    CurrencyHintField::Quote,
    CurrencySource::CachedProvider,
    EvidenceStrength::EnrichedProvider,
)];
const TRADING_QUOTE_HINTS: [HintEvidence; 1] = [HintEvidence::new(
    CurrencyHintField::Quote,
    CurrencySource::QuoteEnrichment,
    EvidenceStrength::EnrichedProvider,
)];
const TRADING_PROFILE_HINTS: [HintEvidence; 1] = [HintEvidence::new(
    CurrencyHintField::ProfileCountry,
    CurrencySource::ProfileCountryHeuristic,
    EvidenceStrength::ProfileHeuristic,
)];
const REPORTING_CACHED_HINTS: [HintEvidence; 2] = [
    HintEvidence::new(
        CurrencyHintField::Financial,
        CurrencySource::CachedProvider,
        EvidenceStrength::EnrichedProvider,
    ),
    HintEvidence::new(
        CurrencyHintField::QuoteSummaryFinancial,
        CurrencySource::CachedProvider,
        EvidenceStrength::EnrichedProvider,
    ),
];
const REPORTING_QUOTE_HINTS: [HintEvidence; 1] = [HintEvidence::new(
    CurrencyHintField::Financial,
    CurrencySource::QuoteEnrichment,
    EvidenceStrength::EnrichedProvider,
)];
const REPORTING_QUOTE_SUMMARY_HINTS: [HintEvidence; 1] = [HintEvidence::new(
    CurrencyHintField::QuoteSummaryFinancial,
    CurrencySource::QuoteSummaryEnrichment,
    EvidenceStrength::EnrichedProvider,
)];
const REPORTING_PROFILE_HINTS: [HintEvidence; 1] = [HintEvidence::new(
    CurrencyHintField::ProfileCountry,
    CurrencySource::ProfileCountryHeuristic,
    EvidenceStrength::ProfileHeuristic,
)];

impl YfClient {
    pub(crate) async fn resolve_trading_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: TradingCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_currency_unit_from_evidence(
            symbol,
            CurrencyKind::Trading,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    pub(crate) async fn resolve_reporting_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: ReportingCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_currency_unit_from_evidence(
            symbol,
            CurrencyKind::Reporting,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    pub(crate) async fn resolve_corporate_action_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: CorporateActionCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_currency_unit_from_evidence(
            symbol,
            CurrencyKind::CorporateAction,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    pub(crate) async fn resolve_analyst_estimate_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: AnalystEstimateCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_currency_unit_from_evidence(
            symbol,
            CurrencyKind::AnalystEstimate,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    async fn resolve_currency_unit_from_evidence(
        &self,
        symbol: &str,
        kind: CurrencyKind,
        override_currency: Option<Currency>,
        direct_evidence: DirectCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        if let Some(currency) = override_currency {
            return Ok(ResolvedCurrencyUnit::from_currency(currency));
        }

        if let Some(unit) = direct_currency_unit(symbol, kind, direct_evidence)? {
            if kind.caches_direct_provider() {
                self.store_resolved_currency(
                    symbol,
                    kind,
                    ResolvedCurrency::new(
                        unit.clone(),
                        CurrencySource::DirectProvider,
                        EvidenceStrength::DirectProvider,
                    ),
                )
                .await;
            }
            return Ok(unit);
        }

        let cached = self.cached_resolved_currency(symbol, kind).await;
        if let Some(resolved) = cached.as_ref()
            && self
                .cached_resolution_is_final(symbol, kind, resolved)
                .await
        {
            return Ok(resolved.unit.clone());
        }

        match kind {
            CurrencyKind::Trading => {
                self.resolve_trading_currency_from_hints(
                    symbol,
                    cache_mode,
                    retry_override,
                    cached.as_ref(),
                )
                .await
            }
            CurrencyKind::Reporting | CurrencyKind::AnalystEstimate => {
                let reporting_cached = if matches!(kind, CurrencyKind::AnalystEstimate) {
                    self.cached_resolved_currency(symbol, CurrencyKind::Reporting)
                        .await
                } else {
                    cached
                };
                if let Some(resolved) = reporting_cached.as_ref()
                    && self
                        .cached_resolution_is_final(symbol, CurrencyKind::Reporting, resolved)
                        .await
                {
                    return Ok(resolved.unit.clone());
                }
                self.resolve_reporting_currency_from_hints(
                    symbol,
                    cache_mode,
                    retry_override,
                    reporting_cached.as_ref(),
                )
                .await
            }
            CurrencyKind::CorporateAction => {
                let trading_cached = self
                    .cached_resolved_currency(symbol, CurrencyKind::Trading)
                    .await;
                if let Some(resolved) = trading_cached.as_ref()
                    && self
                        .cached_resolution_is_final(symbol, CurrencyKind::Trading, resolved)
                        .await
                {
                    return Ok(resolved.unit.clone());
                }
                self.resolve_trading_currency_from_hints(
                    symbol,
                    cache_mode,
                    retry_override,
                    trading_cached.as_ref(),
                )
                .await
            }
        }
    }

    async fn resolve_trading_currency_from_hints(
        &self,
        symbol: &str,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
        provisional: Option<&ResolvedCurrency>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        if let Some(unit) = self
            .resolve_first_hint(symbol, CurrencyKind::Trading, &TRADING_CACHED_HINTS)
            .await?
        {
            return Ok(unit);
        }

        self.enrich_quote_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(unit) = self
            .resolve_first_hint(symbol, CurrencyKind::Trading, &TRADING_QUOTE_HINTS)
            .await?
        {
            return Ok(unit);
        }

        let hints = self.cached_currency_hints(symbol).await;
        if let Some(unit) = inference::infer_listing_currency(symbol, &hints) {
            self.store_resolved_currency(
                symbol,
                CurrencyKind::Trading,
                ResolvedCurrency::new(
                    unit.clone(),
                    CurrencySource::ListingHeuristic,
                    EvidenceStrength::ListingHeuristic,
                ),
            )
            .await;
            return Ok(unit);
        }

        self.enrich_profile_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(unit) = self
            .resolve_first_hint(symbol, CurrencyKind::Trading, &TRADING_PROFILE_HINTS)
            .await?
        {
            return Ok(unit);
        }

        if let Some(resolved) = provisional {
            return Ok(resolved.unit.clone());
        }

        Err(YfError::MissingData(format!(
            "unable to resolve trading currency for {symbol}"
        )))
    }

    async fn resolve_reporting_currency_from_hints(
        &self,
        symbol: &str,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
        provisional: Option<&ResolvedCurrency>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        if let Some(unit) = self
            .resolve_first_hint(symbol, CurrencyKind::Reporting, &REPORTING_CACHED_HINTS)
            .await?
        {
            return Ok(unit);
        }

        self.enrich_quote_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(unit) = self
            .resolve_first_hint(symbol, CurrencyKind::Reporting, &REPORTING_QUOTE_HINTS)
            .await?
        {
            return Ok(unit);
        }

        self.enrich_quote_summary_reporting_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(unit) = self
            .resolve_first_hint(
                symbol,
                CurrencyKind::Reporting,
                &REPORTING_QUOTE_SUMMARY_HINTS,
            )
            .await?
        {
            return Ok(unit);
        }

        self.enrich_profile_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(unit) = self
            .resolve_first_hint(symbol, CurrencyKind::Reporting, &REPORTING_PROFILE_HINTS)
            .await?
        {
            return Ok(unit);
        }

        if let Some(resolved) = provisional {
            return Ok(resolved.unit.clone());
        }

        Err(YfError::MissingData(format!(
            "unable to resolve reporting currency for {symbol}"
        )))
    }

    async fn resolve_first_hint(
        &self,
        symbol: &str,
        kind: CurrencyKind,
        evidence: &[HintEvidence],
    ) -> Result<Option<ResolvedCurrencyUnit>, YfError> {
        let hints = self.cached_currency_hints(symbol).await;
        for hint in evidence {
            if let Some(code) = hints.invalid_code(hint.field) {
                return Err(YfError::InvalidData(format!(
                    "invalid {kind:?} currency code for {symbol} in {:?}: {code}",
                    hint.field
                )));
            }

            let unit = hints.unit(hint.field).cloned();

            if let Some(unit) = unit {
                self.store_resolved_currency(
                    symbol,
                    kind,
                    ResolvedCurrency::new(unit.clone(), hint.source, hint.strength),
                )
                .await;
                return Ok(Some(unit));
            }
        }

        Ok(None)
    }

    async fn cached_resolution_is_final(
        &self,
        symbol: &str,
        kind: CurrencyKind,
        resolved: &ResolvedCurrency,
    ) -> bool {
        if resolved.strength.is_provider() {
            return true;
        }

        let hints = self.cached_currency_hints(symbol).await;
        match kind {
            CurrencyKind::Trading | CurrencyKind::CorporateAction => {
                hints.is_missing(CurrencyHintField::Quote)
            }
            CurrencyKind::Reporting | CurrencyKind::AnalystEstimate => {
                hints.is_missing(CurrencyHintField::Financial)
                    && hints.is_missing(CurrencyHintField::QuoteSummaryFinancial)
            }
        }
    }
}

fn direct_currency_unit(
    symbol: &str,
    kind: CurrencyKind,
    evidence: DirectCurrencyEvidence<'_>,
) -> Result<Option<ResolvedCurrencyUnit>, YfError> {
    let Some(code) = evidence.code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    ResolvedCurrencyUnit::from_code(code)
        .map(Some)
        .ok_or_else(|| {
            YfError::InvalidData(format!(
                "invalid {kind:?} currency code for {symbol} from {}: {code}",
                evidence.label
            ))
        })
}
