use super::{CurrencyPurpose, ResolvedCurrency, ResolvedCurrencyUnit};
use crate::core::{ProjectionContext, ProjectionIssue, YfError};

#[derive(Debug)]
pub struct ProjectedCurrency {
    unit: Option<ResolvedCurrencyUnit>,
    issue: Option<ProjectionIssue>,
}

impl ProjectedCurrency {
    const fn resolved(unit: ResolvedCurrencyUnit) -> Self {
        Self {
            unit: Some(unit),
            issue: None,
        }
    }

    const fn omitted(issue: ProjectionIssue) -> Self {
        Self {
            unit: None,
            issue: Some(issue),
        }
    }

    pub fn into_unit(self) -> Option<ResolvedCurrencyUnit> {
        self.unit
    }

    pub const fn issue(&self) -> Option<&ProjectionIssue> {
        self.issue.as_ref()
    }
}

pub fn project_currency_resolution(
    ctx: &mut ProjectionContext,
    symbol: &str,
    purpose: CurrencyPurpose,
    direct_code: Option<&str>,
    resolution: Result<ResolvedCurrency, YfError>,
) -> Result<ProjectedCurrency, YfError> {
    match resolution {
        Ok(resolved) => {
            for invalid in resolved.invalid_evidence() {
                ctx.omitted_present_field(
                    invalid.path(),
                    Some(symbol.to_string()),
                    ProjectionIssue::InvalidCurrency {
                        code: invalid.code().to_string(),
                    },
                )?;
            }
            ctx.currency_resolution(symbol, purpose, &resolved)?;
            Ok(ProjectedCurrency::resolved(resolved.into_unit()))
        }
        Err(err) => Ok(ProjectedCurrency::omitted(currency_resolution_issue(
            direct_code,
            &err,
        ))),
    }
}

fn invalid_direct_currency_issue(code: Option<&str>) -> Option<ProjectionIssue> {
    let code = code.map(str::trim).filter(|code| !code.is_empty())?;

    ResolvedCurrencyUnit::from_code(code)
        .is_none()
        .then(|| ProjectionIssue::InvalidCurrency {
            code: code.to_string(),
        })
}

fn currency_resolution_issue(direct_code: Option<&str>, err: &YfError) -> ProjectionIssue {
    if let Some(issue) = invalid_direct_currency_issue(direct_code) {
        return issue;
    }
    if let Some(issue) = invalid_resolved_currency_issue(err) {
        return issue;
    }

    match err {
        YfError::MissingData(_) => ProjectionIssue::CurrencyUnresolved,
        other => ProjectionIssue::ProviderError {
            message: other.to_string(),
        },
    }
}

fn invalid_resolved_currency_issue(err: &YfError) -> Option<ProjectionIssue> {
    let YfError::InvalidData(message) = err else {
        return None;
    };
    if !message.contains(" currency code ") {
        return None;
    }

    let (_, code) = message.rsplit_once(": ")?;
    Some(ProjectionIssue::InvalidCurrency {
        code: code.to_string(),
    })
}
