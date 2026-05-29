use crate::{
    YfClient, YfError,
    core::{
        client::{RetryConfig, normalize_symbol},
        net,
    },
};
use paft::domain::Isin;
use serde::Deserialize;

#[derive(Deserialize)]
struct FlatSuggest {
    #[serde(alias = "Value", alias = "value")]
    value: Option<String>,
    #[serde(alias = "Symbol", alias = "symbol")]
    symbol: Option<String>,
    #[serde(alias = "Isin", alias = "isin", alias = "ISIN")]
    isin: Option<String>,
}

pub(super) async fn fetch_isin(
    client: &YfClient,
    symbol: &str,
    retry_override: Option<&RetryConfig>,
) -> Result<Option<String>, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let body = fetch_isin_body(client, &symbol, retry_override).await?;

    let input_norm = normalize_sym(&symbol);

    if let Some(isin) = parse_as_json_value(&body, &input_norm) {
        return Ok(Some(isin));
    }

    if let Some(isin) = parse_as_flat_suggest(&body, &input_norm) {
        return Ok(Some(isin));
    }

    if let Some(isin) = scan_raw_body(&body, &input_norm) {
        return Ok(Some(isin));
    }

    crate::core::logging::trace_debug!("no matching ISIN found in any response shape");
    Ok(None)
}

async fn fetch_isin_body(
    client: &YfClient,
    symbol: &str,
    retry_override: Option<&RetryConfig>,
) -> Result<String, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let mut url = client.base_insider_search().clone();
    url.query_pairs_mut()
        .append_pair("max_results", "5")
        .append_pair("query", &symbol);

    let req = client.http().get(url.clone());
    let resp = client.send_with_retry(req, retry_override).await?;

    if !resp.status().is_success() {
        return Err(net::status_error(resp.status(), &url));
    }

    net::get_text(resp, "isin_search", &symbol, "json")
        .await
        .map_err(YfError::from)
}

fn parse_as_json_value(body: &str, input_norm: &str) -> Option<String> {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(hit) = extract_from_json_value(&val, input_norm) {
            crate::core::logging::trace_debug!(isin = %hit, "ISIN extracted from JSON structures");
            return Some(hit);
        }
    } else {
        crate::core::logging::trace_debug!(
            query = %input_norm,
            "failed to parse ISIN JSON response"
        );
    }
    None
}

fn parse_as_flat_suggest(body: &str, input_norm: &str) -> Option<String> {
    if let Ok(raw_arr) = serde_json::from_str::<Vec<FlatSuggest>>(body) {
        let allow_unscoped = raw_arr.len() == 1;
        for r in &raw_arr {
            if let Some(isin) = r.isin.as_deref().and_then(validated_isin)
                && symbol_scope_matches(r.symbol.as_deref(), input_norm, allow_unscoped)
            {
                return Some(isin);
            }
            if let Some(value) = r.value.as_deref()
                && let Some(isin) = pick_from_pipe_value(value, input_norm)
            {
                return Some(isin);
            }
        }
    }
    None
}

fn scan_raw_body(body: &str, input_norm: &str) -> Option<String> {
    for token in body.split(['"', '\'', ',', '(', ')', '[', ']', ';']) {
        if token.contains('|')
            && let Some(isin) = pick_from_pipe_value(token, input_norm)
        {
            crate::core::logging::trace_debug!(
                isin = %isin,
                "fallback raw scan found symbol-matched ISIN"
            );
            return Some(isin);
        }
    }
    None
}

fn extract_from_json_value(v: &serde_json::Value, target_norm: &str) -> Option<String> {
    let mut arrays: Vec<&serde_json::Value> = Vec::new();

    match v {
        serde_json::Value::Array(_) => arrays.push(v),
        serde_json::Value::Object(map) => {
            for key in [
                "Suggestions",
                "suggestions",
                "items",
                "results",
                "Result",
                "data",
            ] {
                if let Some(val) = map.get(key)
                    && val.is_array()
                {
                    arrays.push(val);
                }
            }
            if arrays.is_empty() {
                for (_, val) in map {
                    if val.is_array() {
                        arrays.push(val);
                    } else if let Some(obj) = val.as_object() {
                        for (_, inner) in obj {
                            if inner.is_array() {
                                arrays.push(inner);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    for arr in arrays {
        if let Some(a) = arr.as_array() {
            let allow_unscoped = a.len() == 1;
            for item in a {
                if let Some(obj) = item.as_object() {
                    let symbol = obj
                        .get("Symbol")
                        .and_then(|x| x.as_str())
                        .or_else(|| obj.get("symbol").and_then(|x| x.as_str()));

                    for k in ["Isin", "isin", "ISIN"] {
                        if let Some(isin) =
                            obj.get(k).and_then(|x| x.as_str()).and_then(validated_isin)
                            && symbol_scope_matches(symbol, target_norm, allow_unscoped)
                        {
                            return Some(isin);
                        }
                    }

                    let value_str = obj
                        .get("Value")
                        .and_then(|x| x.as_str())
                        .or_else(|| obj.get("value").and_then(|x| x.as_str()))
                        .unwrap_or("");
                    if !value_str.is_empty()
                        && let Some(isin) = pick_from_pipe_value(value_str, target_norm)
                    {
                        return Some(isin);
                    }

                    if symbol.is_some_and(|sym| normalize_sym(sym) == target_norm) {
                        for (_k, v) in obj {
                            if let Some(isin) = v.as_str().and_then(validated_isin) {
                                return Some(isin);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn normalize_sym(s: &str) -> String {
    s.trim()
        .chars()
        .map(|ch| match ch {
            '-' | ':' => '.',
            ch if ch.is_ascii_whitespace() => '.',
            ch => ch.to_ascii_lowercase(),
        })
        .collect()
}

fn symbol_scope_matches(symbol: Option<&str>, target_norm: &str, allow_unscoped: bool) -> bool {
    symbol
        .filter(|symbol| !symbol.trim().is_empty())
        .map_or(allow_unscoped, |symbol| {
            normalize_sym(symbol) == target_norm
        })
}

fn validated_isin(candidate: &str) -> Option<String> {
    Isin::new(candidate).ok().map(String::from)
}

fn pick_from_pipe_value(value: &str, target_norm: &str) -> Option<String> {
    let mut parts = value
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty());
    let first = parts.next()?;
    if normalize_sym(first) != target_norm {
        return None;
    }

    parts.find_map(validated_isin)
}
