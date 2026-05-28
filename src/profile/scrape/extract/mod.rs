mod helpers;
mod strategies;

#[cfg(feature = "tracing")]
use helpers::truncate;
use strategies::{
    try_generic_json_scripts, try_quote_summary_store_literal, try_root_app_main,
    try_sveltekit_json,
};

pub fn extract_bootstrap_json(body: &str) -> Result<String, crate::YfError> {
    crate::core::logging::trace_debug!(
        body_len = body.len(),
        "starting profile bootstrap JSON extraction"
    );

    /* Strategy A: legacy root.App.main = {...}; */
    crate::core::logging::trace_debug!("trying profile bootstrap Strategy A: root.App.main");
    if let Some(json_str) = try_root_app_main(body) {
        crate::core::logging::trace_debug!(
            json_len = json_str.len(),
            preview = %truncate(&json_str, 160),
            "profile bootstrap Strategy A matched"
        );
        return Ok(json_str);
    }

    /* Strategy B: literal "QuoteSummaryStore": { ... } object */
    crate::core::logging::trace_debug!(
        "trying profile bootstrap Strategy B: QuoteSummaryStore literal"
    );
    if let Some(wrapped) = try_quote_summary_store_literal(body) {
        crate::core::logging::trace_debug!(
            wrapped_len = wrapped.len(),
            preview = %truncate(&wrapped, 160),
            "profile bootstrap Strategy B matched"
        );
        return Ok(wrapped);
    }

    /* Strategy C: SvelteKit data-sveltekit-fetched blobs. */
    crate::core::logging::trace_debug!("trying profile bootstrap Strategy C: SvelteKit JSON");
    if let Some(wrapped) = try_sveltekit_json(body) {
        crate::core::logging::trace_debug!(
            wrapped_len = wrapped.len(),
            preview = %truncate(&wrapped, 160),
            "profile bootstrap Strategy C matched"
        );
        return Ok(wrapped);
    }

    /* Strategy D: generic scan of all application/json scripts */
    crate::core::logging::trace_debug!("trying profile bootstrap Strategy D: generic JSON scan");
    if let Some(wrapped) = try_generic_json_scripts(body) {
        crate::core::logging::trace_debug!(
            wrapped_len = wrapped.len(),
            preview = %truncate(&wrapped, 160),
            "profile bootstrap Strategy D matched"
        );
        return Ok(wrapped);
    }

    crate::core::logging::trace_debug!("profile bootstrap extraction strategies exhausted");
    Err(crate::YfError::MissingData("bootstrap not found".into()))
}
