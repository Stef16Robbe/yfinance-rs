use base64::{Engine as _, engine::general_purpose};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use serde::Serialize;
use std::{borrow::Cow, collections::HashMap, time::Duration};
use tokio::{
    select,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Error as WsError,
        handshake::client::{Request, generate_key},
        protocol::Message as WsMessage,
    },
};

use crate::{
    YfClient, YfError,
    core::CallOptions,
    core::client::{CacheMode, RetryConfig, normalize_symbols},
    core::conversions::price_from_f64_with_currency_str,
    core::yahoo_vocab::{parse_yahoo_quote_type, yahoo_exchange_to_listing_currency},
};
use paft::domain::{AssetKind, Canonical, Instrument};
use paft::market::quote::QuoteUpdate;

const UNTYPED_STREAM_ASSET_KIND: &str = "YAHOO_STREAM_UNTYPED";

// Yahoo Finance websocket wire types (generated from `yaticker.proto`).
mod wire_ws {
    include!(concat!(env!("OUT_DIR"), "/yaticker.rs"));
}

fn untyped_stream_asset_kind() -> AssetKind {
    AssetKind::Other(Canonical::try_new(UNTYPED_STREAM_ASSET_KIND).expect("valid canonical token"))
}

// Use paft's QuoteUpdate which carries Price and DateTime<Utc>
// pub use paft::market::quote::QuoteUpdate; (imported above)

// Streaming quotes
//
// Volume semantics:
// - Yahoo sends cumulative intraday volume (`day_volume`). This crate converts it into
//   per-update deltas when producing `QuoteUpdate`s.
// - For each symbol, the first observed tick has `volume = None` (no delta yet).
// - On normal progression, `volume = Some(current - last)`.
// - On a detected reset (e.g., midnight rollover where `current < last`), emit the current
//   reading as the first delta of the new session: `volume = Some(current)`.
// - This applies to both WebSocket and Polling streams. The JSON/base64 decoder helper
//   (`decode_and_map_message`) is stateless and always returns `volume = None`.
// - When Yahoo omits instrument type data, or sends an unknown stream quote type, and no
//   typed instrument is cached, the emitted update uses
//   `AssetKind::Other("YAHOO_STREAM_UNTYPED")`. That fallback is deliberately not cached,
//   so later typed quote data can replace it.
//
// Implications:
// - If you need cumulative volume, accumulate the per-update `volume` values yourself or
//   use the `day_volume` from quote endpoints.
// - Expect `None` for only the first message per symbol; reset/rollover ticks emit
//   the current cumulative volume as the first delta of the new session.
/// Configuration for a polling-based quote stream.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// The interval at which to poll for new quote data.
    pub interval: Duration,
    /// If `true`, only emit updates when the price has changed.
    pub diff_only: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            diff_only: true,
        }
    }
}

/// A handle to a running quote stream, used to stop it gracefully.
pub struct StreamHandle {
    join: JoinHandle<()>,
    stop_tx: Option<oneshot::Sender<()>>,
}

impl StreamHandle {
    /// Stops the stream and waits for the background task to complete.
    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.join.await;
    }

    /// Aborts the background task immediately.
    pub fn abort(self) {
        self.join.abort();
    }
}

/// Defines the transport method for streaming quote data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamMethod {
    /// Attempt to use `WebSockets`, and fall back to polling if the connection fails. (Default)
    #[default]
    WebsocketWithFallback,
    /// Use `WebSockets` only.
    ///
    /// This is the preferred method for real-time data. `StreamBuilder::start` will fail if the
    /// initial WebSocket connection or subscription cannot be established.
    Websocket,
    /// Use polling over HTTP. This is a less efficient fallback option.
    Polling,
}

/// Builds and starts a real-time quote stream.
pub struct StreamBuilder {
    client: YfClient,
    symbols: Vec<String>,
    cfg: StreamConfig,
    method: StreamMethod,
    options: CallOptions,
}

impl StreamBuilder {
    /// Creates a new `StreamBuilder`.
    #[must_use]
    pub fn new(client: &YfClient) -> Self {
        Self {
            client: client.clone(),
            symbols: Vec::new(),
            cfg: StreamConfig::default(),
            method: StreamMethod::default(),
            options: CallOptions::default(),
        }
    }

    /// Sets the cache mode for this specific API call (only affects polling mode).
    #[must_use]
    pub const fn cache_mode(mut self, mode: CacheMode) -> Self {
        self.options.cache_mode = mode;
        self
    }

