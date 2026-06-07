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
    let response = InsiderSuggestParser::new(body).parse()?;
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

struct InsiderSuggestParser<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> InsiderSuggestParser<'a> {
    const fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    fn parse(mut self) -> Option<InsiderSuggestResponse> {
        self.skip_ws();
        self.expect("mmSuggestDeliver")?;
        self.skip_ws();
        self.expect("(")?;
        self.parse_integer()?;
        self.expect_comma()?;
        let columns = self.parse_string_array()?;
        self.expect_comma()?;
        let rows = self.parse_rows_array()?;
        self.expect_comma()?;
        self.parse_integer()?;
        self.expect_comma()?;
        self.parse_integer()?;
        self.skip_ws();
        self.expect(")")?;
        self.skip_ws();
        self.consume(";");
        self.skip_ws();
        (self.cursor == self.input.len())
            .then(|| InsiderSuggestResponse::from_parts(&columns, rows))
    }

    fn parse_rows_array(&mut self) -> Option<Vec<Vec<String>>> {
        self.expect_new_array_start()?;
        let mut rows = Vec::new();
        if self.consume(")") {
            return Some(rows);
        }

        loop {
            rows.push(self.parse_string_array()?);
            if self.consume_comma() {
                continue;
            }
            self.expect(")")?;
            return Some(rows);
        }
    }

    fn parse_string_array(&mut self) -> Option<Vec<String>> {
        self.expect_new_array_start()?;
        let mut values = Vec::new();
        if self.consume(")") {
            return Some(values);
        }

        loop {
            values.push(self.parse_string()?);
            if self.consume_comma() {
                continue;
            }
            self.expect(")")?;
            return Some(values);
        }
    }

    fn parse_string(&mut self) -> Option<String> {
        self.skip_ws();
        self.expect("\"")?;
        let mut out = String::new();
        let mut segment_start = self.cursor;

        while self.cursor < self.input.len() {
            match self.input.as_bytes()[self.cursor] {
                b'"' => {
                    out.push_str(&self.input[segment_start..self.cursor]);
                    self.cursor += 1;
                    return Some(out);
                }
                b'\\' => {
                    out.push_str(&self.input[segment_start..self.cursor]);
                    self.cursor += 1;
                    out.push(self.parse_escape()?);
                    segment_start = self.cursor;
                }
                _ => self.cursor += 1,
            }
        }

        None
    }

    fn parse_escape(&mut self) -> Option<char> {
        let escaped = self.input[self.cursor..].chars().next()?;
        self.cursor += escaped.len_utf8();
        match escaped {
            '"' | '\\' | '/' => Some(escaped),
            'b' => Some('\u{0008}'),
            'f' => Some('\u{000c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'u' => {
                let end = self.cursor.checked_add(4)?;
                let hex = self.input.get(self.cursor..end)?;
                let code = u32::from_str_radix(hex, 16).ok()?;
                self.cursor = end;
                char::from_u32(code)
            }
            _ => None,
        }
    }

    fn parse_integer(&mut self) -> Option<i64> {
        self.skip_ws();
        let start = self.cursor;
        self.consume("-");
        while self
            .input
            .as_bytes()
            .get(self.cursor)
            .is_some_and(u8::is_ascii_digit)
        {
            self.cursor += 1;
        }
        (self.cursor > start)
            .then(|| self.input[start..self.cursor].parse().ok())
            .flatten()
    }

    fn expect_new_array_start(&mut self) -> Option<()> {
        self.skip_ws();
        self.expect("new")?;
        self.require_ws()?;
        self.expect("Array")?;
        self.skip_ws();
        self.expect("(")
    }

    fn expect_comma(&mut self) -> Option<()> {
        self.consume_comma().then_some(())
    }

    fn consume_comma(&mut self) -> bool {
        self.skip_ws();
        self.consume(",")
    }

    fn expect(&mut self, token: &str) -> Option<()> {
        self.consume(token).then_some(())
    }

    fn consume(&mut self, token: &str) -> bool {
        self.skip_ws();
        if self.input[self.cursor..].starts_with(token) {
            self.cursor += token.len();
            true
        } else {
            false
        }
    }

    fn require_ws(&mut self) -> Option<()> {
        let before = self.cursor;
        self.skip_ws();
        (self.cursor > before).then_some(())
    }

    fn skip_ws(&mut self) {
        while self.input[self.cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            self.cursor += self.input[self.cursor..]
                .chars()
                .next()
                .expect("checked above")
                .len_utf8();
        }
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
