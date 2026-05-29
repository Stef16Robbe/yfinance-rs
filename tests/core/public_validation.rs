use std::time::Duration;

use httpmock::Method::GET;
use url::Url;
use yfinance_rs::{
    QuotesBuilder, StreamBuilder, StreamMethod, YfClient, YfError,
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
