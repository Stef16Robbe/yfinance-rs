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
        diagnostics::{optional_u32_from_i64, optional_wire_cloned, optional_wire_copied},
        wire::{JsonDecimal, JsonU64, WireValue},
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

fn wire_str(value: &WireValue<String>) -> Option<&str> {
    value.as_ref().map(String::as_str)
}

fn wire_string(value: &WireValue<String>) -> Option<String> {
    wire_str(value).map(str::to_owned)
}

fn optional_screener_string(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<String>,
) -> Result<Option<String>, YfError> {
    optional_wire_cloned(ctx, path, key, path, value)
}

fn optional_screener_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<f64>,
) -> Result<Option<f64>, YfError> {
    optional_wire_copied(ctx, path, key, path, value)
}

fn optional_screener_u64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<JsonU64>,
) -> Result<Option<u64>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.map(JsonU64::into_u64))
}

fn optional_screener_decimal(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: &WireValue<JsonDecimal>,
) -> Result<Option<paft::Decimal>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.map(JsonDecimal::into_decimal))
}

#[derive(Debug, Deserialize)]
struct WireQuote {
    #[serde(default)]
    symbol: WireValue<String>,
    #[serde(rename = "shortName")]
    #[serde(default)]
    short_name: WireValue<String>,
    #[serde(rename = "longName")]
    #[serde(default)]
    long_name: WireValue<String>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: WireValue<String>,
    #[serde(default)]
    exchange: WireValue<String>,
    #[serde(rename = "exchDisp")]
    #[serde(default)]
    exchange_display: WireValue<String>,
    #[serde(rename = "typeDisp")]
    #[serde(default)]
    type_display: WireValue<String>,
    #[serde(rename = "regularMarketPrice")]
    #[serde(default)]
    regular_market_price: WireValue<f64>,
    #[serde(rename = "regularMarketChangePercent")]
    #[serde(default)]
    regular_market_change_percent: WireValue<f64>,
    #[serde(rename = "regularMarketVolume")]
    #[serde(default)]
    regular_market_volume: WireValue<JsonU64>,
    #[serde(rename = "marketCap")]
    #[serde(default)]
    market_cap: WireValue<JsonDecimal>,
    #[serde(default)]
    currency: WireValue<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

struct ScreenerWireFields {
    symbol: Option<String>,
    short_name: Option<String>,
    long_name: Option<String>,
    quote_type_raw: Option<String>,
    exchange_raw: Option<String>,
    exchange_display: Option<String>,
    type_display: Option<String>,
    regular_market_price: Option<f64>,
    regular_market_change_percent: Option<f64>,
    regular_market_volume: Option<u64>,
    market_cap: Option<paft::Decimal>,
    currency_raw: Option<String>,
}

impl WireQuote {
    fn fields(
        &self,
        ctx: &mut ProjectionContext,
        key: Option<String>,
    ) -> Result<ScreenerWireFields, YfError> {
        Ok(ScreenerWireFields {
            symbol: optional_screener_string(ctx, "symbol", key.clone(), &self.symbol)?,
            short_name: optional_screener_string(ctx, "shortName", key.clone(), &self.short_name)?,
            long_name: optional_screener_string(ctx, "longName", key.clone(), &self.long_name)?,
            quote_type_raw: optional_screener_string(
                ctx,
                "quoteType",
                key.clone(),
                &self.quote_type,
            )?,
            exchange_raw: optional_screener_string(ctx, "exchange", key.clone(), &self.exchange)?,
            exchange_display: optional_screener_string(
                ctx,
                "exchDisp",
                key.clone(),
                &self.exchange_display,
            )?,
            type_display: optional_screener_string(
                ctx,
                "typeDisp",
                key.clone(),
                &self.type_display,
            )?,
            regular_market_price: optional_screener_f64(
                ctx,
                "regularMarketPrice",
                key.clone(),
                &self.regular_market_price,
            )?,
            regular_market_change_percent: optional_screener_f64(
                ctx,
                "regularMarketChangePercent",
                key.clone(),
                &self.regular_market_change_percent,
            )?,
            regular_market_volume: optional_screener_u64(
                ctx,
                "regularMarketVolume",
                key.clone(),
                &self.regular_market_volume,
            )?,
            market_cap: optional_screener_decimal(ctx, "marketCap", key.clone(), &self.market_cap)?,
            currency_raw: optional_screener_string(ctx, "currency", key, &self.currency)?,
        })
    }

    fn project(self, ctx: &mut ProjectionContext) -> Result<ScreenerResult, YfError> {
        let wire = self;
        let key = wire_string(&wire.symbol);
        let fields = wire.fields(ctx, key.clone())?;

        let quote_type = fields
            .quote_type_raw
            .as_deref()
            .and_then(YahooQuoteType::parse);
        if quote_type.is_none()
            && let Some(value) = fields.quote_type_raw.as_deref().and_then(nonempty)
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
        let asset_kind = fields
            .quote_type_raw
            .as_deref()
            .and_then(nonempty)
            .and_then(|value| {
                quote_type
                    .map(YahooQuoteType::asset_kind)
                    .or_else(|| parse_yahoo_quote_type(value).ok())
            });
        let exchange = match fields.exchange_raw.as_deref().and_then(nonempty) {
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
        let yahoo_exchange = fields
            .exchange_raw
            .as_deref()
            .and_then(YahooExchangeCode::parse);
        let instrument = project_instrument(
            ctx,
            key.clone(),
            fields.symbol.as_deref(),
            asset_kind,
            exchange.clone(),
        )?;

        let (currency, invalid_currency) = parse_screener_currency(fields.currency_raw.as_deref());
        let currency = ScreenerCurrencyRef {
            unit: currency.as_ref(),
            invalid: invalid_currency.as_deref(),
        };
        let price = optional_screener_price(
            ctx,
            "regularMarketPrice",
            key.clone(),
            currency,
            fields.regular_market_price,
            "screener price",
        )?;
        let market_cap = optional_screener_money(
            ctx,
            "marketCap",
            key,
            currency,
            fields.market_cap,
            "screener market cap",
        )?;

        Ok(ScreenerResult {
            symbol: fields.symbol,
            instrument,
            name: fields.short_name.or(fields.long_name),
            quote_type,
            exchange,
            yahoo_exchange,
            raw_exchange: fields.exchange_raw,
            exchange_display: fields.exchange_display,
            type_display: fields.type_display,
            price,
            regular_market_change_percent: fields.regular_market_change_percent,
            regular_market_volume: fields.regular_market_volume,
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
