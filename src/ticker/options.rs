use serde::Deserialize;
use url::Url;

use crate::{
    DataQuality, ProjectionIssue, YfClient, YfError, YfResponse,
    core::{
        ProjectionContext,
        client::{CacheEndpoint, CacheMode, RetryConfig, SymbolEndpoint},
        conversions::{string_to_asset_kind, string_to_exchange},
        currency_resolver::{
            CurrencyHints, CurrencyKind, ResolvedCurrencyUnit, TradingCurrencyEvidence,
        },
        diagnostics::optional_decimal_f64,
        net,
    },
    screener::YahooQuoteType,
};

use super::model::{OptionChain, OptionContract};
use chrono::{NaiveDate, TimeZone, Utc};
use paft::domain::{AssetKind, Instrument};
use paft::market::options::{OptionContractKey, OptionSide};

/* ---------------- Public: expirations + chain ---------------- */

pub async fn expiration_dates(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<i64>, YfError> {
    let (body, _used_url) =
        fetch_options_raw(client, symbol, None, cache_mode, retry_override).await?;
    let env: OptEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let first = env
        .option_chain
        .and_then(|oc| oc.result)
        .and_then(|mut v| v.pop())
        .ok_or_else(|| YfError::MissingData("empty options result".into()))?;

    Ok(first.expiration_dates.unwrap_or_default())
}

#[allow(clippy::too_many_lines)]
pub async fn option_chain(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<OptionChain, YfError> {
    Ok(option_chain_with_diagnostics(
        client,
        symbol,
        date,
        cache_mode,
        retry_override,
        DataQuality::BestEffort,
    )
    .await?
    .into_data())
}

#[allow(clippy::too_many_lines)]
pub async fn option_chain_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<OptionChain>, YfError> {
    let mut ctx = ProjectionContext::new("options", data_quality);
    let (body, used_url) =
        fetch_options_raw(client, symbol, date, cache_mode, retry_override).await?;
    let env: OptEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let first = env
        .option_chain
        .and_then(|oc| oc.result)
        .and_then(|mut v| v.pop())
        .ok_or_else(|| YfError::MissingData("empty options result".into()))?;

    if let Some(quote) = first.quote.as_ref() {
        client
            .store_currency_hints(
                symbol,
                CurrencyHints::from_options_quote(
                    quote.currency.as_deref(),
                    quote.exchange.as_deref(),
                    quote.full_exchange_name.as_deref(),
                    quote.quote_type.as_deref(),
                ),
            )
            .await;
    }

    let currency_from_response = currency_from_result(&first);
    let underlying_from_response = underlying_instrument_from_result(&first);

    let Some(od) = first.options.and_then(|mut v| v.pop()) else {
        return Ok(ctx.finish(OptionChain {
            contracts: vec![],
            provider: (),
        }));
    };

    let expiration = od.expiration_date.or_else(|| {
        used_url.query().and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("date=").and_then(|v| v.parse::<i64>().ok()))
        })
    });

    let currency = match client
        .resolve_trading_currency_unit(
            symbol,
            None,
            TradingCurrencyEvidence::OptionsQuote(currency_from_response.as_deref()),
            cache_mode,
            retry_override,
        )
        .await
    {
        Ok(currency) => {
            ctx.currency_resolution(client, symbol, CurrencyKind::Trading)
                .await?;
            Some(currency)
        }
        Err(err) => {
            crate::core::logging::trace_debug!(
                symbol,
                error = %err,
                "failed to resolve option-chain trading currency"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = &err;
            None
        }
    };
    let currency_issue = currency
        .is_none()
        .then(|| currency_projection_issue(currency_from_response.as_deref()));
    let underlying = underlying_instrument(client, symbol, underlying_from_response).await?;

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

async fn underlying_instrument(
    client: &YfClient,
    symbol: &str,
    response_instrument: Option<Instrument>,
) -> Result<Instrument, YfError> {
    if let Some(instrument) = response_instrument {
        client
            .store_instrument(symbol.to_string(), instrument.clone())
            .await;
        client
            .store_instrument(instrument.symbol.as_str().to_string(), instrument.clone())
            .await;
        return Ok(instrument);
    }

    if let Some(instrument) = client.cached_instrument(symbol).await {
        return Ok(instrument);
    }

    Err(YfError::MissingData(format!(
        "unable to determine option underlying instrument for {symbol}"
    )))
}

#[allow(clippy::too_many_arguments)]
fn project_option_side(
    ctx: &mut ProjectionContext,
    side: Option<Vec<OptContractNode>>,
    option_side: OptionSide,
    expiration: Option<i64>,
    currency: Option<&ResolvedCurrencyUnit>,
    currency_issue: Option<&ProjectionIssue>,
    underlying: &Instrument,
    out: &mut Vec<OptionContract>,
) -> Result<(), YfError> {
    for contract in side.unwrap_or_default() {
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
    let Some(exp_ts) = contract.expiration.or(expiration) else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag,
            ProjectionIssue::MissingRequiredField {
                field: "expiration",
            },
        )?;
        return Ok(None);
    };
    let Some(exp_dt) = Utc.timestamp_opt(exp_ts, 0).single() else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag,
            ProjectionIssue::InvalidField {
                field: "expiration",
                details: format!("invalid unix timestamp {exp_ts}"),
            },
        )?;
        return Ok(None);
    };
    let Some(strike_raw) = contract.strike else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag,
            ProjectionIssue::MissingRequiredField { field: "strike" },
        )?;
        return Ok(None);
    };
    let Some(currency) = currency else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag,
            currency_issue
                .cloned()
                .unwrap_or(ProjectionIssue::CurrencyUnresolved),
        )?;
        return Ok(None);
    };
    let Some(strike) = currency.price_from_f64(strike_raw) else {
        ctx.dropped_item(
            "option_contract",
            key_for_diag,
            ProjectionIssue::ConversionFailed {
                target: "option strike",
            },
        )?;
        return Ok(None);
    };

    let exp_date: NaiveDate = exp_dt.date_naive();
    let key = OptionContractKey::new(underlying.clone(), option_side, strike, exp_date);
    let key_for_diag = option_contract_diag_key(contract, option_side, Some(exp_ts));
    let contract_instrument = optional_contract_instrument(ctx, key_for_diag.clone(), contract)?;

    Ok(Some(OptionContract {
        key,
        contract_instrument,
        price: optional_option_price(
            ctx,
            "lastPrice",
            key_for_diag.clone(),
            currency,
            contract.last_price,
            "option last price",
        )?,
        bid: optional_option_price(
            ctx,
            "bid",
            key_for_diag.clone(),
            currency,
            contract.bid,
            "option bid",
        )?,
        ask: optional_option_price(
            ctx,
            "ask",
            key_for_diag.clone(),
            currency,
            contract.ask,
            "option ask",
        )?,
        volume: contract.volume,
        open_interest: contract.open_interest,
        implied_volatility: optional_decimal_f64(
            ctx,
            "impliedVolatility",
            key_for_diag.clone(),
            contract.implied_volatility,
            "option implied volatility",
        )?,
        in_the_money: contract.in_the_money,
        expiration_at: Some(exp_dt),
        last_trade_at: optional_option_timestamp(
            ctx,
            "lastTradeDate",
            key_for_diag,
            contract.last_trade_date,
        )?,
        greeks: None,
        provider: (),
    }))
}

