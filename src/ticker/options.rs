use std::str::FromStr;

use serde::Deserialize;
use url::Url;

use crate::{
    YfClient, YfError,
    core::{
        client::{CacheMode, RetryConfig},
        conversions::{decimal_from_f64, price_from_f64, string_to_exchange},
        net,
    },
    screener::YahooQuoteType,
};
use paft::money::Currency;

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

pub async fn option_chain(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<OptionChain, YfError> {
    let (body, used_url) =
        fetch_options_raw(client, symbol, date, cache_mode, retry_override).await?;
    let env: OptEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let first = env
        .option_chain
        .and_then(|oc| oc.result)
        .and_then(|mut v| v.pop())
        .ok_or_else(|| YfError::MissingData("empty options result".into()))?;

    let currency_from_response = currency_from_result(&first);
    let underlying_from_response = underlying_instrument_from_result(&first);

    let Some(od) = first.options.and_then(|mut v| v.pop()) else {
        return Ok(OptionChain {
            contracts: vec![],
            provider: (),
        });
    };

    let expiration = od.expiration_date.or_else(|| {
        used_url.query().and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("date=").and_then(|v| v.parse::<i64>().ok()))
        })
    });

    let (currency, underlying) = if let Some(currency) = currency_from_response {
        (
            currency,
            underlying_instrument(client, symbol, underlying_from_response).await?,
        )
    } else {
        let quote = super::quote::fetch_quote(client, symbol, cache_mode, retry_override).await?;
        let currency = quote
            .price
            .as_ref()
            .map(|m| m.currency().clone())
            .or_else(|| quote.previous_close.as_ref().map(|m| m.currency().clone()))
            .ok_or_else(|| {
                YfError::MissingData("unable to determine currency from options or quote".into())
            })?;
        (
            currency,
            underlying_from_response.unwrap_or(quote.instrument),
        )
    };

    let map_side = |side: Option<Vec<OptContractNode>>,
                    option_side: OptionSide,
                    currency: &Currency|
     -> Vec<OptionContract> {
        side.unwrap_or_default()
            .into_iter()
            .filter_map(|c| {
                let exp_ts = c.expiration.or(expiration)?;
                let exp_dt = Utc.timestamp_opt(exp_ts, 0).single()?;
                let exp_date: NaiveDate = exp_dt.date_naive();
                let strike = c
                    .strike
                    .and_then(|strike| price_from_f64(strike, currency.clone()))?;
                let key = OptionContractKey::new(underlying.clone(), option_side, strike, exp_date);
                let contract_instrument = c
                    .contract_symbol
                    .as_deref()
                    .and_then(|sym| Instrument::from_symbol(sym, AssetKind::Option).ok());

                Some(OptionContract {
                    key,
                    contract_instrument,
                    price: c
                        .last_price
                        .and_then(|v| price_from_f64(v, currency.clone())),
                    bid: c.bid.and_then(|v| price_from_f64(v, currency.clone())),
                    ask: c.ask.and_then(|v| price_from_f64(v, currency.clone())),
                    volume: c.volume,
                    open_interest: c.open_interest,
                    implied_volatility: c.implied_volatility.and_then(decimal_from_f64),
                    in_the_money: c.in_the_money,
                    expiration_at: Some(exp_dt),
                    last_trade_at: c
                        .last_trade_date
                        .and_then(|ts| Utc.timestamp_opt(ts, 0).single()),
                    greeks: None,
                    provider: (),
                })
            })
            .collect()
    };

    Ok(OptionChain {
        contracts: map_side(od.calls, OptionSide::Call, &currency)
            .into_iter()
            .chain(map_side(od.puts, OptionSide::Put, &currency))
            .collect(),
        provider: (),
    })
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

    Instrument::from_symbol(symbol, AssetKind::Equity)
        .map_err(|err| YfError::InvalidParams(format!("invalid option underlying symbol: {err}")))
}

fn underlying_instrument_from_result(node: &OptResultNode) -> Option<Instrument> {
    let quote = node.quote.as_ref();
    let symbol = node
        .underlying_symbol
        .as_deref()
        .or_else(|| quote.and_then(|quote| quote.symbol.as_deref()))?;
    let kind = quote
        .and_then(|quote| quote.quote_type.as_deref())
        .map_or(AssetKind::Equity, quote_type_to_asset_kind);
    let exchange = quote.and_then(OptQuoteNode::exchange);

    match exchange {
        Some(exchange) => Instrument::from_symbol_and_exchange(symbol, exchange, kind),
        None => Instrument::from_symbol(symbol, kind),
    }
    .ok()
}

fn quote_type_to_asset_kind(value: &str) -> AssetKind {
    YahooQuoteType::parse(value)
        .map(YahooQuoteType::asset_kind)
        .or_else(|| value.parse::<AssetKind>().ok())
        .unwrap_or(AssetKind::Equity)
}

/* ---------------- Internal: raw fetch with auth fallback ---------------- */

async fn fetch_options_raw(
    client: &YfClient,
    symbol: &str,
    date: Option<i64>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<(String, Url), YfError> {
    let http = client.http().clone();
    let base = client.base_options_v7();

    let mut url = base.join(symbol)?;
    {
        let mut qp = url.query_pairs_mut();
        if let Some(d) = date {
            qp.append_pair("date", &d.to_string());
        }
    }

    if cache_mode == CacheMode::Use
        && let Some(body) = client.cache_get(&url).await
    {
        return Ok((body, url));
    }

    let req = http.get(url.clone()).header("accept", "application/json");
    let mut resp = client.send_with_retry(req, retry_override).await?;

    if resp.status().is_success() {
        let fixture_key = date.map_or_else(|| symbol.to_string(), |d| format!("{symbol}_{d}"));
        let body = net::get_success_text(resp, &url, "options_v7", &fixture_key, "json").await?;
        if cache_mode != CacheMode::Bypass {
            client.cache_put(&url, &body, None).await;
        }
        return Ok((body, url));
    }

    let code = resp.status().as_u16();
    if code != 401 && code != 403 {
        return Err(net::status_error_code(code, &url));
    }

    client.ensure_credentials().await?;
    let crumb = client
        .crumb()
        .await
        .ok_or_else(|| net::status_error_code(code, &url))?;

    let mut url2 = base.join(symbol)?;
    {
        let mut qp = url2.query_pairs_mut();
        if let Some(d) = date {
            qp.append_pair("date", &d.to_string());
        }
        qp.append_pair("crumb", &crumb);
    }

    let req2 = http.get(url2.clone()).header("accept", "application/json");
    resp = client.send_with_retry(req2, retry_override).await?;

    if !resp.status().is_success() {
        return Err(net::status_error(resp.status(), &url2));
    }

    let fixture_key = date.map_or_else(|| symbol.to_string(), |d| format!("{symbol}_{d}"));
    let body = net::get_success_text(resp, &url2, "options_v7", &fixture_key, "json").await?;
    if cache_mode != CacheMode::Bypass {
        client.cache_put(&url2, &body, None).await;
    }
    Ok((body, url2))
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

fn currency_from_result(node: &OptResultNode) -> Option<Currency> {
    node.quote
        .as_ref()
        .and_then(|q| q.currency.as_deref())
        .and_then(|code| Currency::from_str(code).ok())
}
