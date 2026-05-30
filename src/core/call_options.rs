use crate::core::{
    DataQuality,
    client::{CacheMode, RetryConfig},
};

/// Per-call behavior shared by builders and internal fetch helpers.
#[derive(Clone, Debug)]
pub struct CallOptions {
    pub(crate) cache_mode: CacheMode,
    retry_override: Option<RetryConfig>,
    pub(crate) data_quality: DataQuality,
}

impl CallOptions {
    pub(crate) const fn new() -> Self {
        Self {
            cache_mode: CacheMode::Default,
            retry_override: None,
            data_quality: DataQuality::BestEffort,
        }
    }

    pub(crate) const fn cache_mode(&self) -> CacheMode {
        self.cache_mode
    }

    pub(crate) const fn retry_override(&self) -> Option<&RetryConfig> {
        self.retry_override.as_ref()
    }

    pub(crate) const fn data_quality(&self) -> DataQuality {
        self.data_quality
    }

    pub(crate) const fn with_cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    pub(crate) fn with_retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.retry_override = cfg;
        self
    }

    pub(crate) const fn with_data_quality(mut self, policy: DataQuality) -> Self {
        self.data_quality = policy;
        self
    }

    pub(crate) const fn strict(self) -> Self {
        self.with_data_quality(DataQuality::Strict)
    }
}

impl Default for CallOptions {
    fn default() -> Self {
        Self::new()
    }
}
