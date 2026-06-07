use std::collections::BTreeMap;

use paft::domain::{Exchange, Instrument};
use paft::money::{Money, Price};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::query::{YahooExchangeCode, YahooQuoteType};
use crate::{
    DataQuality, ProjectionIssue, YfError, YfResponse,
    core::{
        ProjectionContext,
        currency_resolver::ResolvedCurrencyUnit,
        diagnostics::optional_u32_from_i64,
        wire::{JsonDecimal, de_u64_from_json},
        yahoo_vocab::{parse_yahoo_exchange, parse_yahoo_quote_type},
    },
};

/// Response from a Yahoo screener request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScreenerResponse {
    /// Total count reported by Yahoo, when present.
    pub count: Option<u32>,
    /// Matching screener results.
    pub results: Vec<ScreenerResult>,
}

/// A single Yahoo screener result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScreenerResult {
    /// Raw Yahoo symbol, preserved even when instrument construction fails.
    pub symbol: Option<String>,
    /// Provider-agnostic instrument identity when the symbol is valid.
    pub instrument: Option<Instrument>,
    /// Display name.
    pub name: Option<String>,
    /// Yahoo quote type.
    pub quote_type: Option<YahooQuoteType>,
    /// Provider-agnostic exchange when parsable by paft.
    pub exchange: Option<Exchange>,
    /// Yahoo exchange code when supported by the typed API.
    pub yahoo_exchange: Option<YahooExchangeCode>,
    /// Raw exchange string from Yahoo.
    pub raw_exchange: Option<String>,
    /// Display exchange name.
    pub exchange_display: Option<String>,
    /// Display quote type name.
    pub type_display: Option<String>,
    /// Last market price.
    pub price: Option<Price>,
    /// Regular market change percent, in percentage points.
    pub regular_market_change_percent: Option<f64>,
    /// Regular market volume.
    pub regular_market_volume: Option<u64>,
    /// Market capitalization as money when Yahoo supplies currency.
    pub market_cap: Option<Money>,
    /// Additional Yahoo screener fields not represented above.
    pub fields: BTreeMap<String, Value>,
}

