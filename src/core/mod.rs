//! Core components of the `yfinance-rs` client.
//!
//! This module contains the foundational building blocks of the library, including:
//! - The main [`YfClient`] and its builder.
//! - The primary [`YfError`] type.
//! - Shared data models like [`Quote`] and [`Candle`].
//! - Internal networking and authentication logic.

/// The main client (`YfClient`), builder, and configuration.
pub(crate) mod call_options;
pub mod client;
pub(crate) mod currency;
pub(crate) mod currency_resolver;
/// Provider projection diagnostics.
pub mod diagnostics;
/// The primary error type (`YfError`) for the crate.
pub mod error;
pub(crate) mod logging;
/// Shared data models used across multiple API modules (e.g., `Quote`, `Candle`).
pub mod models;
pub(crate) mod quotes;
pub(crate) mod quotesummary;
pub(crate) mod redaction;
/// Service traits for abstracting functionality like history fetching.
pub mod services;
pub(crate) mod wire;
#[doc(hidden)]
pub mod yahoo_vocab;

#[cfg(feature = "test-mode")]
pub(crate) mod fixtures;

#[doc(hidden)]
pub mod conversions;
pub(crate) mod net;

// convenient re-exports so most code can just `use crate::core::YfClient`
pub(crate) use call_options::{CallOptions, impl_call_option_setters};
pub use client::{Backoff, CacheEndpoint, CacheMode, RetryConfig, YfClient, YfClientBuilder};
pub(crate) use diagnostics::ProjectionContext;
pub use diagnostics::{
    DataQuality, ProjectionIssue, YfCurrencyInference, YfCurrencyPurpose, YfDiagnostics,
    YfResponse, YfWarning,
};
pub use error::YfError;
pub use models::{
    Action, AdjustmentAnchor, AdjustmentMethod, Candle, CorporateActionAdjustmentCause,
    CorporateActionAdjustmentCauses, FastInfo, HistoryMeta, HistoryResponse, Interval,
    MovingAverages, OhlcPriceBasis, PriceBasis, Quote, Range, Snapshot,
};
pub use services::{HistoryRequest, HistoryService};
