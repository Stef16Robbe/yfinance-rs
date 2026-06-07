use base64::{Engine as _, engine::general_purpose};
use chrono::{DateTime, Utc};
use prost::Message;
use std::{
    io::ErrorKind,
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Instant,
};
use tokio::time::{Duration, timeout};
use tokio_tungstenite::tungstenite::{Message as TestWsMessage, WebSocket, accept};
use url::Url;
use yfinance_rs::core::client::CacheMode;
use yfinance_rs::{AssetKind, StreamMethod};

#[derive(Clone, PartialEq, Message)]
struct TestPricingData {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(float, tag = "2")]
    pub price: f32,
    #[prost(sint64, tag = "3")]
    pub time: i64,
    #[prost(string, tag = "4")]
    pub currency: String,
    #[prost(string, tag = "5")]
    pub exchange: String,
    #[prost(int32, tag = "6")]
    pub quote_type: i32,
    #[prost(sint64, tag = "9")]
    pub day_volume: i64,
    #[prost(float, tag = "16")]
    pub previous_close: f32,
    #[prost(sint64, tag = "27")]
    pub price_hint: i64,
}

type WebsocketHandler = Box<dyn FnOnce(&mut WebSocket<TcpStream>) + Send>;

fn encode_test_pricing_data(message: &TestPricingData) -> String {
    let mut bytes = Vec::new();
    message.encode(&mut bytes).unwrap();
    general_purpose::STANDARD.encode(bytes)
}

fn spawn_websocket_server(
    handler: impl FnOnce(&mut WebSocket<TcpStream>) + Send + 'static,
) -> (Url, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let url = Url::parse(&format!("ws://{addr}/")).unwrap();

    let handle = thread::spawn(move || {
        let stream = accept_websocket_connection(&listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();

        let mut websocket = accept(stream).expect("server should accept websocket handshake");
        handler(&mut websocket);
    });

    (url, handle)
}

fn spawn_websocket_server_sequence(handlers: Vec<WebsocketHandler>) -> (Url, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let url = Url::parse(&format!("ws://{addr}/")).unwrap();

    let handle = thread::spawn(move || {
        for handler in handlers {
            let stream = accept_websocket_connection(&listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();

            let mut websocket = accept(stream).expect("server should accept websocket handshake");
            handler(&mut websocket);
        }
    });

    (url, handle)
}

fn accept_websocket_connection(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("failed to accept websocket client connection: {err}"),
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for websocket client connection"
        );
    }
}

fn assert_subscription(message: TestWsMessage, symbols: &[&str]) {
    let TestWsMessage::Text(text) = message else {
        panic!("expected websocket subscription text frame, got {message:?}");
    };

    let subscription: serde_json::Value =
        serde_json::from_str(text.as_str()).expect("subscription should be valid JSON");
    assert_eq!(subscription["subscribe"], serde_json::json!(symbols));
}

async fn wait_for_mock_calls(
    mock: &httpmock::Mock<'_>,
    expected: usize,
    timeout_duration: Duration,
) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout_duration {
        if mock.calls_async().await >= expected {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    mock.calls_async().await >= expected
}

fn websocket_url_for_mock_server(server: &httpmock::MockServer, path: &str) -> Url {
    let base_url = server.base_url().replacen("http://", "ws://", 1);
    Url::parse(&format!("{base_url}{path}")).unwrap()
}

#[tokio::test]
async fn stream_websocket_maps_numeric_quote_type_without_cached_instrument() {
    let (stream_url, websocket_thread) = spawn_websocket_server(|websocket| {
        let subscription = websocket
            .read()
            .expect("server should receive websocket subscription");
        assert_subscription(subscription, &["AAPL"]);

        let payload = encode_test_pricing_data(&TestPricingData {
            id: "AAPL".to_string(),
            price: 314.6,
            time: 1_780_426_509_000,
            currency: "USD".to_string(),
            exchange: "NMS".to_string(),
            quote_type: 8,
            day_volume: 26_248_990,
            previous_close: 313.0,
            price_hint: 2,
        });
        let frame = serde_json::json!({ "message": payload }).to_string();
        websocket
            .send(TestWsMessage::Text(frame.into()))
            .expect("server should send pricing update");
    });

    let client = yfinance_rs::YfClient::builder()
        .base_stream(stream_url)
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::Websocket);

    let (handle, mut rx) = builder.start().await.unwrap();
    let update = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for websocket update")
        .expect("stream closed before websocket update");
    handle.abort();
    websocket_thread
        .join()
        .expect("websocket server thread panicked");

    assert_eq!(update.instrument.symbol.as_str(), "AAPL");
    assert!(matches!(update.instrument.kind, AssetKind::Equity));
}