    /// Overrides the default retry policy for this specific API call (only affects polling mode).
    #[must_use]
    pub fn retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.options = self.options.with_retry_policy(cfg);
        self
    }

    /// Sets the symbols to stream.
    #[must_use]
    pub fn symbols<I, S>(mut self, syms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.symbols = syms.into_iter().map(std::convert::Into::into).collect();
        self
    }

    /// Adds a single symbol to the stream.
    #[must_use]
    pub fn add_symbol(mut self, sym: impl Into<String>) -> Self {
        self.symbols.push(sym.into());
        self
    }

    /// Sets the streaming transport method.
    #[must_use]
    pub const fn method(mut self, method: StreamMethod) -> Self {
        self.method = method;
        self
    }

    /// Sets the polling interval. (Only used for `Polling` and `WebsocketWithFallback` methods).
    #[must_use]
    pub const fn interval(mut self, dur: Duration) -> Self {
        self.cfg.interval = dur;
        self
    }

    /// If `true`, only emit updates when the price changes. (Only used for `Polling` method).
    #[must_use]
    pub const fn diff_only(mut self, yes: bool) -> Self {
        self.cfg.diff_only = yes;
        self
    }

    fn validated_symbols(&self) -> Result<Vec<String>, crate::core::YfError> {
        if self.symbols.is_empty() {
            return Err(crate::core::YfError::InvalidParams(
                "symbols list cannot be empty".into(),
            ));
        }
        if self.cfg.interval.is_zero() {
            return Err(crate::core::YfError::InvalidParams(
                "stream interval must be greater than zero".into(),
            ));
        }

        normalize_symbols(self.symbols.iter().map(String::as_str))
    }

    /// Starts the stream, returning a handle to control it and a channel receiver for quote updates.
    ///
    /// # Errors
    ///
    /// This method will return an error if no symbols have been added to the builder.
    ///
    /// With [`StreamMethod::Websocket`], this also returns initial WebSocket handshake and
    /// subscription errors. Runtime stream failures after startup close the receiver.
    ///
    /// With [`StreamMethod::WebsocketWithFallback`] and [`StreamMethod::Polling`], this returns
    /// after spawning the background task; connection or polling failures are handled inside the
    /// task.
    pub async fn start(
        &self,
    ) -> Result<(StreamHandle, tokio::sync::mpsc::Receiver<QuoteUpdate>), crate::core::YfError>
    {
        let symbols = self.validated_symbols()?;

        let (tx, rx) = tokio::sync::mpsc::channel::<QuoteUpdate>(1024);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (startup_tx, startup_rx) = match self.method {
            StreamMethod::Websocket => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (Some(tx), Some(rx))
            }
            StreamMethod::WebsocketWithFallback | StreamMethod::Polling => (None, None),
        };

        let join = tokio::spawn({
            let client = self.client.clone();
            let symbols = symbols.clone();
            let cfg = self.cfg.clone();
            let method = self.method;

            let mut stop_rx = stop_rx;

            let options = self.options.clone();

            async move {
                match method {
                    StreamMethod::Websocket => {
                        if let Err(e) =
                            run_websocket_stream(&client, symbols, tx, &mut stop_rx, startup_tx)
                                .await
                        {
                            crate::core::logging::trace_warn!(
                                error = %e,
                                "websocket stream failed"
                            );
                            #[cfg(not(feature = "tracing"))]
                            let _ = &e;
                        }
                    }
                    StreamMethod::WebsocketWithFallback => {
                        if let Err(e) = run_websocket_stream(
                            &client,
                            symbols.clone(),
                            tx.clone(),
                            &mut stop_rx,
                            None,
                        )
                        .await
                        {
                            crate::core::logging::trace_warn!(
                                error = %e,
                                "websocket stream failed; falling back to polling"
                            );
                            #[cfg(not(feature = "tracing"))]
                            let _ = &e;
                            run_polling_stream(client, symbols, cfg, tx, &mut stop_rx, &options)
                                .await;
                        }
                    }
                    StreamMethod::Polling => {
                        run_polling_stream(client, symbols, cfg, tx, &mut stop_rx, &options).await;
                    }
                }
            }
        });

        if let Some(startup_rx) = startup_rx {
            match startup_rx.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(YfError::InvalidData(
                        "websocket stream task ended before reporting startup".into(),
                    ));
                }
            }
        }

        Ok((
            StreamHandle {
                join,
                stop_tx: Some(stop_tx),
            },
            rx,
        ))
    }
}

