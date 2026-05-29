use crate::core::{
    ProjectionIssue, YfDiagnostics, YfError, YfResponse, YfWarning,
    currency_resolver::{CurrencyKind, ResolvedCurrency},
    diagnostics::{DataQuality, YfCurrencyKind, YfCurrencySource, YfEvidenceStrength},
};

#[derive(Debug, Clone)]
pub struct ProjectionContext {
    endpoint: &'static str,
    policy: DataQuality,
    diagnostics: YfDiagnostics,
}

impl ProjectionContext {
    pub(crate) const fn new(endpoint: &'static str, policy: DataQuality) -> Self {
        Self {
            endpoint,
            policy,
            diagnostics: YfDiagnostics::new(),
        }
    }

    pub(crate) const fn policy(&self) -> DataQuality {
        self.policy
    }

    pub(crate) fn record(&mut self, warning: YfWarning) -> Result<(), YfError> {
        match self.policy {
            DataQuality::BestEffort => {
                self.diagnostics.push(warning);
                Ok(())
            }
            DataQuality::Strict => Err(YfError::DataQuality(Box::new(warning))),
        }
    }

    pub(crate) fn dropped_item(
        &mut self,
        item: &'static str,
        key: Option<String>,
        reason: ProjectionIssue,
    ) -> Result<(), YfError> {
        self.record(YfWarning::DroppedItem {
            endpoint: self.endpoint,
            item,
            key,
            reason,
        })
    }

    pub(crate) fn omitted_present_field(
        &mut self,
        path: &'static str,
        key: Option<String>,
        reason: ProjectionIssue,
    ) -> Result<(), YfError> {
        self.record(YfWarning::OmittedPresentField {
            endpoint: self.endpoint,
            path,
            key,
            reason,
        })
    }

    pub(crate) fn coerced_present_field(
        &mut self,
        path: &'static str,
        key: Option<String>,
        coercion: String,
    ) -> Result<(), YfError> {
        self.record(YfWarning::CoercedPresentField {
            endpoint: self.endpoint,
            path,
            key,
            coercion,
        })
    }

    pub(crate) fn suppressed_error(
        &mut self,
        operation: &'static str,
        error: &YfError,
    ) -> Result<(), YfError> {
        self.record(YfWarning::SuppressedError {
            endpoint: self.endpoint,
            operation,
            error: error.to_string(),
        })
    }

    pub(crate) fn provider_feature_unavailable(
        &mut self,
        feature: &'static str,
        reason: ProjectionIssue,
    ) -> Result<(), YfError> {
        self.record(YfWarning::ProviderFeatureUnavailable {
            endpoint: self.endpoint,
            feature,
            reason,
        })
    }

    pub(crate) fn unavailable_feature(&mut self, feature: &'static str) -> Result<(), YfError> {
        self.provider_feature_unavailable(feature, ProjectionIssue::ProviderUnavailable { feature })
    }

    pub(crate) fn repaired_data(
        &mut self,
        item: &'static str,
        key: Option<String>,
        repair: &'static str,
    ) -> Result<(), YfError> {
        self.record(YfWarning::RepairedData {
            endpoint: self.endpoint,
            item,
            key,
            repair,
        })
    }

    pub(crate) fn currency_resolution(
        &mut self,
        symbol: &str,
        kind: CurrencyKind,
        resolved: &ResolvedCurrency,
    ) -> Result<(), YfError> {
        if resolved.source().is_direct_provider() {
            return Ok(());
        }
        self.record(YfWarning::CurrencyInferred {
            endpoint: self.endpoint,
            symbol: symbol.to_string(),
            kind: YfCurrencyKind::from(kind),
            source: YfCurrencySource::from(resolved.source()),
            strength: YfEvidenceStrength::from(resolved.strength()),
        })
    }

    pub(crate) fn extend(&mut self, diagnostics: YfDiagnostics) {
        self.diagnostics.extend(diagnostics);
    }

    pub(crate) fn finish<T>(self, data: T) -> YfResponse<T> {
        YfResponse::new(data, self.diagnostics)
    }
}