#[tokio::test]
async fn stream_websocket_reports_initial_connection_failure() {
    let client = yfinance_rs::YfClient::builder()
        .base_stream(Url::parse("wss://invalid-url-for-testing.invalid/").unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::Websocket);

    let Err(err) = builder.start().await else {
        panic!("websocket start should report initial connection failure");
    };

    assert!(
        matches!(err, yfinance_rs::YfError::Websocket(_)),
        "expected websocket error, got: {err}"
    );
}

#[tokio::test]
async fn stream_websocket_status_error_redacts_sensitive_query_params() {
    let server = httpmock::MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/stream");
        then.status(200).body("");
    });

    let base_url = format!(
        "{}{}",
        websocket_url_for_mock_server(&server, "/stream"),
        "?version=2&crumb=s3cr3t&token=t"
    );
    let client = yfinance_rs::YfClient::builder()
        .base_stream(Url::parse(&base_url).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::Websocket);

    let Err(err) = builder.start().await else {
        panic!("websocket startup should fail on non-upgrade status");
    };

    let message = err.to_string();
    assert!(matches!(err, yfinance_rs::YfError::Status { .. }));
    assert!(message.contains("crumb=REDACTED"));
    assert!(message.contains("token=REDACTED"));
    assert!(!message.contains("s3cr3t"));
    assert!(!message.contains("token=t"));
    mock.assert();
}

#[tokio::test]
async fn stream_websocket_startup_respects_connect_timeout() {
    let server = httpmock::MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/stream");
        then.status(200).delay(Duration::from_secs(2)).body("");
    });

    let client = yfinance_rs::YfClient::builder()
        .base_stream(websocket_url_for_mock_server(&server, "/stream"))
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::Websocket)
        .websocket_connect_timeout(Duration::from_millis(50));

    let Err(err) = timeout(Duration::from_secs(1), builder.start())
        .await
        .expect("websocket startup should finish after connect timeout")
    else {
        panic!("websocket startup should fail after connect timeout");
    };

    assert!(
        matches!(err, yfinance_rs::YfError::Websocket(_)),
        "expected websocket timeout error, got: {err}"
    );
    assert!(
        err.to_string().contains("startup"),
        "expected startup timeout message, got: {err}"
    );
    assert!(
        wait_for_mock_calls(&mock, 1, Duration::from_secs(1)).await,
        "websocket startup request should have reached the test server"
    );
}

#[tokio::test]
async fn stream_websocket_fallback_stop_cancels_startup() {
    let server = httpmock::MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/stream");
        then.status(200).delay(Duration::from_secs(2)).body("");
    });

    let client = yfinance_rs::YfClient::builder()
        .base_stream(websocket_url_for_mock_server(&server, "/stream"))
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::WebsocketWithFallback)
        .websocket_connect_timeout(Duration::from_secs(5));

    let (handle, _rx) = builder.start().await.unwrap();

    assert!(
        wait_for_mock_calls(&mock, 1, Duration::from_secs(1)).await,
        "websocket startup did not begin"
    );

    timeout(Duration::from_millis(250), handle.stop())
        .await
        .expect("stop should not wait for the websocket startup request to finish");
}

#[tokio::test]
async fn stream_websocket_replies_to_ping() {
    let (stream_url, websocket_thread) = spawn_websocket_server(|websocket| {
        let subscription = websocket
            .read()
            .expect("server should receive websocket subscription");
        assert_subscription(subscription, &["AAPL"]);

        let payload = b"heartbeat".to_vec();
        websocket
            .send(TestWsMessage::Ping(payload.clone().into()))
            .expect("server should send ping");

        let reply = websocket
            .read()
            .expect("server should receive websocket pong");
        assert_eq!(reply, TestWsMessage::Pong(payload.into()));
    });

    let client = yfinance_rs::YfClient::builder()
        .base_stream(stream_url)
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::Websocket);

    let (handle, _rx) = builder.start().await.unwrap();
    let server_result = tokio::task::spawn_blocking(move || websocket_thread.join())
        .await
        .expect("websocket server join task panicked");
    handle.stop().await;

    server_result.expect("websocket server thread panicked");
}

