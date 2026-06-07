use std::borrow::Cow;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::{
    ProjectionIssue, YfClient, YfError, YfResponse,
    core::{
        CallOptions, ProjectionContext,
        client::{CacheEndpoint, SymbolEndpoint, normalize_symbol},
        currency_resolver::{
            CurrencyHints, CurrencyPurpose, ResolvedCurrencyUnit, TradingCurrencyEvidence,
            project_currency_resolution,
        },
        diagnostics::{
            optional_decimal_f64, optional_wire_cloned, optional_wire_copied, required_wire_value,
        },
        net,
        wire::{JsonU64, WireValue},
        yahoo_vocab::{first_parsed_yahoo_exchange, parse_yahoo_quote_type},
    },
};

use super::model::{OptionChain, OptionContract};
use chrono::{NaiveDate, TimeZone, Utc};
use paft::NonNegativeDecimal;
use paft::domain::{AssetKind, Instrument};
use paft::market::options::{OptionContractKey, OptionSide};
use paft::money::PriceAmount;

fn wire_str(value: &WireValue<String>) -> Option<&str> {
    value.as_ref().map(String::as_str)
}

fn wire_string(value: &WireValue<String>) -> Option<String> {
    wire_str(value).map(str::to_owned)
}

fn optional_option_string(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: &WireValue<String>,
) -> Result<Option<String>, YfError> {
    optional_wire_cloned(ctx, path, key, path, value)
}

fn optional_option_i64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: &WireValue<i64>,
) -> Result<Option<i64>, YfError> {
    optional_wire_copied(ctx, path, key, path, value)
}

fn optional_option_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: &WireValue<f64>,
) -> Result<Option<f64>, YfError> {
    optional_wire_copied(ctx, path, key, path, value)
}

fn optional_option_bool(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: &WireValue<bool>,
) -> Result<Option<bool>, YfError> {
    optional_wire_copied(ctx, path, key, path, value)
}

fn optional_option_u64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: &WireValue<JsonU64>,
) -> Result<Option<u64>, YfError> {
    Ok(optional_wire_copied(ctx, path, key, path, value)?.map(JsonU64::into_u64))
}

/* ---------------- Public: expirations + chain ---------------- */

pub async fn expiration_dates(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<Vec<i64>, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let (body, _used_url) = fetch_options_raw(client, &symbol, None, options).await?;
    let env: OptEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let first = first_option_result(env)?;

    Ok(first.expiration_dates.unwrap_or_default())
}

#[allow(clippy::too_many_lines)]
pub async fn option_chain(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    options: &CallOptions,
) -> Result<OptionChain, YfError> {
    Ok(option_chain_with_diagnostics(client, symbol, date, options)
        .await?
        .into_data())
}

#[allow(clippy::too_many_lines)]
pub async fn option_chain_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    options: &CallOptions,
) -> Result<YfResponse<OptionChain>, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let mut ctx = ProjectionContext::new("options", options.data_quality());
    let (body, used_url) = fetch_options_raw(client, &symbol, date, options).await?;
    let env: OptEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let first = first_option_result(env)?;

    if let Some(quote) = first.quote.as_ref() {
        client.store_currency_hints(
            &symbol,
            CurrencyHints::from_options_quote(
                wire_str(&quote.currency),
                wire_str(&quote.exchange),
                wire_str(&quote.full_exchange_name),
                wire_str(&quote.quote_type),
            ),
        );
    }

    let currency_from_response = currency_from_result(&first);
    let underlying_from_response = underlying_instrument_from_result(&first);
    let raw_underlying_quote_type = quote_type_from_result(&first);

    let Some(od) = first.options.and_then(|mut v| v.pop()) else {
        return Ok(ctx.finish(OptionChain {
            contracts: vec![],
            provider: (),
        }));
    };

    let expiration = optional_option_i64(
        &mut ctx,
        "expirationDate",
        Some(symbol.as_str()),
        &od.expiration_date,
    )?
    .or_else(|| {
        used_url.query().and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("date=").and_then(|v| v.parse::<i64>().ok()))
        })
    });

    if !option_date_has_contracts(&od) {
        return Ok(ctx.finish(OptionChain {
            contracts: vec![],
            provider: (),
        }));
    }

    let currency = project_currency_resolution(
        &mut ctx,
        &symbol,
        CurrencyPurpose::Trading,
        currency_from_response.as_deref(),
        client
            .resolve_trading_currency(
                &symbol,
                None,
                TradingCurrencyEvidence::OptionsQuote(currency_from_response.as_deref()),
                options,
            )
            .await,
    )?;
    let currency_issue = currency.issue().cloned();
    let currency = currency.into_unit();
    let underlying = underlying_instrument(
        client,
        &symbol,
        underlying_from_response,
        raw_underlying_quote_type,
    )?;

    let mut contracts = Vec::new();
    project_option_side(
        &mut ctx,
        od.calls,
        OptionSide::Call,
        expiration,
        currency.as_ref(),
        currency_issue.as_ref(),
        &underlying,
        &mut contracts,
    )?;
    project_option_side(
        &mut ctx,
        od.puts,
        OptionSide::Put,
        expiration,
        currency.as_ref(),
        currency_issue.as_ref(),
        &underlying,
        &mut contracts,
    )?;

    Ok(ctx.finish(OptionChain {
        contracts,
        provider: (),
    }))
}

