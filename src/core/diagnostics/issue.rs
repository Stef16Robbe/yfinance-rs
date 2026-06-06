use std::fmt;

/// Why provider data could not be represented losslessly.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionIssue {
    /// A required provider field was absent or null.
    MissingRequiredField {
        /// Field name.
        field: &'static str,
    },
    /// One or more required provider fields were absent or null.
    MissingRequiredFields {
        /// Field names.
        fields: Vec<&'static str>,
    },
    /// A provider field was present but malformed.
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Additional details.
        details: String,
    },
    /// A present value could not be represented by the target public type.
    ConversionFailed {
        /// Target public type or field.
        target: &'static str,
    },
    /// A monetary value was present, but no currency could be resolved.
    CurrencyUnresolved,
    /// A provider currency code was present but invalid.
    InvalidCurrency {
        /// Raw provider currency code.
        code: String,
    },
    /// Yahoo returned an API or provider-level error.
    ProviderError {
        /// Error details.
        message: String,
    },
    /// Yahoo did not return a requested module or feature.
    ProviderUnavailable {
        /// Feature or module name.
        feature: &'static str,
    },
}

impl fmt::Display for ProjectionIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredField { field } => write!(f, "missing required field {field}"),
            Self::MissingRequiredFields { fields } => {
                write!(f, "missing required fields {}", fields.join(", "))
            }
            Self::InvalidField { field, details } => write!(f, "invalid field {field}: {details}"),
            Self::ConversionFailed { target } => write!(f, "conversion failed for {target}"),
            Self::CurrencyUnresolved => f.write_str("currency unresolved"),
            Self::InvalidCurrency { code } => write!(f, "invalid currency code {code}"),
            Self::ProviderError { message } => write!(f, "provider error: {message}"),
            Self::ProviderUnavailable { feature } => {
                write!(f, "provider did not return {feature}")
            }
        }
    }
}