#[tokio::test]
async fn stream_websocket_fallback_to_polling_after_idle_timeout() {
    let server = crate::common::setup_server();

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let (stream_url, websocket_thread) = spawn_websocket_server(|websocket| {
        let subscription = websocket
            .read()
            .expect("server should receive websocket subscription");
        assert_subscription(subscription, &["AAPL"]);

        thread::sleep(Duration::from_millis(300));
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_stream(stream_url)
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::WebsocketWithFallback)
        .interval(Duration::from_millis(40))
        .websocket_idle_timeout(Duration::from_millis(80));

    let (handle, mut rx) = builder.start().await.unwrap();

    let got = timeout(Duration::from_secs(3), rx.recv()).await;
    handle.abort();
    websocket_thread
        .join()
        .expect("websocket server thread panicked");

    assert!(
        mock.calls_async().await >= 1,
        "fallback should poll quotes after websocket idle timeout"
    );

    let update = got
        .expect("timed out waiting for fallback stream update")
        .expect("stream closed without falling back after websocket idle timeout");

    assert_eq!(update.instrument.symbol.as_str(), "AAPL");
}

#[tokio::test]
async fn stream_websocket_fallback_to_polling_offline() {
    let server = crate::common::setup_server();

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_stream(Url::parse("wss://invalid-url-for-testing.invalid/").unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::WebsocketWithFallback)
        .interval(Duration::from_millis(40));

    let (handle, mut rx) = builder.start().await.unwrap();

    let got = timeout(Duration::from_secs(3), rx.recv()).await;
    handle.abort();

    assert!(
        mock.calls_async().await >= 1,
        "fallback should poll quotes after websocket startup failure"
    );

    let update = got
        .expect("timed out waiting for cached stream update")
        .expect("stream closed without emitting an update");

    assert_eq!(update.instrument.symbol.as_str(), "AAPL");
    assert!(
        update
            .price
            .as_ref()
            .map_or(0.0, yfinance_rs::core::conversions::money_to_f64)
            > 0.0,
        "cached price should be > 0"
    );
}

#[tokio::test]
async fn stream_websocket_fallback_to_polling_after_remote_close() {
    let server = crate::common::setup_server();

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let (stream_url, websocket_thread) = spawn_websocket_server(|websocket| {
        let subscription = websocket
            .read()
            .expect("server should receive websocket subscription");
        assert_subscription(subscription, &["AAPL"]);

        websocket
            .send(TestWsMessage::Close(None))
            .expect("server should send close frame");
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_stream(stream_url)
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::WebsocketWithFallback)
        .interval(Duration::from_millis(40));

    let (handle, mut rx) = builder.start().await.unwrap();

    let got = timeout(Duration::from_secs(3), rx.recv()).await;
    handle.abort();
    websocket_thread
        .join()
        .expect("websocket server thread panicked");

    assert!(
        mock.calls_async().await >= 1,
        "fallback should poll quotes after websocket remote close"
    );

    let update = got
        .expect("timed out waiting for fallback stream update")
        .expect("stream closed without falling back to polling");

    assert_eq!(update.instrument.symbol.as_str(), "AAPL");
}

#[tokio::test]
async fn stream_websocket_fallback_reconnects_after_polling() {
    let server = crate::common::setup_server();

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let ws_update_price = 314.6_f32;
    let (stream_url, websocket_thread) = spawn_websocket_server_sequence(vec![
        Box::new(|websocket| {
            let subscription = websocket
                .read()
                .expect("server should receive first websocket subscription");
            assert_subscription(subscription, &["AAPL"]);

            websocket
                .send(TestWsMessage::Close(None))
                .expect("server should send close frame");
        }),
        Box::new(move |websocket| {
            let subscription = websocket
                .read()
                .expect("server should receive second websocket subscription");
            assert_subscription(subscription, &["AAPL"]);

            let payload = encode_test_pricing_data(&TestPricingData {
                id: "AAPL".to_string(),
                price: ws_update_price,
                time: 1_780_426_509_000,
                currency: "USD".to_string(),
                exchange: "NMS".to_string(),
                quote_type: 8,
                day_volume: 26_248_990,
                previous_close: 313.0,
                price_hint: 2,
            });
            let frame = serde_json::json!({ "message": payload }).to_string();
            websocket
                .send(TestWsMessage::Text(frame.into()))
                .expect("server should send pricing update after reconnect");
        }),
    ]);

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_stream(stream_url)
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::WebsocketWithFallback)
        .interval(Duration::from_millis(40));

    let (handle, mut rx) = builder.start().await.unwrap();

    let websocket_update = timeout(Duration::from_secs(3), async {
        while let Some(update) = rx.recv().await {
            let price = update
                .price
                .as_ref()
                .map(yfinance_rs::core::conversions::money_to_f64);
            if price.is_some_and(|price| (price - f64::from(ws_update_price)).abs() < 0.01) {
                return update;
            }
        }
        panic!("stream closed before websocket reconnect update");
    })
    .await
    .expect("timed out waiting for websocket reconnect update");

    handle.abort();
    websocket_thread
        .join()
        .expect("websocket server thread panicked");

    assert!(
        mock.calls_async().await >= 1,
        "fallback should poll quotes before reconnecting"
    );
    assert_eq!(websocket_update.instrument.symbol.as_str(), "AAPL");
}

#[tokio::test]
async fn stream_polling_explicitly_offline() {
    let server = crate::common::setup_server();

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "MSFT", "json"));
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(50));

    let (handle, mut rx) = builder.start().await.unwrap();
    let got = timeout(Duration::from_secs(3), rx.recv()).await;
    handle.abort();
    mock.assert();

    assert!(got.is_ok());
}