fn underlying_instrument(
    client: &YfClient,
    symbol: &str,
    response_instrument: Option<Instrument>,
    raw_quote_type: Option<String>,
) -> Result<Instrument, YfError> {
    if let Some(instrument) = response_instrument {
        client.store_instrument(symbol.to_string(), instrument.clone());
        client.store_instrument(instrument.symbol.as_str().to_string(), instrument.clone());
        return Ok(instrument);
    }

    if let Some(instrument) = client.cached_instrument(symbol) {
        return Ok(instrument);
    }

    Err(YfError::OptionUnderlyingTypeUnavailable {
        symbol: symbol.to_string(),
        quote_type: raw_quote_type,
    })
}

#[allow(clippy::too_many_arguments)]
fn project_option_side(
    ctx: &mut ProjectionContext,
    side: Option<Vec<Value>>,
    option_side: OptionSide,
    expiration: Option<i64>,
    currency: Option<&ResolvedCurrencyUnit>,
    currency_issue: Option<&ProjectionIssue>,
    underlying: &Instrument,
    out: &mut Vec<OptionContract>,
) -> Result<(), YfError> {
    for (idx, contract) in side.unwrap_or_default().into_iter().enumerate() {
        let contract = match OptContractNode::deserialize(&contract) {
            Ok(contract) => contract,
            Err(err) => {
                let key =
                    option_contract_diag_key_from_value(&contract, option_side, expiration, idx);
                ctx.dropped_item(
                    "option_contract",
                    Some(&key),
                    ProjectionIssue::InvalidField {
                        field: "contract",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        if let Some(contract) = project_option_contract(
            ctx,
            &contract,
            option_side,
            expiration,
            currency,
            currency_issue,
            underlying,
        )? {
            out.push(contract);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn project_option_contract(
    ctx: &mut ProjectionContext,
    contract: &OptContractNode,
    option_side: OptionSide,
    expiration: Option<i64>,
    currency: Option<&ResolvedCurrencyUnit>,
    currency_issue: Option<&ProjectionIssue>,
    underlying: &Instrument,
) -> Result<Option<OptionContract>, YfError> {
    let key_for_diag = option_contract_diag_key(contract, option_side, expiration);
    let contract_expiration = optional_option_i64(
        ctx,
        "expiration",
        key_for_diag.as_deref(),
        &contract.expiration,
    )?;
    let Some(exp_ts) = contract_expiration.or(expiration) else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag.as_deref(),
            ProjectionIssue::MissingRequiredField {
                field: "expiration",
            },
        )?;
        return Ok(None);
    };
    let Some(exp_dt) = Utc.timestamp_opt(exp_ts, 0).single() else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag.as_deref(),
            ProjectionIssue::InvalidField {
                field: "expiration",
                details: format!("invalid unix timestamp {exp_ts}"),
            },
        )?;
        return Ok(None);
    };
    let Some(strike_raw) = required_wire_value(
        ctx,
        "option_contract",
        key_for_diag.as_deref(),
        "strike",
        &contract.strike,
    )?
    .copied() else {
        return Ok(None);
    };
    let Some(currency) = currency else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag.as_deref(),
            currency_issue
                .cloned()
                .unwrap_or(ProjectionIssue::CurrencyUnresolved),
        )?;
        return Ok(None);
    };
    let Some(strike) = currency.price_from_f64(strike_raw) else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag.as_deref(),
            ProjectionIssue::ConversionFailed {
                target: "option strike",
            },
        )?;
        return Ok(None);
    };

    let exp_date: NaiveDate = exp_dt.date_naive();
    let mut key = OptionContractKey::new(underlying.clone(), option_side, strike, exp_date);
    let key_for_diag = option_contract_diag_key(contract, option_side, Some(exp_ts));
    if let Some(contract_instrument) =
        optional_contract_instrument(ctx, key_for_diag.as_deref(), contract)?
    {
        key = key.with_contract_instrument(contract_instrument);
    }
    let last_price = optional_option_f64(
        ctx,
        "lastPrice",
        key_for_diag.as_deref(),
        &contract.last_price,
    )?;
    let bid = optional_option_f64(ctx, "bid", key_for_diag.as_deref(), &contract.bid)?;
    let ask = optional_option_f64(ctx, "ask", key_for_diag.as_deref(), &contract.ask)?;
    let volume = optional_option_u64(ctx, "volume", key_for_diag.as_deref(), &contract.volume)?;
    let open_interest = optional_option_u64(
        ctx,
        "openInterest",
        key_for_diag.as_deref(),
        &contract.open_interest,
    )?;
    let implied_volatility = optional_option_f64(
        ctx,
        "impliedVolatility",
        key_for_diag.as_deref(),
        &contract.implied_volatility,
    )?;
    let in_the_money = optional_option_bool(
        ctx,
        "inTheMoney",
        key_for_diag.as_deref(),
        &contract.in_the_money,
    )?;
    let last_trade_date = optional_option_i64(
        ctx,
        "lastTradeDate",
        key_for_diag.as_deref(),
        &contract.last_trade_date,
    )?;

    Ok(Some(OptionContract {
        key,
        currency: currency.currency().clone(),
        price: optional_option_price(
            ctx,
            "lastPrice",
            key_for_diag.as_deref(),
            currency,
            last_price,
            "option last price",
        )?,
        bid: optional_option_price(
            ctx,
            "bid",
            key_for_diag.as_deref(),
            currency,
            bid,
            "option bid",
        )?,
        ask: optional_option_price(
            ctx,
            "ask",
            key_for_diag.as_deref(),
            currency,
            ask,
            "option ask",
        )?,
        volume,
        open_interest,
        implied_volatility: optional_non_negative_decimal_f64(
            ctx,
            "impliedVolatility",
            key_for_diag.as_deref(),
            implied_volatility,
            "option implied volatility",
        )?,
        in_the_money,
        expiration_at: Some(exp_dt),
        last_trade_at: optional_option_timestamp(
            ctx,
            "lastTradeDate",
            key_for_diag.as_deref(),
            last_trade_date,
        )?,
        greeks: None,
        provider: (),
    }))
}