fn optional_contract_instrument(
    ctx: &mut ProjectionContext,
    key: Option<String>,
    contract: &OptContractNode,
) -> Result<Option<Instrument>, YfError> {
    let Some(symbol) = contract.contract_symbol.as_deref() else {
        return Ok(None);
    };
    match Instrument::from_symbol(symbol, AssetKind::Option) {
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
    key: Option<String>,
    currency: &ResolvedCurrencyUnit,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<paft::money::Price>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(price) = currency.price_from_f64(value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(price))
}

fn optional_option_timestamp(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
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
) -> Option<String> {
    contract.contract_symbol.clone().or_else(|| {
        let side = match option_side {
            OptionSide::Call => "call",
            OptionSide::Put => "put",
        };
        Some(format!(
            "{side}:{}@{}",
            contract
                .strike
                .map_or_else(|| "?".to_string(), |strike| strike.to_string()),
            contract
                .expiration
                .or(expiration)
                .map_or_else(|| "?".to_string(), |expiration| expiration.to_string())
        ))
    })
}

fn currency_projection_issue(code: Option<&str>) -> ProjectionIssue {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return ProjectionIssue::CurrencyUnresolved;
    };
    if ResolvedCurrencyUnit::from_code(code).is_none() {
        ProjectionIssue::InvalidCurrency {
            code: code.to_string(),
        }
    } else {
        ProjectionIssue::CurrencyUnresolved
    }
}

