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
    pub(crate) async fn resolve_trading_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: TradingCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyKind::Trading,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_trading_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: TradingCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_trading_currency(
            symbol,
            override_currency,
            evidence,
            cache_mode,
            retry_override,
        )
        .await
        .map(ResolvedCurrency::into_unit)
    }

    pub(crate) async fn resolve_reporting_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: ReportingCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyKind::Reporting,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_reporting_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: ReportingCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_reporting_currency(
            symbol,
            override_currency,
            evidence,
            cache_mode,
            retry_override,
        )
        .await
        .map(ResolvedCurrency::into_unit)
    }

    pub(crate) async fn resolve_corporate_action_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: CorporateActionCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyKind::CorporateAction,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_corporate_action_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: CorporateActionCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_corporate_action_currency(
            symbol,
            override_currency,
            evidence,
            cache_mode,
            retry_override,
        )
        .await
        .map(ResolvedCurrency::into_unit)
    }

    pub(crate) async fn resolve_analyst_estimate_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: AnalystEstimateCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyKind::AnalystEstimate,
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label()),
            cache_mode,
            retry_override,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_analyst_estimate_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: AnalystEstimateCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_analyst_estimate_currency(
            symbol,
            override_currency,
            evidence,
            cache_mode,
            retry_override,
        )
        .await
        .map(ResolvedCurrency::into_unit)
    }

    async fn resolve_currency_from_evidence(
        &self,
        symbol: &str,
        kind: CurrencyKind,
        override_currency: Option<Currency>,
        direct_evidence: DirectCurrencyEvidence<'_>,
        cache_mode: CacheMode,
        retry_override: Option<&RetryConfig>,
    ) -> Result<ResolvedCurrency, YfError> {
        if let Some(currency) = override_currency {
            return Ok(ResolvedCurrency::new(
                override_currency_unit(symbol, kind, currency)?,
                CurrencySource::Override,
                EvidenceStrength::Override,
            ));
        }

        if let Some(unit) = direct_currency_unit(symbol, kind, direct_evidence)? {
            let resolved = ResolvedCurrency::new(
                unit,
                CurrencySource::DirectProvider,
                EvidenceStrength::DirectProvider,
            );
            if kind.caches_direct_provider() {
                self.store_resolved_currency(symbol, kind, resolved.clone())
                    .await;
            }
            return Ok(resolved);
        }

        let cached = self.cached_resolved_currency(symbol, kind).await;
        if let Some(resolved) = cached.as_ref()
            && self
                .cached_resolution_is_final(symbol, kind, resolved)
                .await
        {
            return Ok(resolved.clone());
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
                    return Ok(if matches!(kind, CurrencyKind::AnalystEstimate) {
                        cross_kind_cached_resolution(resolved)
                    } else {
                        resolved.clone()
                    });
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
                    return Ok(cross_kind_cached_resolution(resolved));
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
    ) -> Result<ResolvedCurrency, YfError> {
        if let Some(resolved) = self
            .resolve_first_hint(symbol, CurrencyKind::Trading, &TRADING_CACHED_HINTS)
            .await?
        {
            return Ok(resolved);
        }

        self.enrich_quote_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(resolved) = self
            .resolve_first_hint(symbol, CurrencyKind::Trading, &TRADING_QUOTE_HINTS)
            .await?
        {
            return Ok(resolved);
        }

        let hints = self.cached_currency_hints(symbol).await;
        if let Some(unit) = inference::infer_listing_currency(symbol, &hints) {
            let resolved = ResolvedCurrency::new(
                unit,
                CurrencySource::ListingHeuristic,
                EvidenceStrength::ListingHeuristic,
            );
            self.store_resolved_currency(symbol, CurrencyKind::Trading, resolved.clone())
                .await;
            return Ok(resolved);
        }

        self.enrich_profile_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(resolved) = self
            .resolve_first_hint(symbol, CurrencyKind::Trading, &TRADING_PROFILE_HINTS)
            .await?
        {
            return Ok(resolved);
        }

        if let Some(resolved) = provisional {
            return Ok(resolved.clone());
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
    ) -> Result<ResolvedCurrency, YfError> {
        if let Some(resolved) = self
            .resolve_first_hint(symbol, CurrencyKind::Reporting, &REPORTING_CACHED_HINTS)
            .await?
        {
            return Ok(resolved);
        }

        self.enrich_quote_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(resolved) = self
            .resolve_first_hint(symbol, CurrencyKind::Reporting, &REPORTING_QUOTE_HINTS)
            .await?
        {
            return Ok(resolved);
        }

        self.enrich_quote_summary_reporting_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                CurrencyKind::Reporting,
                &REPORTING_QUOTE_SUMMARY_HINTS,
            )
            .await?
        {
            return Ok(resolved);
        }

        self.enrich_profile_hints(symbol, cache_mode, retry_override)
            .await;
        if let Some(resolved) = self
            .resolve_first_hint(symbol, CurrencyKind::Reporting, &REPORTING_PROFILE_HINTS)
            .await?
        {
            return Ok(resolved);
        }

        if let Some(resolved) = provisional {
            return Ok(resolved.clone());
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
    ) -> Result<Option<ResolvedCurrency>, YfError> {
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
                let resolved = ResolvedCurrency::new(unit, hint.source, hint.strength);
                self.store_resolved_currency(symbol, kind, resolved.clone())
                    .await;
                return Ok(Some(resolved));
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

fn override_currency_unit(
    symbol: &str,
    kind: CurrencyKind,
    currency: Currency,
) -> Result<ResolvedCurrencyUnit, YfError> {
    currency.decimal_places().map_err(|err| {
        YfError::InvalidParams(format!(
            "invalid {kind:?} currency override for {symbol}: {err}"
        ))
    })?;
    Ok(ResolvedCurrencyUnit::from_currency(currency))
}

fn cross_kind_cached_resolution(resolved: &ResolvedCurrency) -> ResolvedCurrency {
    if resolved.source() == CurrencySource::DirectProvider {
        ResolvedCurrency::new(
            resolved.unit.clone(),
            CurrencySource::CachedProvider,
            resolved.strength(),
        )
    } else {
        resolved.clone()
    }
}