pub(super) fn parse_screener_body_with_diagnostics(
    body: &str,
    data_quality: DataQuality,
) -> Result<YfResponse<ScreenerResponse>, YfError> {
    let mut ctx = ProjectionContext::new("screener", data_quality);
    let env: WireEnvelope = serde_json::from_str(body)?;
    reject_screener_error(&env)?;

    let result = env
        .finance
        .result
        .and_then(|result| result.into_iter().next())
        .ok_or_else(|| YfError::MissingData("screener result missing".into()))?;

    let count = optional_u32_from_i64(&mut ctx, "count", None, "count", result.count)?;
    let mut results = Vec::new();
    for (idx, quote) in result
        .quotes
        .ok_or_else(|| YfError::MissingData("screener quotes missing".into()))?
        .into_iter()
        .enumerate()
    {
        let key = Some(screener_quote_diag_key(&quote, idx));
        let quote = match serde_json::from_value::<WireQuote>(quote) {
            Ok(quote) => quote,
            Err(err) => {
                ctx.dropped_item(
                    "screener_result",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "quote",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        results.push(quote.project(&mut ctx)?);
    }

    Ok(ctx.finish(ScreenerResponse { count, results }))
}

pub(super) fn validate_screener_body(body: &str) -> Result<(), YfError> {
    let env: WireEnvelope = serde_json::from_str(body)?;
    reject_screener_error(&env)
}

fn reject_screener_error(env: &WireEnvelope) -> Result<(), YfError> {
    if let Some(error) = env.finance.error.as_ref() {
        return Err(YfError::Api(error.to_string()));
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct WireEnvelope {
    finance: WireFinance,
}

#[derive(Debug, Deserialize)]
struct WireFinance {
    result: Option<Vec<WireResult>>,
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WireResult {
    count: Option<i64>,
    quotes: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct WireQuote {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(rename = "shortName")]
    #[serde(default)]
    short_name: Option<String>,
    #[serde(rename = "longName")]
    #[serde(default)]
    long_name: Option<String>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: Option<String>,
    #[serde(default)]
    exchange: Option<String>,
    #[serde(rename = "exchDisp")]
    #[serde(default)]
    exchange_display: Option<String>,
    #[serde(rename = "typeDisp")]
    #[serde(default)]
    type_display: Option<String>,
    #[serde(rename = "regularMarketPrice")]
    #[serde(default)]
    regular_market_price: Option<f64>,
    #[serde(rename = "regularMarketChangePercent")]
    #[serde(default)]
    regular_market_change_percent: Option<f64>,
    #[serde(rename = "regularMarketVolume")]
    #[serde(default, deserialize_with = "de_u64_from_json")]
    regular_market_volume: Option<u64>,
    #[serde(rename = "marketCap")]
    #[serde(default)]
    market_cap: Option<JsonDecimal>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl WireQuote {
    fn project(self, ctx: &mut ProjectionContext) -> Result<ScreenerResult, YfError> {
        let wire = self;
        let key = wire.symbol.clone();
        let quote_type = wire.quote_type.as_deref().and_then(YahooQuoteType::parse);
        if quote_type.is_none()
            && let Some(value) = wire.quote_type.as_deref().and_then(nonempty)
        {
            ctx.omitted_present_field(
                "quoteType",
                key.clone(),
                ProjectionIssue::InvalidField {
                    field: "quoteType",
                    details: format!("unsupported Yahoo quote type {value:?}"),
                },
            )?;
        }
        let asset_kind = wire
            .quote_type
            .as_deref()
            .and_then(nonempty)
            .and_then(|value| {
                quote_type
                    .map(YahooQuoteType::asset_kind)
                    .or_else(|| parse_yahoo_quote_type(value).ok())
            });
        let exchange = match wire.exchange.as_deref().and_then(nonempty) {
            Some(exchange) => match parse_yahoo_exchange(exchange) {
                Ok(exchange) => Some(exchange),
                Err(err) => {
                    ctx.omitted_present_field(
                        "exchange",
                        key.clone(),
                        ProjectionIssue::InvalidField {
                            field: "exchange",
                            details: err.to_string(),
                        },
                    )?;
                    None
                }
            },
            None => None,
        };
        let yahoo_exchange = wire.exchange.as_deref().and_then(YahooExchangeCode::parse);
        let instrument = project_instrument(
            ctx,
            key.clone(),
            wire.symbol.as_deref(),
            asset_kind,
            exchange.clone(),
        )?;

        let (currency, invalid_currency) = parse_screener_currency(wire.currency.as_deref());
        let currency = ScreenerCurrencyRef {
            unit: currency.as_ref(),
            invalid: invalid_currency.as_deref(),
        };
        let price = optional_screener_price(
            ctx,
            "regularMarketPrice",
            key.clone(),
            currency,
            wire.regular_market_price,
            "screener price",
        )?;
        let market_cap = optional_screener_money(
            ctx,
            "marketCap",
            key,
            currency,
            wire.market_cap.map(JsonDecimal::into_decimal),
            "screener market cap",
        )?;

        Ok(ScreenerResult {
            symbol: wire.symbol,
            instrument,
            name: wire.short_name.or(wire.long_name),
            quote_type,
            exchange,
            yahoo_exchange,
            raw_exchange: wire.exchange,
            exchange_display: wire.exchange_display,
            type_display: wire.type_display,
            price,
            regular_market_change_percent: wire.regular_market_change_percent,
            regular_market_volume: wire.regular_market_volume,
            market_cap,
            fields: wire.extra,
        })
    }
}

fn screener_quote_diag_key(value: &Value, idx: usize) -> String {
    value
        .get("symbol")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map_or_else(|| format!("quotes[{idx}]"), ToString::to_string)
}

fn project_instrument(
    ctx: &mut ProjectionContext,
    key: Option<String>,
    symbol: Option<&str>,
    asset_kind: Option<paft::domain::AssetKind>,
    exchange: Option<Exchange>,
) -> Result<Option<Instrument>, YfError> {
    let Some(symbol) = symbol.and_then(nonempty) else {
        return Ok(None);
    };
    let Some(asset_kind) = asset_kind else {
        ctx.omitted_present_field(
            "instrument",
            key,
            ProjectionIssue::MissingRequiredField { field: "quoteType" },
        )?;
        return Ok(None);
    };

    let instrument = match exchange {
        Some(exchange) => Instrument::from_symbol_and_exchange(symbol, exchange, asset_kind),
        None => Instrument::from_symbol(symbol, asset_kind),
    };
    match instrument {
        Ok(instrument) => Ok(Some(instrument)),
        Err(err) => {
            ctx.omitted_present_field(
                "instrument",
                key,
                ProjectionIssue::InvalidField {
                    field: "symbol",
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
}

fn parse_screener_currency(code: Option<&str>) -> (Option<ResolvedCurrencyUnit>, Option<String>) {
    let Some(code) = code.and_then(nonempty) else {
        return (None, None);
    };
    ResolvedCurrencyUnit::from_code(code)
        .map_or_else(|| (None, Some(code.to_string())), |unit| (Some(unit), None))
}

#[derive(Clone, Copy)]
struct ScreenerCurrencyRef<'a> {
    unit: Option<&'a ResolvedCurrencyUnit>,
    invalid: Option<&'a str>,
}

fn optional_screener_price(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    currency: ScreenerCurrencyRef<'_>,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<Price>, YfError> {
    optional_screener_currency_value(
        ctx,
        path,
        key,
        currency,
        value,
        target,
        ResolvedCurrencyUnit::price_from_f64,
    )
}

fn optional_screener_money(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    currency: ScreenerCurrencyRef<'_>,
    value: Option<paft::Decimal>,
    target: &'static str,
) -> Result<Option<Money>, YfError> {
    let major = currency.unit.map(ResolvedCurrencyUnit::major_unit);
    let currency = ScreenerCurrencyRef {
        unit: major.as_ref(),
        invalid: currency.invalid,
    };
    optional_screener_currency_value(ctx, path, key, currency, value, target, |unit, value| {
        unit.money_from_decimal(value).ok()
    })
}

fn optional_screener_currency_value<T, U>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    currency: ScreenerCurrencyRef<'_>,
    value: Option<T>,
    target: &'static str,
    convert: impl FnOnce(&ResolvedCurrencyUnit, T) -> Option<U>,
) -> Result<Option<U>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(currency_unit) = currency.unit else {
        let reason = currency
            .invalid
            .map_or(ProjectionIssue::CurrencyUnresolved, |code| {
                ProjectionIssue::InvalidCurrency {
                    code: code.to_string(),
                }
            });
        ctx.omitted_present_field(path, key, reason)?;
        return Ok(None);
    };
    let Some(value) = convert(currency_unit, value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(value))
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}
