use crate::core::{CallOptions, YfClient, YfError, quotesummary};

/* ---------- Single focused fetch with crumb + retry ---------- */

pub(super) async fn fetch_modules_body(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    options: &CallOptions,
) -> Result<String, YfError> {
    quotesummary::fetch_body(client, symbol, modules, "fundamentals", options).await
}

pub(super) fn parse_modules(body: &str) -> Result<super::wire::V10Result<'_>, YfError> {
    quotesummary::parse_module_result(body)
}