#[derive(Serialize)]
struct WsSubscribe<'a> {
    subscribe: &'a [String],
}

fn report_websocket_startup_error(
    startup_tx: Option<oneshot::Sender<Result<(), YfError>>>,
    err: YfError,
) -> Result<(), YfError> {
    if let Some(tx) = startup_tx {
        if let Err(Err(err)) = tx.send(Err(err)) {
            return Err(err);
        }
        return Ok(());
    }

    Err(err)
}

fn websocket_remote_closed_error() -> YfError {
    WsError::ConnectionClosed.into()
}

fn websocket_stop_requested(stop_rx: &mut oneshot::Receiver<()>) -> bool {
    !matches!(stop_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty))
}

#[allow(clippy::too_many_lines)]
async fn run_websocket_stream(
    client: &YfClient,
    symbols: Vec<String>,
    tx: mpsc::Sender<QuoteUpdate>,
    stop_rx: &mut oneshot::Receiver<()>,
    startup_tx: Option<oneshot::Sender<Result<(), YfError>>>,
) -> Result<(), YfError> {
    let startup_result = async {
        let base = client.base_stream();
        let host = base
            .host_str()
            .ok_or_else(|| YfError::InvalidParams("URL has no host".into()))?;

        let request = Request::builder()
            .uri(base.as_str())
            .header("Host", host)
            .header("Origin", "https://finance.yahoo.com")
            .header("User-Agent", client.user_agent())
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Sec-WebSocket-Version", "13")
            .body(())
            .map_err(|e| {
                YfError::InvalidParams(format!("Failed to build websocket request: {e}"))
            })?;

        let (ws_stream, _) = connect_async(request).await?;
        let (mut write, read) = ws_stream.split();

        let sub_msg = serde_json::to_string(&WsSubscribe {
            subscribe: &symbols,
        })
        .map_err(YfError::Json)?;
        write.send(WsMessage::Text(sub_msg.into())).await?;

        Ok((write, read))
    }
    .await;

    let (mut write, mut read) = match startup_result {
        Ok(parts) => parts,
        Err(err) => return report_websocket_startup_error(startup_tx, err),
    };

    if let Some(tx) = startup_tx
        && tx.send(Ok(())).is_err()
    {
        return Ok(());
    }

    #[cfg(feature = "test-mode")]
    let mut recorded = false;

    let mut last_day_volume: HashMap<String, u64> = HashMap::new();
    let mut last_ts: HashMap<String, DateTime<Utc>> = HashMap::new();

    loop {
        select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        #[cfg(feature = "test-mode")]
                        {
                            if !recorded && std::env::var("YF_RECORD").ok().as_deref() == Some("1") {
                                if let Err(e) = crate::core::fixtures::record_fixture("stream_ws", "MULTI", "b64", &text) {
                                    crate::core::logging::trace_warn!(
                                        error = %e,
                                        "failed to write stream fixture"
                                    );
                                    #[cfg(not(feature = "tracing"))]
                                    let _ = e;
                                }
                                recorded = true;
                            }
                        }

                        match decode_ws_pricing(&text) {
                            Ok(ticker) => {
                                if let Some(update) = map_ws_pricing_to_update_with_delta(client, &ticker, &mut last_day_volume, &mut last_ts).await
                                    && tx.send(update).await.is_err() { return Ok(()); }
                            },
                            Err(e) => {
                                crate::core::logging::trace_debug!(
                                    error = %e,
                                    "websocket text frame decode failed"
                                );
                                #[cfg(not(feature = "tracing"))]
                                let _ = e;
                                // Non-price frames (acks/heartbeats) may lack "message"; ignore.
                            }
                        }
                    }
                    Some(Ok(WsMessage::Binary(bin))) => {
                        // Try to interpret as UTF-8 JSON-wrapped base64 first
                        let handled = if let Ok(as_text) = std::str::from_utf8(&bin) {
                            if let Ok(ticker) = decode_ws_pricing(as_text) {
                                if let Some(update) = map_ws_pricing_to_update_with_delta(client, &ticker, &mut last_day_volume, &mut last_ts).await
                                    && tx.send(update).await.is_err() { return Ok(()); }
                                true
                            } else { false }
                        } else { false };
                        // If not handled, treat as raw protobuf bytes
                        if !handled {
                            match wire_ws::PricingData::decode(&*bin) {
                                Ok(ticker) => {
                                    if let Some(update) = map_ws_pricing_to_update_with_delta(client, &ticker, &mut last_day_volume, &mut last_ts).await
                                        && tx.send(update).await.is_err() { return Ok(()); }
                                }
                                Err(e) => {
                                    crate::core::logging::trace_debug!(
                                        error = %e,
                                        "websocket binary frame decode failed"
                                    );
                                    #[cfg(not(feature = "tracing"))]
                                    let _ = e;
                                }
                            }
                        }
                    }
                    Some(Ok(WsMessage::Ping(payload))) => {
                        write.send(WsMessage::Pong(payload)).await?;
                    }
                    Some(Ok(WsMessage::Pong(_) | WsMessage::Frame(_))) => {}
                    Some(Ok(WsMessage::Close(_))) => {
                        write.flush().await?;
                        if websocket_stop_requested(stop_rx) {
                            break;
                        }
                        return Err(websocket_remote_closed_error());
                    }
                    Some(Err(e)) => return Err(e.into()),
                    None => {
                        if websocket_stop_requested(stop_rx) {
                            break;
                        }
                        return Err(websocket_remote_closed_error());
                    }
                }
            },
            _ = &mut *stop_rx => {
                break;
            }
        }
    }
    Ok(())
}