fn optional_contract_instrument(
    ctx: &mut ProjectionContext,
    key: Option<&str>,
    contract: &OptContractNode,
) -> Result<Option<Instrument>, YfError> {
    let Some(symbol) =
        optional_option_string(ctx, "contractSymbol", key, &contract.contract_symbol)?
    else {
        return Ok(None);
    };
    match Instrument::from_symbol(&symbol, AssetKind::Option) {
        Ok(instrument) => Ok(Some(instrument)),
        Err(err) => {
            ctx.omitted_present_field(
                "contractSymbol",
                key,
                ProjectionIssue::InvalidField {
                    field: "contractSymbol",
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
}

fn optional_option_price(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    currency: &ResolvedCurrencyUnit,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<PriceAmount>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(price) = currency.price_amount_from_f64(value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(price))
}

fn optional_non_negative_decimal_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<NonNegativeDecimal>, YfError> {
    let Some(decimal) = optional_decimal_f64(ctx, path, key, value, target)? else {
        return Ok(None);
    };
    if let Ok(value) = NonNegativeDecimal::new(decimal) {
        Ok(Some(value))
    } else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        Ok(None)
    }
}

fn optional_option_timestamp(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: Option<i64>,
) -> Result<Option<chrono::DateTime<Utc>>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(timestamp) = Utc.timestamp_opt(value, 0).single() else {
        ctx.omitted_present_field(
            path,
            key,
            ProjectionIssue::InvalidField {
                field: path,
                details: format!("invalid unix timestamp {value}"),
            },
        )?;
        return Ok(None);
    };
    Ok(Some(timestamp))
}

fn option_contract_diag_key(
    contract: &OptContractNode,
    option_side: OptionSide,
    expiration: Option<i64>,
) -> Option<Cow<'_, str>> {
    wire_str(&contract.contract_symbol)
        .map(Cow::Borrowed)
        .or_else(|| {
            let side = match option_side {
                OptionSide::Call => "call",
                OptionSide::Put => "put",
            };
            Some(Cow::Owned(format!(
                "{side}:{}@{}",
                contract
                    .strike
                    .as_ref()
                    .map_or_else(|| "?".to_string(), ToString::to_string),
                contract
                    .expiration
                    .as_ref()
                    .copied()
                    .or(expiration)
                    .map_or_else(|| "?".to_string(), |expiration| expiration.to_string())
            )))
        })
}

fn option_contract_diag_key_from_value(
    contract: &Value,
    option_side: OptionSide,
    expiration: Option<i64>,
    idx: usize,
) -> String {
    if let Some(symbol) = contract
        .get("contractSymbol")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
    {
        return symbol.to_string();
    }

    let side = match option_side {
        OptionSide::Call => "call",
        OptionSide::Put => "put",
    };
    let expiration =
        expiration.map_or_else(|| "?".to_string(), |expiration| expiration.to_string());
    format!("{side}[{idx}]@{expiration}")
}

fn underlying_instrument_from_result(node: &OptResultNode) -> Option<Instrument> {
    let quote = node.quote.as_ref();
    let symbol = node
        .underlying_symbol
        .as_ref()
        .map(String::as_str)
        .or_else(|| quote.and_then(|quote| wire_str(&quote.symbol)))?;
    let kind = quote
        .and_then(|quote| wire_str(&quote.quote_type))
        .and_then(|value| quote_type_to_asset_kind(value).ok())?;
    let exchange = quote.and_then(OptQuoteNode::exchange);

    match exchange {
        Some(exchange) => Instrument::from_symbol_and_exchange(symbol, exchange, kind),
        None => Instrument::from_symbol(symbol, kind),
    }
    .ok()
}

fn quote_type_to_asset_kind(value: &str) -> Result<AssetKind, YfError> {
    parse_yahoo_quote_type(value)
}

fn quote_type_from_result(node: &OptResultNode) -> Option<String> {
    node.quote
        .as_ref()
        .and_then(|quote| wire_str(&quote.quote_type))
        .map(str::trim)
        .filter(|quote_type| !quote_type.is_empty())
        .map(str::to_string)
}

fn option_date_has_contracts(node: &OptByDateNode) -> bool {
    node.calls.as_ref().is_some_and(|calls| !calls.is_empty())
        || node.puts.as_ref().is_some_and(|puts| !puts.is_empty())
}

/* ---------------- Internal: raw fetch with auth fallback ---------------- */

async fn fetch_options_raw(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    options: &CallOptions,
) -> Result<(String, Url), YfError> {
    let symbol = normalize_symbol(symbol)?;
    let mut url = client.symbol_url(SymbolEndpoint::OptionsV7, &symbol)?;
    {
        let mut qp = url.query_pairs_mut();
        if let Some(d) = date {
            qp.append_pair("date", &d.to_string());
        }
    }

    let fixture_key = date.map_or_else(|| symbol.clone(), |d| format!("{symbol}_{d}"));
    net::fetch_text_with_auth_retry(
        client,
        url,
        net::AuthFetchConfig {
            auth_mode: net::AuthMode::OptionalCrumb,
            cache_endpoint: CacheEndpoint::Options,
            options,
            cache_body: None,
            endpoint: "options_v7",
            fixture_key: &fixture_key,
            ext: "json",
            retry_on_invalid_crumb_body: true,
            cache_validator: Some(validate_options_body),
        },
        |url| client.http().get(url).header("accept", "application/json"),
    )
    .await
}

/* ---------------- Minimal serde mapping for v7 options ---------------- */

#[derive(Deserialize)]
struct OptEnvelope {
    #[serde(rename = "optionChain")]
    option_chain: Option<OptChainNode>,
}

#[derive(Deserialize)]
struct OptChainNode {
    result: Option<Vec<OptResultNode>>,
    error: Option<serde_json::Value>,
}

fn validate_options_body(body: &str) -> Result<(), YfError> {
    let env: OptEnvelope = serde_json::from_str(body).map_err(YfError::Json)?;
    first_option_result(env).map(|_| ())
}

#[derive(Deserialize)]
struct OptResultNode {
    #[serde(rename = "underlyingSymbol")]
    #[serde(default)]
    underlying_symbol: WireValue<String>,
    #[serde(rename = "expirationDates")]
    expiration_dates: Option<Vec<i64>>,
    quote: Option<OptQuoteNode>,
    options: Option<Vec<OptByDateNode>>,
}

fn first_option_result(env: OptEnvelope) -> Result<OptResultNode, YfError> {
    let option_chain = env
        .option_chain
        .ok_or_else(|| YfError::MissingData("empty options result".into()))?;

    if let Some(error) = option_chain.error.as_ref() {
        let message = option_chain_error_message(error);
        crate::core::logging::trace_error!(
            error = %message,
            "optionChain error"
        );
        return Err(YfError::Api(format!("yahoo error: {message}")));
    }

    option_chain
        .result
        .and_then(|mut v| v.pop())
        .ok_or_else(|| YfError::MissingData("empty options result".into()))
}

fn option_chain_error_message(error: &Value) -> String {
    error
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| error.get("message").and_then(Value::as_str))
        .map_or_else(|| error.to_string(), str::to_owned)
}

