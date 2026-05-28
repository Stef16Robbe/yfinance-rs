use serde_json::Value;

#[cfg(feature = "tracing")]
use super::helpers::truncate;
use super::helpers::{
    extract_store_like_from_quote_summary_value, find_quote_summary_store_in_value,
    find_quote_summary_value_in_value, normalize_store_like, wrap_store_like,
};
use crate::profile::scrape::utils::{find_matching_brace, iter_json_scripts};

/// Strategy A: look for `root.App.main = {...};`
pub fn try_root_app_main(body: &str) -> Option<String> {
    if let Some(start) = body.find("root.App.main") {
        let after = &body[start..];
        if let Some(eq) = after.find('=') {
            let mut payload = &after[eq + 1..];
            payload = payload.trim_start();
            let end_script = payload.find("</script>").unwrap_or(payload.len());
            let segment = &payload[..end_script];
            if let Some(semi) = segment.rfind(';') {
                let json_str = segment[..semi].trim();
                crate::core::logging::trace_debug!(
                    preview = %truncate(json_str, 160),
                    "profile bootstrap Strategy A preview"
                );
                return Some(json_str.to_string());
            }
        }
    }
    None
}

/// Strategy B: find literal `"QuoteSummaryStore" : { ... }` and wrap it.
pub fn try_quote_summary_store_literal(body: &str) -> Option<String> {
    let key = "\"QuoteSummaryStore\"";
    if let Some(pos) = body.find(key) {
        let after = &body[pos + key.len()..];
        if let Some(brace_rel) = after.find('{') {
            let obj_start = pos + key.len() + brace_rel;
            if let Some(obj_end) = find_matching_brace(body, obj_start) {
                let obj = &body[obj_start..=obj_end];
                let wrapped = format!(
                    r#"{{"context":{{"dispatcher":{{"stores":{{"QuoteSummaryStore":{obj}}}}}}}}}"#
                );
                crate::core::logging::trace_debug!(
                    object_len = obj.len(),
                    preview = %truncate(obj, 160),
                    "profile bootstrap Strategy B object preview"
                );
                return Some(wrapped);
            }
            crate::core::logging::trace_debug!(
                "profile bootstrap Strategy B found start but failed to match closing brace"
            );
        }
    }
    None
}

/// Strategy C: scan `SvelteKit` `data-sveltekit-fetched` JSON blobs.
#[allow(clippy::too_many_lines)]
#[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
pub fn try_sveltekit_json(body: &str) -> Option<String> {
    let scripts = iter_json_scripts(body);

    crate::core::logging::trace_debug!(
        script_count = scripts.len(),
        "profile bootstrap Strategy C inspecting JSON scripts"
    );

    for (i, (tag_attrs, inner_json)) in scripts.iter().enumerate() {
        let is_svelte = tag_attrs.contains("data-sveltekit-fetched");
        if !is_svelte {
            continue;
        }

        crate::core::logging::trace_debug!(
            index = i,
            attrs = %truncate(tag_attrs, 160),
            inner_len = inner_json.len(),
            preview = %truncate(inner_json, 120),
            "profile bootstrap Strategy C script preview"
        );

        // Case C1: array of objects having nodes[].data
        if let Ok(outer_array) = serde_json::from_str::<Vec<Value>>(inner_json) {
            crate::core::logging::trace_debug!(
                index = i,
                array_len = outer_array.len(),
                "profile bootstrap Strategy C parsed script as array"
            );
            for (ai, outer_obj) in outer_array.into_iter().enumerate() {
                if let Some(nodes) = outer_obj.get("nodes").and_then(|n| n.as_array()) {
                    crate::core::logging::trace_debug!(
                        index = i,
                        array_index = ai,
                        nodes_len = nodes.len(),
                        "profile bootstrap Strategy C found nodes"
                    );
                    for (ni, node) in nodes.iter().enumerate() {
                        if let Some(data) = node.get("data")
                            && let Some(store_like) =
                                extract_store_like_from_quote_summary_value(data)
                            && let Ok(wrapped) = wrap_store_like(&store_like)
                        {
                            crate::core::logging::trace_debug!(
                                index = i,
                                array_index = ai,
                                node_index = ni,
                                wrapped_len = wrapped.len(),
                                "profile bootstrap Strategy C matched nodes data"
                            );
                            return Some(wrapped);
                        }
                    }
                }
            }
        }

        // Case C2: object with "body" either JSON string or inline JSON
        let parsed_obj = match serde_json::from_str::<Value>(inner_json) {
            Ok(v @ Value::Object(_)) => Some(v),
            Ok(_) => None,
            Err(e) => {
                crate::core::logging::trace_debug!(
                    index = i,
                    error = %e,
                    "profile bootstrap Strategy C parse as object failed"
                );
                None
            }
        };

        if let Some(mut outer_obj) = parsed_obj {
            let body_val_opt = { outer_obj.get_mut("body").map(serde_json::Value::take) };

            if let Some(body_val) = body_val_opt {
                let payload_opt = match body_val {
                    Value::String(s) => serde_json::from_str::<Value>(&s).ok(),
                    Value::Object(_) | Value::Array(_) => Some(body_val),
                    _ => None,
                };

                if let Some(payload) = payload_opt {
                    if let Some(qss) = find_quote_summary_store_in_value(&payload) {
                        let store_like = normalize_store_like(qss.clone());
                        if let Ok(wrapped) = wrap_store_like(&store_like) {
                            crate::core::logging::trace_debug!(
                                index = i,
                                wrapped_len = wrapped.len(),
                                "profile bootstrap Strategy C matched QuoteSummaryStore path"
                            );
                            return Some(wrapped);
                        }
                    }

                    if let Some(qs_val) = find_quote_summary_value_in_value(&payload)
                        && let Some(store_like) =
                            extract_store_like_from_quote_summary_value(qs_val)
                        && let Ok(wrapped) = wrap_store_like(&store_like)
                    {
                        crate::core::logging::trace_debug!(
                            index = i,
                            wrapped_len = wrapped.len(),
                            "profile bootstrap Strategy C matched quoteSummary result"
                        );
                        return Some(wrapped);
                    }
                }
            }
        }
    }

    None
}

