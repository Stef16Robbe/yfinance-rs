use super::wire::V10Result;
use crate::core::{CallOptions, YfClient, YfError, quotesummary};

/* ---------- Single focused fetch with crumb + retry ---------- */

pub(super) async fn fetch_modules(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    options: &CallOptions,
) -> Result<V10Result, YfError> {
    quotesummary::fetch_module_result(client, symbol, modules, "fundamentals", options).await
}