#[tokio::test]
async fn stream_polling_stop_cancels_in_flight_quote_fetch() {
    let server = crate::common::setup_server();

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .delay(Duration::from_secs(2))
            .body(crate::common::fixture("quote_v7", "MSFT", "json"));
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(1))
        .cache_mode(CacheMode::Bypass);

    let (handle, _rx) = builder.start().await.unwrap();

    assert!(
        wait_for_mock_calls(&mock, 1, Duration::from_secs(1)).await,
        "polling stream did not start the quote request"
    );

    timeout(Duration::from_millis(250), handle.stop())
        .await
        .expect("stop should not wait for the in-flight quote request to finish");
}

#[tokio::test]
async fn stream_polling_omits_malformed_optional_price_and_keeps_siblings() {
    let server = crate::common::setup_server();

    let body = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "AAPL",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": "not-a-number",
                    "currency": "USD"
                },
                {
                    "symbol": "MSFT",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": 420.00,
                    "regularMarketPreviousClose": 419.00,
                    "regularMarketVolume": "1000",
                    "currency": "USD"
                }
            ],
            "error": null
        }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL,MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(50));

    let (handle, mut rx) = builder.start().await.unwrap();
    let first = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for polling stream update")
        .expect("stream closed without emitting an update");
    let second = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for second polling stream update")
        .expect("stream closed before second update");
    handle.abort();
    mock.assert();

    assert_eq!(first.instrument.symbol.as_str(), "AAPL");
    assert!(first.price.is_none());
    assert_eq!(second.instrument.symbol.as_str(), "MSFT");
    assert_eq!(
        second.volume.as_ref().map(ToString::to_string),
        Some("1000".into())
    );
}

#[tokio::test]
async fn stream_polling_uses_regular_market_time_as_update_timestamp() {
    let server = crate::common::setup_server();
    let provider_time = 1_761_768_001;

    let body = format!(
        r#"{{
            "quoteResponse": {{
                "result": [
                    {{
                        "symbol": "MSFT",
                        "quoteType": "EQUITY",
                        "regularMarketPrice": 420.00,
                        "regularMarketPreviousClose": 419.00,
                        "regularMarketVolume": 1000,
                        "regularMarketTime": {provider_time},
                        "currency": "USD"
                    }}
                ],
                "error": null
            }}
        }}"#
    );

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(50));

    let (handle, mut rx) = builder.start().await.unwrap();
    let update = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for polling stream update")
        .expect("stream closed without emitting an update");
    handle.abort();
    mock.assert();

    assert_eq!(
        update.ts,
        DateTime::<Utc>::from_timestamp(provider_time, 0).unwrap()
    );
}