/// Strategy D: generic scan of *all* application/json scripts with multiple fallbacks.
#[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
pub fn try_generic_json_scripts(body: &str) -> Option<String> {
    let scripts = iter_json_scripts(body);

    for (i, (_attrs, inner_json)) in scripts.iter().enumerate() {
        let val = match serde_json::from_str::<Value>(inner_json) {
            Ok(v) => v,
            Err(e) => {
                crate::core::logging::trace_debug!(
                    index = i,
                    error = %e,
                    preview = %truncate(inner_json, 120),
                    "profile bootstrap Strategy D script parse failed"
                );
                continue;
            }
        };

        // D1: direct QuoteSummaryStore object
        if let Some(qss) = find_quote_summary_store_in_value(&val) {
            let store_like = normalize_store_like(qss.clone());
            if let Ok(wrapped) = wrap_store_like(&store_like) {
                crate::core::logging::trace_debug!(
                    index = i,
                    wrapped_len = wrapped.len(),
                    "profile bootstrap Strategy D matched QuoteSummaryStore"
                );
                return Some(wrapped);
            }
        }

        // D2: quoteSummary -> result[0]
        if let Some(qs_val) = find_quote_summary_value_in_value(&val)
            && let Some(store_like) = extract_store_like_from_quote_summary_value(qs_val)
            && let Ok(wrapped) = wrap_store_like(&store_like)
        {
            crate::core::logging::trace_debug!(
                index = i,
                wrapped_len = wrapped.len(),
                "profile bootstrap Strategy D matched quoteSummary result"
            );
            return Some(wrapped);
        }

        // D3: value has a "body" which itself is a JSON string/object/array
        if let Some(body_val) = val.get("body") {
            let payload_opt = match body_val {
                Value::String(s) => serde_json::from_str::<Value>(s).ok(),
                Value::Object(_) | Value::Array(_) => Some(body_val.clone()),
                _ => None,
            };

            if let Some(payload) = payload_opt {
                if let Some(qss) = find_quote_summary_store_in_value(&payload) {
                    let store_like = normalize_store_like(qss.clone());
                    if let Ok(wrapped) = wrap_store_like(&store_like) {
                        crate::core::logging::trace_debug!(
                            index = i,
                            wrapped_len = wrapped.len(),
                            "profile bootstrap Strategy D matched body QuoteSummaryStore"
                        );
                        return Some(wrapped);
                    }
                }

                if let Some(qs_val) = find_quote_summary_value_in_value(&payload)
                    && let Some(store_like) = extract_store_like_from_quote_summary_value(qs_val)
                    && let Ok(wrapped) = wrap_store_like(&store_like)
                {
                    crate::core::logging::trace_debug!(
                        index = i,
                        wrapped_len = wrapped.len(),
                        "profile bootstrap Strategy D matched body quoteSummary result"
                    );
                    return Some(wrapped);
                }
            }
        }
    }

    None
}
