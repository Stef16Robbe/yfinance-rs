use std::collections::BTreeMap;

use paft::domain::{AssetKind, Exchange, Instrument};
use paft::money::Money;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::query::{YahooExchangeCode, YahooQuoteType};
use crate::YfError;

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
    pub price: Option<Money>,
    /// Regular market change percent, in percentage points.
    pub regular_market_change_percent: Option<f64>,
    /// Regular market volume.
    pub regular_market_volume: Option<u64>,
    /// Market capitalization as money when Yahoo supplies currency.
    pub market_cap: Option<Money>,
    /// Additional Yahoo screener fields not represented above.
    pub fields: BTreeMap<String, Value>,
}

pub(crate) fn parse_screener_body(body: &str) -> Result<ScreenerResponse, YfError> {
    let env: WireEnvelope = serde_json::from_str(body)?;
    if let Some(error) = env.finance.error {
        return Err(YfError::Api(error.to_string()));
    }

    let result = env
        .finance
        .result
        .and_then(|result| result.into_iter().next())
        .ok_or_else(|| YfError::MissingData("screener result missing".into()))?;

    let count = result.count.and_then(|c| u32::try_from(c).ok());
    let results = result
        .quotes
        .unwrap_or_default()
        .into_iter()
        .map(ScreenerResult::from)
        .collect();

    Ok(ScreenerResponse { count, results })
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
    quotes: Option<Vec<WireQuote>>,
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
    #[serde(default)]
    regular_market_volume: Option<u64>,
    #[serde(rename = "marketCap")]
    #[serde(default)]
    market_cap: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl From<WireQuote> for ScreenerResult {
    fn from(wire: WireQuote) -> Self {
        let quote_type = wire.quote_type.as_deref().and_then(YahooQuoteType::parse);
        let asset_kind = quote_type.map_or(AssetKind::Equity, YahooQuoteType::asset_kind);
        let exchange = wire
            .exchange
            .as_deref()
            .and_then(|exchange| exchange.parse::<Exchange>().ok());
        let yahoo_exchange = wire.exchange.as_deref().and_then(YahooExchangeCode::parse);
        let instrument = wire.symbol.as_deref().and_then(|symbol| {
            exchange
                .clone()
                .map_or_else(
                    || Instrument::from_symbol(symbol, asset_kind),
                    |ex| Instrument::from_symbol_and_exchange(symbol, ex, asset_kind),
                )
                .ok()
        });

        let price = wire.regular_market_price.map(|price| {
            crate::core::conversions::f64_to_money_with_currency_str(
                price,
                wire.currency.as_deref(),
            )
        });
        let market_cap = wire.market_cap.map(|market_cap| {
            crate::core::conversions::f64_to_money_with_currency_str(
                market_cap,
                wire.currency.as_deref(),
            )
        });

        Self {
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
        }
    }
}