#[derive(Deserialize)]
struct OptQuoteNode {
    #[serde(default)]
    symbol: WireValue<String>,
    #[serde(rename = "quoteType")]
    #[serde(default)]
    quote_type: WireValue<String>,
    #[serde(rename = "fullExchangeName")]
    #[serde(default)]
    full_exchange_name: WireValue<String>,
    #[serde(default)]
    exchange: WireValue<String>,
    #[serde(default)]
    market: WireValue<String>,
    #[serde(rename = "marketCapFigureExchange")]
    #[serde(default)]
    market_cap_figure_exchange: WireValue<String>,
    #[serde(default)]
    currency: WireValue<String>,
}

impl OptQuoteNode {
    fn exchange(&self) -> Option<paft::domain::Exchange> {
        first_parsed_yahoo_exchange([
            wire_str(&self.full_exchange_name),
            wire_str(&self.exchange),
            wire_str(&self.market),
            wire_str(&self.market_cap_figure_exchange),
        ])
    }
}

#[derive(Deserialize)]
struct OptByDateNode {
    #[serde(rename = "expirationDate")]
    #[serde(default)]
    expiration_date: WireValue<i64>,
    calls: Option<Vec<Value>>,
    puts: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct OptContractNode {
    #[serde(rename = "contractSymbol")]
    #[serde(default)]
    contract_symbol: WireValue<String>,
    #[serde(rename = "expiration")]
    #[serde(default)]
    expiration: WireValue<i64>,
    #[serde(rename = "lastTradeDate")]
    #[serde(default)]
    last_trade_date: WireValue<i64>,
    #[serde(default)]
    strike: WireValue<f64>,
    #[serde(rename = "lastPrice")]
    #[serde(default)]
    last_price: WireValue<f64>,
    #[serde(default)]
    bid: WireValue<f64>,
    #[serde(default)]
    ask: WireValue<f64>,
    #[serde(default)]
    volume: WireValue<JsonU64>,
    #[serde(rename = "openInterest")]
    #[serde(default)]
    open_interest: WireValue<JsonU64>,
    #[serde(rename = "impliedVolatility")]
    #[serde(default)]
    implied_volatility: WireValue<f64>,
    #[serde(rename = "inTheMoney")]
    #[serde(default)]
    in_the_money: WireValue<bool>,
}

fn currency_from_result(node: &OptResultNode) -> Option<String> {
    node.quote.as_ref().and_then(|q| wire_string(&q.currency))
}
