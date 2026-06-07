use crate::{
    YfClient, YfError,
    core::{
        client::{RetryConfig, normalize_symbol},
        net,
    },
};
use boa_ast::{
    Expression, Statement, StatementListItem, expression::literal::LiteralKind, scope::Scope,
};
use boa_interner::{Interner, Sym};
use boa_parser::{Parser, Source};
use paft::domain::Isin;
use serde::Deserialize;
use serde_json::{Number, Value};

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
    let (_, columns, rows, _, _): InsiderSuggestArgs =
        serde_json::from_value(Value::Array(args)).ok()?;
    Some(InsiderSuggestResponse::from_parts(&columns, rows))
}

fn business_insider_callback_args(body: &str) -> Option<Vec<Value>> {
    let mut interner = Interner::default();
    let mut parser = Parser::new(Source::from_bytes(body.trim()));
    let script = parser
        .parse_script(&Scope::new_global(), &mut interner)
        .ok()?;

    let [StatementListItem::Statement(statement)] = script.statements().statements() else {
        return None;
    };
    let Statement::Expression(Expression::Call(call)) = statement.as_ref() else {
        return None;
    };
    if !identifier_is(call.function(), "mmSuggestDeliver", &interner) {
        return None;
    }

    call.args()
        .iter()
        .map(|arg| js_data_expression_to_json(arg, &interner))
        .collect()
}

fn js_data_expression_to_json(expr: &Expression, interner: &Interner) -> Option<Value> {
    match expr {
        Expression::Literal(literal) => match literal.kind() {
            LiteralKind::String(sym) => Some(Value::String(js_string(*sym, interner)?)),
            LiteralKind::Num(value) => Number::from_f64(*value).map(Value::Number),
            LiteralKind::Int(value) => Some(Value::Number(Number::from(i64::from(*value)))),
            LiteralKind::Bool(value) => Some(Value::Bool(*value)),
            LiteralKind::Null => Some(Value::Null),
            LiteralKind::BigInt(_) | LiteralKind::Undefined => None,
        },
        Expression::ArrayLiteral(array) => array
            .as_ref()
            .iter()
            .map(|element| js_data_expression_to_json(element.as_ref()?, interner))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        Expression::New(new) if identifier_is(new.constructor(), "Array", interner) => new
            .arguments()
            .iter()
            .map(|arg| js_data_expression_to_json(arg, interner))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        Expression::Parenthesized(parenthesized) => {
            js_data_expression_to_json(parenthesized.expression(), interner)
        }
        _ => None,
    }
}

fn identifier_is(expr: &Expression, expected: &str, interner: &Interner) -> bool {
    let Expression::Identifier(identifier) = expr else {
        return false;
    };
    interner
        .resolve(identifier.sym())
        .and_then(|ident| ident.utf8())
        .is_some_and(|ident| ident == expected)
}

fn js_string(sym: Sym, interner: &Interner) -> Option<String> {
    interner.resolve(sym)?.utf8().map(str::to_owned)
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
