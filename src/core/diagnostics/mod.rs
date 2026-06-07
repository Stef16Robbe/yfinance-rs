//! Diagnostics for Yahoo-to-`paft` projection.
//!
//! These types describe adapter-level data quality issues. They intentionally
//! live in `yfinance-rs`, not `paft`, because they describe whether Yahoo wire
//! data was projected losslessly into strict provider-agnostic models.

mod context;
mod currency;
mod issue;
mod monetary;
mod numeric;
mod projection;
mod response;
mod warning;

pub(crate) use context::ProjectionContext;
pub use currency::{YfCurrencyInference, YfCurrencyPurpose};
pub use issue::ProjectionIssue;
pub(crate) use monetary::{
    optional_decimal_f64, optional_money_decimal_with_currency_issue,
    optional_money_i64_with_currency_issue, optional_money_u64_with_currency_issue,
    optional_price_f64_with_currency_issue, optional_ratio_f64,
};
pub(crate) use numeric::{optional_u32_from_i64, optional_u32_from_raw_f64};
pub(crate) use projection::{
    WireProjection, diagnostic_key, nonempty, nonempty_string, optional_parsed, optional_projected,
    optional_wire_cloned, optional_wire_value, parse_optional, required_nonempty_string,
    required_parsed, required_period, required_timestamp, required_wire_date, required_wire_value,
};
pub use response::{DataQuality, YfDiagnostics, YfResponse};
pub use warning::YfWarning;
