use super::{
    AnalystEstimateCurrencyEvidence, CorporateActionCurrencyEvidence, CurrencyCacheKind,
    CurrencyPurpose, CurrencyResolutionMode, CurrencyResolutionSpec, DirectCurrencyCache,
    DirectCurrencyField, ReportingCurrencyEvidence, ResolvedCurrency, ResolvedCurrencyUnit,
    TradingCurrencyEvidence, hints::CurrencyHintField, inference, types::InvalidCurrencyEvidence,
};
use crate::core::{CallOptions, YfClient, YfError};
use paft::money::Currency;

#[derive(Clone, Copy)]
struct HintEvidence {
    field: CurrencyHintField,
}

impl HintEvidence {
    const fn new(field: CurrencyHintField) -> Self {
        Self { field }
    }
}

#[derive(Clone, Copy)]
struct DirectCurrencyEvidence<'a> {
    code: Option<&'a str>,
    label: &'static str,
    field: Option<DirectCurrencyField>,
}

impl<'a> DirectCurrencyEvidence<'a> {
    const fn new(
        code: Option<&'a str>,
        label: &'static str,
        field: Option<DirectCurrencyField>,
    ) -> Self {
        Self { code, label, field }
    }
}

const TRADING_QUOTE_HINTS: [HintEvidence; 1] = [HintEvidence::new(CurrencyHintField::Quote)];
const TRADING_PROFILE_HINTS: [HintEvidence; 1] =
    [HintEvidence::new(CurrencyHintField::ProfileCountry)];
const REPORTING_CACHED_HINTS: [HintEvidence; 2] = [
    HintEvidence::new(CurrencyHintField::Financial),
    HintEvidence::new(CurrencyHintField::QuoteSummaryFinancial),
];
const REPORTING_QUOTE_HINTS: [HintEvidence; 1] = [HintEvidence::new(CurrencyHintField::Financial)];
const REPORTING_QUOTE_SUMMARY_HINTS: [HintEvidence; 1] =
    [HintEvidence::new(CurrencyHintField::QuoteSummaryFinancial)];
const REPORTING_PROFILE_HINTS: [HintEvidence; 1] =
    [HintEvidence::new(CurrencyHintField::ProfileCountry)];

