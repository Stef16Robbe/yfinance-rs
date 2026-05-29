use crate::common::{mock_quote_v7_multi, setup_server};
use httpmock::Method::GET;
use std::{path::Path, time::Duration};
use url::Url;
use yfinance_rs::{CacheMode, ProjectionIssue, YfError, YfWarning};

#[tokio::test]
async fn offline_multi_quotes_uses_recorded_fixture() {
    // Skip if the recorded fixture isn't present; you must run the live recorder first.
    let fixture = Path::new("tests/fixtures/quote_v7_MULTI.json");
    if !fixture.exists() {
        eprintln!(
            "skipping offline test: missing {}. run the live recorder with YF_RECORD=1 first.",
            fixture.display()
        );
        return;
    }

    let server = setup_server();
    let _mock = mock_quote_v7_multi(&server, "AAPL,MSFT");

    let base = Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(base)
        .build()
        .unwrap();

    let quotes = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .fetch()
        .await
        .unwrap();

    // Sanity against the recorded fixture
    let syms: Vec<_> = quotes
        .iter()
        .map(|q| q.instrument.symbol.as_str())
        .collect();
    assert!(syms.contains(&"AAPL"));
    assert!(syms.contains(&"MSFT"));
}

#[tokio::test]
async fn malformed_quote_node_missing_symbol_is_dropped_from_batch() {
    let server = setup_server();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL,MSFT");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteResponse": {
                    "result": [
                      { "quoteType": "EQUITY", "regularMarketPrice": 190.0 },
                      { "symbol": "MSFT", "quoteType": "EQUITY", "regularMarketPrice": 420.0 }
                    ]
                  }
                }"#,
            );
    });

    let base = Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(base)
        .build()
        .unwrap();

    let response = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .fetch_with_diagnostics()
        .await
        .unwrap();

    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].instrument.symbol.as_str(), "MSFT");
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::DroppedItem {
            endpoint: "quotes",
            item: "quote",
            key: None,
            reason: ProjectionIssue::MissingRequiredField { field: "symbol" },
        }
    )));

    let err = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .strict()
        .fetch()
        .await;

    mock.assert_calls(2);
    assert!(matches!(err, Err(YfError::DataQuality(_))));
}

#[tokio::test]
async fn batch_quotes_with_diagnostics_reports_unresolved_currency_for_present_price() {
    let server = setup_server();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"AAPL","quoteType":"EQUITY","regularMarketPrice":190.0}]}}"#,
            );
    });

    let base = Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(base)
        .build()
        .unwrap();

    let response = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL"])
        .fetch_with_diagnostics()
        .await
        .unwrap();

    assert_eq!(response.data.len(), 1);
    assert!(response.data[0].price.is_none());
    assert!(response.diagnostics.warnings.iter().any(|warning| {
        matches!(
            warning,
            YfWarning::OmittedPresentField {
                endpoint: "quotes",
                path: "regularMarketPrice",
                key: Some(key),
                reason: ProjectionIssue::CurrencyUnresolved,
            } if key == "AAPL"
        )
    }));
}

#[tokio::test]
async fn malformed_quote_node_invalid_symbol_is_dropped_from_batch() {
    let server = setup_server();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "BAD");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"BAD SYMBOL","quoteType":"EQUITY","regularMarketPrice":190.0}]}}"#,
            );
    });

    let base = Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(base)
        .build()
        .unwrap();

    let response = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["BAD"])
        .fetch_with_diagnostics()
        .await
        .unwrap();

    assert!(response.data.is_empty());
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::DroppedItem {
            endpoint: "quotes",
            item: "quote",
            key: Some(key),
            reason: ProjectionIssue::InvalidField {
                field: "symbol",
                ..
            },
        } if key == "BAD SYMBOL"
    )));

    let err = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["BAD"])
        .strict()
        .fetch()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, YfError::DataQuality(_)));
}

#[tokio::test]
async fn default_quote_cache_mode_bypasses_response_cache() {
    let server = setup_server();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let base = Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(base)
        .cache_ttl(Duration::from_mins(1))
        .build()
        .unwrap();

    for _ in 0..2 {
        let quotes = yfinance_rs::QuotesBuilder::new(&client)
            .symbols(["AAPL"])
            .fetch()
            .await
            .unwrap();
        assert_eq!(quotes.len(), 1);
    }

    mock.assert_calls(2);
}

#[tokio::test]
async fn explicit_quote_cache_mode_uses_response_cache() {
    let server = setup_server();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let base = Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(base)
        .cache_ttl(Duration::from_mins(1))
        .build()
        .unwrap();

    for _ in 0..2 {
        let quotes = yfinance_rs::QuotesBuilder::new(&client)
            .symbols(["AAPL"])
            .cache_mode(CacheMode::Use)
            .fetch()
            .await
            .unwrap();
        assert_eq!(quotes.len(), 1);
    }

    mock.assert_calls(1);
}
