//! Diagnostics for Yahoo-to-`paft` projection.
//!
//! These types describe adapter-level data quality issues. They intentionally
//! live in `yfinance-rs`, not `paft`, because they describe whether Yahoo wire
//! data was projected losslessly into strict provider-agnostic models.

mod context;
mod currency;
mod issue;
mod monetary;
mod response;
mod warning;

pub(crate) use context::ProjectionContext;
pub use currency::{YfCurrencyKind, YfCurrencySource, YfEvidenceStrength};
pub use issue::ProjectionIssue;
pub(crate) use monetary::{
    optional_money_f64, optional_money_i64, optional_money_u64, optional_price_f64,
};
pub use response::{DataQuality, YfDiagnostics, YfResponse};
pub use warning::YfWarning;