fn decode_ws_pricing(text: &str) -> Result<wire_ws::PricingData, YfError> {
    let s = text.trim();
    let b64_cow: Cow<str> = if s.starts_with('{') {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => {
                let msg = v.get("message").and_then(|m| m.as_str()).ok_or_else(|| {
                    YfError::MissingData("ws json message missing 'message' field".into())
                })?;
                Cow::Owned(msg.to_string())
            }
            Err(_) => Cow::Borrowed(s),
        }
    } else {
        Cow::Borrowed(s)
    };
    let decoded = general_purpose::STANDARD
        .decode(b64_cow.as_ref())
        .map_err(YfError::Base64)?;
    let ticker = wire_ws::PricingData::decode(&*decoded)?;
    Ok(ticker)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VolumeDelta {
    value: Option<u64>,
    changed: bool,
}

fn volume_delta_from_cumulative(
    last_by_symbol: &mut HashMap<String, u64>,
    symbol: &str,
    current: Option<u64>,
) -> VolumeDelta {
    let Some(current) = current else {
        return VolumeDelta {
            value: None,
            changed: false,
        };
    };

    let previous = last_by_symbol.insert(symbol.to_string(), current);
    let value = match previous {
        Some(previous) if current >= previous => Some(current - previous),
        Some(_) => Some(current),
        None => None,
    };

    VolumeDelta {
        value,
        changed: value.is_some_and(|delta| delta > 0),
    }
}

async fn resolve_stream_instrument(
    client: &YfClient,
    symbol: &str,
    kind: Option<AssetKind>,
) -> Option<Instrument> {
    if let Some(instrument) = client.cached_instrument(symbol).await {
        return Some(instrument);
    }

    let should_cache = kind.is_some();
    let kind = kind.unwrap_or_else(untyped_stream_asset_kind);
    let Ok(instrument) = Instrument::from_symbol(symbol, kind) else {
        crate::core::logging::trace_debug!(
            symbol = %symbol,
            "skipping stream update with invalid symbol"
        );
        return None;
    };

    if should_cache {
        client
            .store_instrument(symbol.to_string(), instrument.clone())
            .await;
    }

    Some(instrument)
}

fn ws_pricing_timestamp(ticker: &wire_ws::PricingData) -> Result<DateTime<Utc>, YfError> {
    DateTime::from_timestamp_millis(ticker.time).ok_or_else(|| {
        YfError::InvalidParams(format!(
            "Invalid timestamp in stream message: {}",
            ticker.time
        ))
    })
}

fn ws_pricing_to_update(
    ticker: &wire_ws::PricingData,
    instrument: Instrument,
    timestamp: DateTime<Utc>,
    volume: Option<u64>,
) -> QuoteUpdate {
    let currency_str = ws_currency_str(ticker);

    QuoteUpdate {
        instrument,
        price: ws_price_from_f32(ticker.price, currency_str),
        previous_close: ws_price_from_f32(ticker.previous_close, currency_str),
        ts: timestamp,
        volume,
        provider: (),
    }
}

