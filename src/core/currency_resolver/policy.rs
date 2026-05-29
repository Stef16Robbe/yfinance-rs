use super::{CurrencyKind, ResolvedCurrency, ResolvedCurrencyUnit};
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
    kind: CurrencyKind,
    direct_code: Option<&str>,
    resolution: Result<ResolvedCurrency, YfError>,
) -> Result<ProjectedCurrency, YfError> {
    match resolution {
        Ok(resolved) => {
            ctx.currency_resolution(symbol, kind, &resolved)?;
            Ok(ProjectedCurrency::resolved(resolved.into_unit()))
        }
        Err(err) if invalid_direct_currency_code(direct_code) => Err(err),
        Err(err) => Ok(ProjectedCurrency::omitted(currency_resolution_issue(&err))),
    }
}

fn invalid_direct_currency_code(code: Option<&str>) -> bool {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return false;
    };

    ResolvedCurrencyUnit::from_code(code).is_none()
}

fn currency_resolution_issue(err: &YfError) -> ProjectionIssue {
    match err {
        YfError::MissingData(_) => ProjectionIssue::CurrencyUnresolved,
        other => ProjectionIssue::ProviderError {
            message: other.to_string(),
        },
    }
}