fn underlying_instrument_from_result(node: &OptResultNode) -> Option<Instrument> {
    let quote = node.quote.as_ref();
    let symbol = node
        .underlying_symbol
        .as_deref()
        .or_else(|| quote.and_then(|quote| quote.symbol.as_deref()))?;
    let kind = quote
        .and_then(|quote| quote.quote_type.as_deref())
        .and_then(|value| quote_type_to_asset_kind(value).ok())?;
    let exchange = quote.and_then(OptQuoteNode::exchange);

    match exchange {
        Some(exchange) => Instrument::from_symbol_and_exchange(symbol, exchange, kind),
        None => Instrument::from_symbol(symbol, kind),
    }
    .ok()
}

fn quote_type_to_asset_kind(value: &str) -> Result<AssetKind, YfError> {
    YahooQuoteType::parse(value)
        .map(YahooQuoteType::asset_kind)
        .map_or_else(|| string_to_asset_kind(value), Ok)
}

/* ---------------- Internal: raw fetch with auth fallback ---------------- */

async fn fetch_options_raw(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<(String, Url), YfError> {
    let mut url = client.symbol_url(SymbolEndpoint::OptionsV7, symbol)?;
    {
        let mut qp = url.query_pairs_mut();
        if let Some(d) = date {
            qp.append_pair("date", &d.to_string());
        }
    }

    let fixture_key = date.map_or_else(|| symbol.to_string(), |d| format!("{symbol}_{d}"));
    net::fetch_text_with_auth_retry(
        client,
        url,
        net::AuthFetchConfig {
            auth_mode: net::AuthMode::OptionalCrumb,
            cache_endpoint: CacheEndpoint::Options,
            cache_mode,
            cache_body: None,
            retry_override,
            endpoint: "options_v7",
            fixture_key: &fixture_key,
            ext: "json",
            retry_on_invalid_crumb_body: true,
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
    #[allow(dead_code)]
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct OptResultNode {
    #[serde(rename = "underlyingSymbol")]
    underlying_symbol: Option<String>,
    #[serde(rename = "expirationDates")]
    expiration_dates: Option<Vec<i64>>,
    quote: Option<OptQuoteNode>,
    options: Option<Vec<OptByDateNode>>,
}

#[derive(Deserialize)]
struct OptQuoteNode {
    symbol: Option<String>,
    #[serde(rename = "quoteType")]
    quote_type: Option<String>,
    #[serde(rename = "fullExchangeName")]
    full_exchange_name: Option<String>,
    exchange: Option<String>,
    market: Option<String>,
    #[serde(rename = "marketCapFigureExchange")]
    market_cap_figure_exchange: Option<String>,
    currency: Option<String>,
}

impl OptQuoteNode {
    fn exchange(&self) -> Option<paft::domain::Exchange> {
        string_to_exchange(
            self.full_exchange_name
                .clone()
                .or_else(|| self.exchange.clone())
                .or_else(|| self.market.clone())
                .or_else(|| self.market_cap_figure_exchange.clone()),
        )
    }
}

#[derive(Deserialize)]
struct OptByDateNode {
    #[serde(rename = "expirationDate")]
    expiration_date: Option<i64>,
    calls: Option<Vec<OptContractNode>>,
    puts: Option<Vec<OptContractNode>>,
}

#[derive(Deserialize)]
struct OptContractNode {
    #[serde(rename = "contractSymbol")]
    contract_symbol: Option<String>,
    #[serde(rename = "expiration")]
    expiration: Option<i64>,
    #[serde(rename = "lastTradeDate")]
    last_trade_date: Option<i64>,
    strike: Option<f64>,
    #[serde(rename = "lastPrice")]
    last_price: Option<f64>,
    bid: Option<f64>,
    ask: Option<f64>,
    volume: Option<u64>,
    #[serde(rename = "openInterest")]
    open_interest: Option<u64>,
    #[serde(rename = "impliedVolatility")]
    implied_volatility: Option<f64>,
    #[serde(rename = "inTheMoney")]
    in_the_money: Option<bool>,
}

fn currency_from_result(node: &OptResultNode) -> Option<String> {
    node.quote.as_ref().and_then(|q| q.currency.clone())
}
