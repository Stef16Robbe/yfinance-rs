use crate::{
    YfClient, YfError,
    core::{
        client::{RetryConfig, normalize_symbol},
        net,
    },
};
use paft::domain::Isin;
use serde::Deserialize;
use serde_json::Value;

type InsiderSuggestArgs = (Value, Vec<String>, Vec<Vec<String>>, Value, Value);

#[derive(Deserialize)]
struct JsonSuggestRow {
    #[serde(alias = "Value", alias = "value")]
    value: Option<String>,
    #[serde(alias = "Symbol", alias = "symbol")]
    symbol: Option<String>,
    #[serde(alias = "Isin", alias = "isin", alias = "ISIN")]
    isin: Option<String>,
}

#[derive(Debug)]
struct InsiderSuggestResponse {
    rows: Vec<InsiderSuggestRow>,
}

#[derive(Debug, Default)]
struct InsiderSuggestRow {
    keywords: Option<String>,
    ids: Option<String>,
}

pub(super) async fn fetch_isin(
    client: &YfClient,
    symbol: &str,
    retry_override: Option<&RetryConfig>,
) -> Result<Option<String>, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let body = fetch_isin_body(client, &symbol, retry_override).await?;

    let input_norm = normalize_sym(&symbol);

    if let Some(isin) = parse_business_insider_suggest(&body, &input_norm) {
        return Ok(Some(isin));
    }

    if let Some(isin) = parse_json_suggest_rows(&body, &input_norm) {
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

fn parse_business_insider_suggest(body: &str, input_norm: &str) -> Option<String> {
    let response = parse_business_insider_suggest_response(body)?;
    for row in &response.rows {
        if let Some(isin) = row.isin_for_symbol(input_norm) {
            crate::core::logging::trace_debug!(
                isin = %isin,
                "ISIN extracted from Business Insider suggest response"
            );
            return Some(isin);
        }
    }
    None
}

fn parse_business_insider_suggest_response(body: &str) -> Option<InsiderSuggestResponse> {
    let args = business_insider_callback_args(body)?;
    let json_args = business_insider_args_to_json(args)?;
    let (_, columns, rows, _, _): InsiderSuggestArgs =
        serde_json::from_str(&format!("[{json_args}]")).ok()?;
    Some(InsiderSuggestResponse::from_parts(&columns, rows))
}

fn business_insider_callback_args(body: &str) -> Option<&str> {
    let body = body.trim();
    let body = body.strip_prefix("mmSuggestDeliver")?.trim_start();
    let body = body.strip_prefix('(')?.trim();
    let body = body.strip_suffix(';').unwrap_or(body).trim_end();
    body.strip_suffix(')').map(str::trim)
}

fn business_insider_args_to_json(args: &str) -> Option<String> {
    let mut out = String::with_capacity(args.len());
    let mut cursor = 0;
    let mut in_string = false;
    let mut escaped = false;

    while cursor < args.len() {
        if in_string {
            let ch = args[cursor..].chars().next()?;
            out.push(ch);
            cursor += ch.len_utf8();

            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if let Some(len) = array_constructor_len(&args[cursor..]) {
            out.push('[');
            cursor += len;
            continue;
        }

        let ch = args[cursor..].chars().next()?;
        cursor += ch.len_utf8();
        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            ')' => out.push(']'),
            '(' => return None,
            _ => out.push(ch),
        }
    }

    (!in_string).then_some(out)
}

fn array_constructor_len(input: &str) -> Option<usize> {
    let rest = input.strip_prefix("new")?;
    let new_len = input.len() - rest.len();
    let ws_after_new = leading_whitespace_len(rest);
    if ws_after_new == 0 {
        return None;
    }

    let rest = &rest[ws_after_new..];
    let rest = rest.strip_prefix("Array")?;
    let array_len = "Array".len();
    let ws_after_array = leading_whitespace_len(rest);
    let rest = &rest[ws_after_array..];
    rest.starts_with('(')
        .then_some(new_len + ws_after_new + array_len + ws_after_array + '('.len_utf8())
}

fn leading_whitespace_len(input: &str) -> usize {
    input
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum()
}

fn parse_json_suggest_rows(body: &str, input_norm: &str) -> Option<String> {
    let rows = serde_json::from_str::<Vec<JsonSuggestRow>>(body).ok()?;
    let allow_unscoped = rows.len() == 1;
    for row in &rows {
        if let Some(isin) = row.isin.as_deref().and_then(validated_isin)
            && symbol_scope_matches(row.symbol.as_deref(), input_norm, allow_unscoped)
        {
            return Some(isin);
        }
        if let Some(value) = row.value.as_deref()
            && let Some(isin) = pick_from_pipe_value(value, input_norm)
        {
            return Some(isin);
        }
    }
    None
}

impl InsiderSuggestResponse {
    fn from_parts(columns: &[String], raw_rows: Vec<Vec<String>>) -> Self {
        let rows = raw_rows
            .into_iter()
            .map(|cells| InsiderSuggestRow::from_cells(columns, &cells))
            .collect();
        Self { rows }
    }
}

impl InsiderSuggestRow {
    fn from_cells(columns: &[String], cells: &[String]) -> Self {
        let mut row = Self::default();
        for (column, cell) in columns.iter().zip(cells) {
            match column.as_str() {
                "Keywords" => row.keywords = Some(cell.clone()),
                "IDs" => row.ids = Some(cell.clone()),
                _ => {}
            }
        }
        row
    }

    fn isin_for_symbol(&self, target_norm: &str) -> Option<String> {
        let symbol = self.symbol_hint()?;
        if normalize_sym(symbol) != target_norm {
            return None;
        }

        self.keywords
            .as_deref()
            .into_iter()
            .flat_map(pipe_parts)
            .find_map(validated_isin)
    }

    fn symbol_hint(&self) -> Option<&str> {
        self.keywords
            .as_deref()
            .and_then(first_pipe_part)
            .or_else(|| self.ids.as_deref().and_then(second_pipe_part))
    }
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

fn pipe_parts(value: &str) -> impl Iterator<Item = &str> {
    value
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

fn first_pipe_part(value: &str) -> Option<&str> {
    pipe_parts(value).next()
}

fn second_pipe_part(value: &str) -> Option<&str> {
    pipe_parts(value).nth(1)
}

fn pick_from_pipe_value(value: &str, target_norm: &str) -> Option<String> {
    let mut parts = pipe_parts(value);
    let first = parts.next()?;
    if normalize_sym(first) != target_norm {
        return None;
    }

    parts.find_map(validated_isin)
}
