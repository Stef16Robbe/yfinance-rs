use super::YfWarning;

/// Controls how provider data quality issues are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataQuality {
    /// Preserve valid public data and report projection issues as diagnostics.
    #[default]
    BestEffort,
    /// Convert the first projection issue into an error.
    Strict,
}

/// A response paired with adapter-level data-quality diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YfResponse<T> {
    /// The successfully projected public data.
    pub data: T,
    /// Warnings emitted while projecting Yahoo wire data into public models.
    pub diagnostics: YfDiagnostics,
}

impl<T> YfResponse<T> {
    /// Creates a response from data and diagnostics.
    #[must_use]
    pub const fn new(data: T, diagnostics: YfDiagnostics) -> Self {
        Self { data, diagnostics }
    }

    /// Returns `true` when no projection warnings were recorded.
    #[must_use]
    pub const fn is_lossless(&self) -> bool {
        self.diagnostics.is_lossless()
    }

    /// Returns the data, discarding diagnostics.
    #[must_use]
    pub fn into_data(self) -> T {
        self.data
    }

    /// Maps the inner data while preserving diagnostics.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> YfResponse<U> {
        YfResponse {
            data: f(self.data),
            diagnostics: self.diagnostics,
        }
    }
}

/// Diagnostics emitted while projecting Yahoo data into strict public models.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YfDiagnostics {
    /// Projection warnings. An empty list means the response was lossless.
    pub warnings: Vec<YfWarning>,
}

impl YfDiagnostics {
    /// Creates empty diagnostics.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            warnings: Vec::new(),
        }
    }

    /// Returns `true` when no projection warnings were recorded.
    #[must_use]
    pub const fn is_lossless(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Returns the number of warnings recorded.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.warnings.len()
    }

    /// Returns `true` when no warnings were recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    pub(crate) fn push(&mut self, warning: YfWarning) {
        self.warnings.push(warning);
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.warnings.extend(other.warnings);
    }

    pub(crate) fn with_key_prefix(mut self, prefix: &str) -> Self {
        self.warnings = self
            .warnings
            .into_iter()
            .map(|warning| warning.with_key_prefix(prefix))
            .collect();
        self
    }
}