#[tokio::test]
async fn stream_polling_diff_only_ignores_volume_only_change() {
    let server = crate::common::setup_server();

    // First response: price P, volume V1
    let body1 = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "MSFT",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": 420.00,
                    "regularMarketPreviousClose": 420.00,
                    "regularMarketVolume": 1000,
                    "currency": "USD"
                }
            ],
            "error": null
        }
    }"#;

    // Second response: same price P, higher volume V2
    let body2 = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "MSFT",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": 420.00,
                    "regularMarketPreviousClose": 420.00,
                    "regularMarketVolume": 1500,
                    "currency": "USD"
                }
            ],
            "error": null
        }
    }"#;

    // Set up two sequential mocks. The first is limited to a single call so the second one is used next.
    let mut m1 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body1);
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    // diff_only defaults to true; ensure we bypass cache so each poll hits the server
    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(100))
        .cache_mode(CacheMode::Bypass);

    let (handle, mut rx) = builder.start().await.unwrap();

    // First tick (price change from None -> P) should emit
    let first = timeout(Duration::from_secs(3), rx.recv()).await;
    // After first emission, switch the mock to return a higher volume
    m1.delete();
    let _m2 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body2);
    });

    // Second tick: price unchanged, volume increased -> diff_only should not emit.
    let second = timeout(Duration::from_millis(350), rx.recv()).await;

    handle.abort();

    let first = first
        .expect("timed out waiting for first update")
        .expect("stream closed before first update");

    assert_eq!(
        first.volume.as_ref().map(ToString::to_string),
        Some("1000".into())
    );
    assert!(
        second.is_err(),
        "volume-only changes should not emit with diff_only"
    );
}

#[tokio::test]
async fn stream_polling_diff_only_does_not_track_skipped_updates() {
    let server = crate::common::setup_server();

    let missing_currency = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "MSFT",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": 420.00,
                    "regularMarketPreviousClose": 419.00,
                    "regularMarketVolume": 1000
                }
            ],
            "error": null
        }
    }"#;

    let valid_quote = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "MSFT",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": 420.00,
                    "regularMarketPreviousClose": 419.00,
                    "regularMarketVolume": 1500,
                    "currency": "USD"
                }
            ],
            "error": null
        }
    }"#;

    let calls = Arc::new(AtomicUsize::new(0));
    let response_calls = Arc::clone(&calls);
    let _mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.respond_with(move |_req: &httpmock::HttpMockRequest| {
            let body = if response_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                missing_currency
            } else {
                valid_quote
            };

            httpmock::HttpMockResponse::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(body)
                .build()
        });
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(100))
        .cache_mode(CacheMode::Bypass);

    let (handle, mut rx) = builder.start().await.unwrap();
    let update = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for valid polling stream update")
        .expect("stream closed before valid polling stream update");
    handle.abort();

    assert!(
        calls.load(Ordering::Relaxed) >= 2,
        "stream should keep polling after a skipped update"
    );
    assert_eq!(update.instrument.symbol.as_str(), "MSFT");
    assert_eq!(
        update.volume.as_ref().map(ToString::to_string),
        Some("1500".into())
    );
}

#[tokio::test]
async fn stream_polling_does_not_cache_untyped_instrument_fallback() {
    let server = crate::common::setup_server();

    let body1 = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "MSFT",
                    "regularMarketPrice": 420.00,
                    "regularMarketPreviousClose": 420.00,
                    "regularMarketVolume": 1000,
                    "currency": "USD"
                }
            ],
            "error": null
        }
    }"#;

    let body2 = r#"{
        "quoteResponse": {
            "result": [
                {
                    "symbol": "MSFT",
                    "quoteType": "EQUITY",
                    "regularMarketPrice": 421.00,
                    "regularMarketPreviousClose": 420.00,
                    "regularMarketVolume": 1500,
                    "currency": "USD"
                }
            ],
            "error": null
        }
    }"#;

    let mut m1 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body1);
    });

    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["MSFT"])
        .method(StreamMethod::Polling)
        .interval(Duration::from_millis(100))
        .cache_mode(CacheMode::Bypass);

    let (handle, mut rx) = builder.start().await.unwrap();

    let first = timeout(Duration::from_secs(3), rx.recv()).await;
    m1.delete();
    let _m2 = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(body2);
    });
    let second = timeout(Duration::from_secs(3), rx.recv()).await;

    handle.abort();

    let first = first
        .expect("timed out waiting for first update")
        .expect("stream closed before first update");
    let second = second
        .expect("timed out waiting for second update")
        .expect("stream closed before second update");

    assert_eq!(
        first.instrument.kind.to_string(),
        "YAHOO_STREAM_UNTYPED",
        "missing quoteType should use the explicit untyped stream fallback"
    );
    assert!(
        matches!(second.instrument.kind, AssetKind::Equity),
        "later typed quote data should replace the uncached untyped fallback"
    );
}