impl YfClient {
    pub(crate) async fn resolve_trading_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: TradingCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyResolutionSpec::trading(),
            override_currency,
            DirectCurrencyEvidence::new(evidence.direct_code(), evidence.label(), evidence.field()),
            options,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_trading_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: TradingCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_trading_currency(symbol, override_currency, evidence, options)
            .await
            .map(ResolvedCurrency::into_unit)
    }

    pub(crate) async fn resolve_reporting_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: ReportingCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyResolutionSpec::reporting(),
            override_currency,
            DirectCurrencyEvidence::new(
                evidence.direct_code(),
                evidence.label(),
                Some(evidence.field()),
            ),
            options,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_reporting_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: ReportingCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_reporting_currency(symbol, override_currency, evidence, options)
            .await
            .map(ResolvedCurrency::into_unit)
    }

    pub(crate) async fn resolve_corporate_action_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: CorporateActionCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyResolutionSpec::corporate_action(),
            override_currency,
            DirectCurrencyEvidence::new(
                evidence.direct_code(),
                evidence.label(),
                Some(evidence.field()),
            ),
            options,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_corporate_action_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: CorporateActionCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_corporate_action_currency(symbol, override_currency, evidence, options)
            .await
            .map(ResolvedCurrency::into_unit)
    }

    pub(crate) async fn resolve_analyst_estimate_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: AnalystEstimateCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyResolutionSpec::analyst_estimate(),
            override_currency,
            DirectCurrencyEvidence::new(
                evidence.direct_code(),
                evidence.label(),
                Some(evidence.field()),
            ),
            options,
        )
        .await
    }

    pub(crate) async fn resolve_analyst_price_target_currency(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrency, YfError> {
        self.resolve_currency_from_evidence(
            symbol,
            CurrencyResolutionSpec::analyst_price_target(),
            override_currency,
            DirectCurrencyEvidence::new(None, "none", None),
            options,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resolve_analyst_estimate_currency_unit(
        &self,
        symbol: &str,
        override_currency: Option<Currency>,
        evidence: AnalystEstimateCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrencyUnit, YfError> {
        self.resolve_analyst_estimate_currency(symbol, override_currency, evidence, options)
            .await
            .map(ResolvedCurrency::into_unit)
    }

    async fn resolve_currency_from_evidence(
        &self,
        symbol: &str,
        spec: CurrencyResolutionSpec,
        override_currency: Option<Currency>,
        direct_evidence: DirectCurrencyEvidence<'_>,
        options: &CallOptions,
    ) -> Result<ResolvedCurrency, YfError> {
        if let Some(currency) = override_currency {
            return Ok(ResolvedCurrency::override_currency(override_currency_unit(
                symbol,
                spec.purpose(),
                currency,
            )?));
        }

        if let Some(unit) = direct_currency_unit(symbol, spec.purpose(), direct_evidence)? {
            let field = direct_evidence.field.ok_or_else(|| {
                YfError::InvalidData(format!(
                    "direct {:?} currency for {symbol} has no provider field",
                    spec.purpose()
                ))
            })?;
            let resolved = ResolvedCurrency::direct_provider(unit, field);
            if let DirectCurrencyCache::Store(cache_kind) = spec.direct_cache() {
                self.store_resolved_currency(symbol, cache_kind, resolved.clone())
                    .await;
            }
            return Ok(resolved);
        }

        let cache_kind = spec.mode().cache_kind();
        let cached = self.cached_resolved_currency(symbol, cache_kind).await;
        if let Some(resolved) = cached.as_ref()
            && self
                .cached_resolution_is_final(symbol, spec.mode(), resolved)
                .await
        {
            return Ok(resolved.clone());
        }

        match spec.mode() {
            CurrencyResolutionMode::TradingLike => {
                self.resolve_trading_currency_from_hints(symbol, spec, options, cached.as_ref())
                    .await
            }
            CurrencyResolutionMode::ReportingLike => {
                self.resolve_reporting_currency_from_hints(symbol, spec, options, cached.as_ref())
                    .await
            }
        }
    }

    async fn resolve_trading_currency_from_hints(
        &self,
        symbol: &str,
        spec: CurrencyResolutionSpec,
        options: &CallOptions,
        provisional: Option<&ResolvedCurrency>,
    ) -> Result<ResolvedCurrency, YfError> {
        let mut invalid_evidence = Vec::new();

        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &TRADING_QUOTE_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        self.enrich_quote_hints(symbol, options).await;
        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &TRADING_QUOTE_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        let hints = self.cached_currency_hints(symbol).await;
        if let Some(unit) = inference::infer_listing_currency(symbol, &hints) {
            let resolved = ResolvedCurrency::listing_heuristic(unit)
                .with_invalid_evidence(std::mem::take(&mut invalid_evidence));
            self.store_resolved_currency(symbol, spec.mode().cache_kind(), resolved.clone())
                .await;
            return Ok(resolved);
        }

        self.enrich_profile_hints(symbol, options).await;
        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &TRADING_PROFILE_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        if let Some(resolved) = provisional {
            return Ok(resolved
                .clone()
                .with_invalid_evidence(std::mem::take(&mut invalid_evidence)));
        }

        if let Some(invalid) = invalid_evidence.first() {
            return Err(invalid_currency_evidence_error(
                symbol,
                spec.purpose(),
                invalid,
            ));
        }

        Err(YfError::MissingData(format!(
            "unable to resolve trading currency for {symbol}"
        )))
    }

    async fn resolve_reporting_currency_from_hints(
        &self,
        symbol: &str,
        spec: CurrencyResolutionSpec,
        options: &CallOptions,
        provisional: Option<&ResolvedCurrency>,
    ) -> Result<ResolvedCurrency, YfError> {
        let mut invalid_evidence = Vec::new();

        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &REPORTING_CACHED_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        self.enrich_quote_hints(symbol, options).await;
        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &REPORTING_QUOTE_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        self.enrich_quote_summary_reporting_hints(symbol, options)
            .await;
        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &REPORTING_QUOTE_SUMMARY_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        self.enrich_profile_hints(symbol, options).await;
        if let Some(resolved) = self
            .resolve_first_hint(
                symbol,
                spec.mode().cache_kind(),
                &REPORTING_PROFILE_HINTS,
                &mut invalid_evidence,
            )
            .await
        {
            return Ok(resolved);
        }

        if let Some(resolved) = provisional {
            return Ok(resolved
                .clone()
                .with_invalid_evidence(std::mem::take(&mut invalid_evidence)));
        }

        if let Some(invalid) = invalid_evidence.first() {
            return Err(invalid_currency_evidence_error(
                symbol,
                spec.purpose(),
                invalid,
            ));
        }

        Err(YfError::MissingData(format!(
            "unable to resolve reporting currency for {symbol}"
        )))
    }

    async fn resolve_first_hint(
        &self,
        symbol: &str,
        cache_kind: CurrencyCacheKind,
        evidence: &[HintEvidence],
        invalid_evidence: &mut Vec<InvalidCurrencyEvidence>,
    ) -> Option<ResolvedCurrency> {
        let hints = self.cached_currency_hints(symbol).await;
        for hint in evidence {
            if let Some(code) = hints.invalid_code(hint.field) {
                push_invalid_currency_evidence(invalid_evidence, hint.field, code);
                continue;
            }

            let value = hints.value(hint.field);

            if let Some(value) = value {
                let resolved = value
                    .resolved_currency()
                    .with_invalid_evidence(std::mem::take(invalid_evidence));
                self.store_resolved_currency(symbol, cache_kind, resolved.clone())
                    .await;
                return Some(resolved);
            }
        }

        None
    }

    async fn cached_resolution_is_final(
        &self,
        symbol: &str,
        mode: CurrencyResolutionMode,
        resolved: &ResolvedCurrency,
    ) -> bool {
        if resolved.is_trusted() {
            return true;
        }

        let hints = self.cached_currency_hints(symbol).await;
        match mode {
            CurrencyResolutionMode::TradingLike => hints.is_missing(CurrencyHintField::Quote),
            CurrencyResolutionMode::ReportingLike => {
                hints.is_missing(CurrencyHintField::Financial)
                    && hints.is_missing(CurrencyHintField::QuoteSummaryFinancial)
            }
        }
    }
}

fn direct_currency_unit(
    symbol: &str,
    purpose: CurrencyPurpose,
    evidence: DirectCurrencyEvidence<'_>,
) -> Result<Option<ResolvedCurrencyUnit>, YfError> {
    let Some(code) = evidence.code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    ResolvedCurrencyUnit::from_code(code)
        .map(Some)
        .ok_or_else(|| {
            YfError::InvalidData(format!(
                "invalid {purpose:?} currency code for {symbol} from {}: {code}",
                evidence.label
            ))
        })
}

fn override_currency_unit(
    symbol: &str,
    purpose: CurrencyPurpose,
    currency: Currency,
) -> Result<ResolvedCurrencyUnit, YfError> {
    currency.decimal_places().map_err(|err| {
        YfError::InvalidParams(format!(
            "invalid {purpose:?} currency override for {symbol}: {err}"
        ))
    })?;
    Ok(ResolvedCurrencyUnit::from_currency(currency))
}

fn invalid_currency_evidence_error(
    symbol: &str,
    purpose: CurrencyPurpose,
    invalid: &InvalidCurrencyEvidence,
) -> YfError {
    YfError::InvalidData(format!(
        "invalid {purpose:?} currency code for {symbol} in {}: {}",
        invalid.path(),
        invalid.code()
    ))
}

fn push_invalid_currency_evidence(
    invalid_evidence: &mut Vec<InvalidCurrencyEvidence>,
    field: CurrencyHintField,
    code: &str,
) {
    let path = field.provider_path();
    if invalid_evidence
        .iter()
        .any(|invalid| invalid.path() == path && invalid.code() == code)
    {
        return;
    }

    invalid_evidence.push(InvalidCurrencyEvidence::new(path, code));
}
