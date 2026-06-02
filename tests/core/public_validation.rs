use std::{
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpListener},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use httpmock::Method::GET;
use url::Url;
use yfinance_rs::{
    QuotesBuilder, StreamBuilder, StreamMethod, YfClient, YfCurrencyInference, YfCurrencyPurpose,
    YfError,
    core::client::{Backoff, RetryConfig},
};

fn invalid_retry_with_factor(factor: f64) -> RetryConfig {
    RetryConfig {
        backoff: Backoff::Exponential {
            base: Duration::from_millis(1),
            factor,
            max: Duration::from_millis(10),
            jitter: false,
        },
        ..RetryConfig::default()
    }
}

fn assert_invalid_params(err: YfError, expected: &str) {
    match err {
        YfError::InvalidParams(message) => assert!(
            message.contains(expected),
            "expected invalid params message to contain {expected:?}; got {message:?}"
        ),
        other => panic!("expected InvalidParams, got {other:?}"),
    }
}

#[test]
fn public_currency_diagnostics_expose_purpose_and_inference() {
    assert_eq!(
        YfCurrencyPurpose::AnalystEstimate.to_string(),
        "analyst-estimate"
    );
    assert_eq!(
        YfCurrencyInference::ProfileCountryHeuristic.to_string(),
        "profile-country heuristic"
    );
}

struct CaptureServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Receiver<Vec<u8>>,
    handle: Option<JoinHandle<()>>,
}

impl CaptureServer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let (tx, requests) = mpsc::channel();
        let handle = thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(200)))
                            .unwrap();
                        let mut buffer = [0; 2048];
                        let bytes_read = stream.read(&mut buffer).unwrap_or(0);
                        let _ = stream
                            .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
                        let _ = tx.send(buffer[..bytes_read].to_vec());
                        return;
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            }
        });

        Self {
            addr,
            stop,
            requests,
            handle: Some(handle),
        }
    }

    fn proxy_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn recv_timeout(&self, timeout: Duration) -> Result<Vec<u8>, mpsc::RecvTimeoutError> {
        self.requests.recv_timeout(timeout)
    }
}

impl Drop for CaptureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn client_builder_rejects_invalid_retry_backoff_factors() {
    for factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
        let err = YfClient::builder()
            .retry_config(invalid_retry_with_factor(factor))
            .build()
            .unwrap_err();

        assert_invalid_params(err, "factor");
    }
}

#[test]
fn client_builder_rejects_excessive_retry_counts() {
    let cfg = RetryConfig {
        max_retries: RetryConfig::MAX_RETRIES + 1,
        ..RetryConfig::default()
    };

    let err = YfClient::builder().retry_config(cfg).build().unwrap_err();

    assert_invalid_params(err, "max_retries");
}

#[tokio::test]
async fn general_proxy_routes_https_requests_through_proxy() {
    let proxy = CaptureServer::start();
    let target = CaptureServer::start();
    let target_authority = target.addr.to_string();
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("https://{target_authority}/v7/finance/quote")).unwrap())
        .try_proxy(&proxy.proxy_url())
        .unwrap()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let result = QuotesBuilder::new(&client).symbols(["AAPL"]).fetch().await;

    assert!(result.is_err());
    let request = proxy
        .recv_timeout(Duration::from_secs(2))
        .expect("HTTPS quote request should be sent to the configured general proxy");
    let request = String::from_utf8(request).expect("proxy CONNECT request should be UTF-8");

    assert!(
        request.starts_with("CONNECT "),
        "expected an HTTPS proxy CONNECT request, got {request:?}"
    );
    assert!(
        request.contains(&target_authority),
        "expected CONNECT request to target {target_authority}, got {request:?}"
    );
    assert!(
        target.recv_timeout(Duration::from_millis(100)).is_err(),
        "HTTPS request bypassed the proxy and connected directly to the target"
    );
}

#[tokio::test]
async fn per_call_retry_override_is_validated_before_request() {
    let client = YfClient::default();

    let err = QuotesBuilder::new(&client)
        .symbols(["AAPL"])
        .retry_policy(Some(invalid_retry_with_factor(f64::NAN)))
        .fetch()
        .await
        .unwrap_err();

    assert_invalid_params(err, "factor");
}

#[tokio::test]
async fn quote_builder_rejects_invalid_symbols_before_request() {
    let client = YfClient::default();

    for symbol in ["", " \t ", ".", "..", "AAPL/MSFT"] {
        let err = QuotesBuilder::new(&client)
            .symbols([symbol])
            .fetch()
            .await
            .unwrap_err();

        assert_invalid_params(err, "symbol");
    }
}

#[tokio::test]
async fn quote_symbols_are_normalized_before_request() {
    let server = crate::common::setup_server();
    let quote = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let quotes = QuotesBuilder::new(&client)
        .symbols([" aapl "])
        .fetch()
        .await
        .unwrap();

    quote.assert();
    assert_eq!(quotes[0].instrument.symbol.as_str(), "AAPL");
}

#[tokio::test]
async fn stream_builder_rejects_zero_interval_before_starting() {
    let client = YfClient::default();
    let builder = StreamBuilder::new(&client)
        .symbols(["AAPL"])
        .method(StreamMethod::Polling)
        .interval(Duration::ZERO);

    let Err(err) = builder.start().await else {
        panic!("zero stream interval should fail before startup");
    };

    assert_invalid_params(err, "interval");
}