fn ws_currency_str(ticker: &wire_ws::PricingData) -> Option<&str> {
    nonempty_str(&ticker.currency)
        .or_else(|| nonempty_str(&ticker.exchange).and_then(yahoo_exchange_to_listing_currency))
}

fn nonempty_str(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

const fn stream_quote_type_to_asset_kind(quote_type: i32) -> Option<AssetKind> {
    match quote_type {
        8 => Some(AssetKind::Equity),
        9 => Some(AssetKind::Index),
        11 | 12 | 20 => Some(AssetKind::Fund),
        13 => Some(AssetKind::Option),
        14 => Some(AssetKind::Forex),
        15 => Some(AssetKind::Warrant),
        17 => Some(AssetKind::Bond),
        18 => Some(AssetKind::Future),
        23 => Some(AssetKind::Commodity),
        41 => Some(AssetKind::Crypto),
        _ => None,
    }
}

fn ws_price_from_f32(value: f32, currency_str: Option<&str>) -> Option<paft::money::Price> {
    let value = f64::from(value);
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    price_from_f64_with_currency_str(value, currency_str)
}

async fn map_ws_pricing_to_update_with_delta(
    client: &YfClient,
    ticker: &wire_ws::PricingData,
    last_vol: &mut HashMap<String, u64>,
    last_ts: &mut HashMap<String, DateTime<Utc>>,
) -> Option<QuoteUpdate> {
    let instrument = resolve_stream_instrument(
        client,
        &ticker.id,
        stream_quote_type_to_asset_kind(ticker.quote_type),
    )
    .await?;
    let timestamp = match ws_pricing_timestamp(ticker) {
        Ok(timestamp) => timestamp,
        Err(error) => {
            crate::core::logging::trace_debug!(
                error = %error,
                timestamp_millis = ticker.time,
                symbol = %ticker.id,
                "skipping websocket update with invalid timestamp"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = error;
            return None;
        }
    };

    // If out-of-order, emit but don't mutate state; volume=None
    if let Some(prev_ts) = last_ts.get(&ticker.id)
        && timestamp < *prev_ts
    {
        return Some(ws_pricing_to_update(ticker, instrument, timestamp, None));
    }

    let cur_vol = u64::try_from(ticker.day_volume).unwrap_or(0);
    let volume = volume_delta_from_cumulative(last_vol, &ticker.id, Some(cur_vol));

    // Update state only for in-order ticks
    last_ts.insert(ticker.id.clone(), timestamp);

    Some(ws_pricing_to_update(
        ticker,
        instrument,
        timestamp,
        volume.value,
    ))
}

/// Decodes a single base64-encoded protobuf message from the Yahoo Finance WebSocket stream.
#[doc(hidden)]
pub fn decode_and_map_message(text: &str) -> Result<QuoteUpdate, YfError> {
    let ticker = decode_ws_pricing(text)?;
    let kind = stream_quote_type_to_asset_kind(ticker.quote_type)
        .unwrap_or_else(untyped_stream_asset_kind);
    let instrument = Instrument::from_symbol(&ticker.id, kind)
        .map_err(|_| YfError::InvalidParams(format!("ws symbol invalid: {}", ticker.id)))?;

    let timestamp = match ws_pricing_timestamp(&ticker) {
        Ok(timestamp) => timestamp,
        Err(error) => {
            crate::core::logging::trace_warn!(
                error = %error,
                timestamp_millis = ticker.time,
                symbol = %ticker.id,
                "received websocket update with invalid timestamp"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = &error;
            return Err(error);
        }
    };

    Ok(ws_pricing_to_update(&ticker, instrument, timestamp, None))
}

async fn run_polling_stream(
    client: crate::core::YfClient,
    symbols: Vec<String>,
    cfg: StreamConfig,
    tx: tokio::sync::mpsc::Sender<QuoteUpdate>,
    stop_rx: &mut tokio::sync::oneshot::Receiver<()>,
    options: &CallOptions,
) {
    let mut ticker = tokio::time::interval(cfg.interval);
    let mut last_price: HashMap<String, Option<f64>> = HashMap::new();
    let mut last_day_volume: HashMap<String, u64> = HashMap::new();

    let symbol_slices: Vec<&str> = symbols.iter().map(AsRef::as_ref).collect();

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if tx.is_closed() { break; }
                match crate::core::quotes::fetch_v7_quotes(&client, &symbol_slices, options).await {
                    Ok(quotes) => {
                        for q in quotes {
                            let ts = q
                                .regular_market_time
                                .and_then(|t| DateTime::from_timestamp(t, 0))
                                .unwrap_or_else(Utc::now);
                            let sym_s = q.symbol.clone().unwrap_or_default();
                            let lp = q.regular_market_price.or(q.regular_market_previous_close);

                            // Track price changes when diff_only is enabled
                            let price_changed = if cfg.diff_only {
                                let prev = last_price.insert(sym_s.clone(), lp);
                                prev != Some(lp)
                            } else {
                                true
                            };

                            let volume = volume_delta_from_cumulative(
                                &mut last_day_volume,
                                &sym_s,
                                q.regular_market_volume,
                            );

                            // With diff_only, emit if either price OR volume changed
                            if cfg.diff_only && !price_changed && !volume.changed {
                                continue;
                            }

                            let currency_str = q.currency.as_deref();
                            let kind = q
                                .quote_type
                                .as_deref()
                                .and_then(|value| parse_yahoo_quote_type(value).ok());
                            let Some(instrument) =
                                resolve_stream_instrument(&client, &sym_s, kind).await
                            else {
                                continue;
                            };
                            if tx.send(QuoteUpdate {
                                instrument,
                                price: lp.and_then(|v| price_from_f64_with_currency_str(v, currency_str)),
                                previous_close: q.regular_market_previous_close.and_then(|v| price_from_f64_with_currency_str(v, currency_str)),
                                ts,
                                volume: volume.value,
                                provider: (),
                            }).await.is_err() {
                                // Break outer loop if receiver is dropped
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        crate::core::logging::trace_debug!(
                            error = %e,
                            "polling stream quote fetch failed"
                        );
                        #[cfg(not(feature = "tracing"))]
                        let _ = e;
                    }
                }
                if tx.is_closed() { break; }
            }
            _ = &mut *stop_rx => { break; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VolumeDelta, stream_quote_type_to_asset_kind, volume_delta_from_cumulative};
    use paft::domain::AssetKind;
    use std::collections::HashMap;

    #[test]
    fn stream_quote_type_to_asset_kind_maps_known_yahoo_codes() {
        for (code, expected) in [
            (8, AssetKind::Equity),
            (9, AssetKind::Index),
            (11, AssetKind::Fund),
            (12, AssetKind::Fund),
            (13, AssetKind::Option),
            (14, AssetKind::Forex),
            (15, AssetKind::Warrant),
            (17, AssetKind::Bond),
            (18, AssetKind::Future),
            (20, AssetKind::Fund),
            (23, AssetKind::Commodity),
            (41, AssetKind::Crypto),
        ] {
            assert_eq!(stream_quote_type_to_asset_kind(code), Some(expected));
        }
    }

    #[test]
    fn stream_quote_type_to_asset_kind_rejects_non_instrument_codes() {
        assert_eq!(stream_quote_type_to_asset_kind(0), None);
        assert_eq!(stream_quote_type_to_asset_kind(7), None);
        assert_eq!(stream_quote_type_to_asset_kind(1000), None);
        assert_eq!(stream_quote_type_to_asset_kind(i32::MAX), None);
    }

    #[test]
    fn volume_delta_from_cumulative_tracks_first_delta_reset_and_missing_values() {
        let mut last_by_symbol = HashMap::new();

        assert_eq!(
            volume_delta_from_cumulative(&mut last_by_symbol, "MSFT", None),
            VolumeDelta {
                value: None,
                changed: false
            }
        );
        assert_eq!(
            volume_delta_from_cumulative(&mut last_by_symbol, "MSFT", Some(100)),
            VolumeDelta {
                value: None,
                changed: false
            }
        );
        assert_eq!(
            volume_delta_from_cumulative(&mut last_by_symbol, "MSFT", Some(125)),
            VolumeDelta {
                value: Some(25),
                changed: true
            }
        );
        assert_eq!(
            volume_delta_from_cumulative(&mut last_by_symbol, "MSFT", Some(125)),
            VolumeDelta {
                value: Some(0),
                changed: false
            }
        );
        assert_eq!(
            volume_delta_from_cumulative(&mut last_by_symbol, "MSFT", Some(10)),
            VolumeDelta {
                value: Some(10),
                changed: true
            }
        );
    }
}
