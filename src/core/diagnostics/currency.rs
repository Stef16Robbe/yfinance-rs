use std::fmt;

/// Currency purpose used by diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YfCurrencyPurpose {
    /// Trading currency for quoted prices.
    Trading,
    /// Reporting currency for statements and ownership totals.
    Reporting,
    /// Currency for dividends and capital gains.
    CorporateAction,
    /// Currency for analyst estimates and price targets.
    AnalystEstimate,
}

impl fmt::Display for YfCurrencyPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Trading => "trading",
            Self::Reporting => "reporting",
            Self::CorporateAction => "corporate-action",
            Self::AnalystEstimate => "analyst-estimate",
        };
        f.write_str(value)
    }
}

/// Heuristic used to infer a currency when Yahoo did not provide a usable currency field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YfCurrencyInference {
    /// Currency inferred from Yahoo symbol/listing or exchange metadata.
    ListingHeuristic,
    /// Currency inferred from a profile country mapping.
    ProfileCountryHeuristic,
}

impl fmt::Display for YfCurrencyInference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ListingHeuristic => "listing heuristic",
            Self::ProfileCountryHeuristic => "profile-country heuristic",
        };
        f.write_str(value)
    }
}

impl From<crate::core::currency_resolver::CurrencyPurpose> for YfCurrencyPurpose {
    fn from(value: crate::core::currency_resolver::CurrencyPurpose) -> Self {
        match value {
            crate::core::currency_resolver::CurrencyPurpose::Trading => Self::Trading,
            crate::core::currency_resolver::CurrencyPurpose::Reporting => Self::Reporting,
            crate::core::currency_resolver::CurrencyPurpose::CorporateAction => {
                Self::CorporateAction
            }
            crate::core::currency_resolver::CurrencyPurpose::AnalystEstimate => {
                Self::AnalystEstimate
            }
        }
    }
}

impl From<crate::core::currency_resolver::CurrencyInference> for YfCurrencyInference {
    fn from(value: crate::core::currency_resolver::CurrencyInference) -> Self {
        match value {
            crate::core::currency_resolver::CurrencyInference::ListingHeuristic => {
                Self::ListingHeuristic
            }
            crate::core::currency_resolver::CurrencyInference::ProfileCountryHeuristic => {
                Self::ProfileCountryHeuristic
            }
        }
    }
}
