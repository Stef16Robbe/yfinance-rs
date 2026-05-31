use httpmock::Method::GET;
use httpmock::{Mock, MockServer};
use url::Url;

use crate::common;
use yfinance_rs::core::conversions::*;
use yfinance_rs::core::{Interval, Range};
use yfinance_rs::{
    DownloadBuilder, DownloadConcurrency, ProjectionIssue, YfClient, YfError, YfWarning,
};

fn has_more_than_two_decimals(x: f64) -> bool {
    if !x.is_finite() {
        return false;
    }
    let cents = (x * 100.0).round();
    (x - cents / 100.0).abs() > 1e-12
}

async fn wait_for_mock_calls(
    mock: &Mock<'_>,
    expected: usize,
    timeout: std::time::Duration,
) -> bool {
    let started = tokio::time::Instant::now();
    while started.elapsed() < timeout {
        if mock.calls_async().await >= expected {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    mock.calls_async().await >= expected
}

const fn chart_body_without_instrument_type() -> &'static str {
    r#"{
      "chart": {
        "result": [{
          "meta": {
            "currency": "USD",
            "symbol": "NOKIND",
            "timezone": "America/New_York",
            "gmtoffset": -14400
          },
          "timestamp": [1710000000],
          "indicators": {
            "quote": [{
              "open": [100.0],
              "high": [101.0],
              "low": [99.0],
              "close": [100.5],
              "volume": [1000]
            }],
            "adjclose": [{
              "adjclose": [100.5]
            }]
          }
        }],
        "error": null
      }
    }"#
}

#[tokio::test]
async fn download_multi_symbols_happy_path() {
    let server = common::setup_server();

    let m_aapl = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let m_msft = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/MSFT")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "MSFT", "json"));
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = DownloadBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .range(Range::M6)
        .interval(Interval::D1)
        .auto_adjust(true)
        .prepost(false)
        .actions(true)
        .run()
        .await
        .unwrap();

    m_aapl.assert();
    m_msft.assert();

    let keys: Vec<_> = res
        .entries
        .iter()
        .map(|e| e.instrument.symbol.as_str().to_string())
        .collect();
    assert!(keys.iter().any(|s| s == "AAPL"));
    assert!(keys.iter().any(|s| s == "MSFT"));
}

#[tokio::test]
async fn best_effort_download_drops_entry_without_instrument_metadata() {
    let server = common::setup_server();

    let m_aapl = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let m_missing = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/NOKIND")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(chart_body_without_instrument_type());
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = DownloadBuilder::new(&client)
        .symbols(["AAPL", "NOKIND"])
        .run_with_diagnostics()
        .await
        .unwrap();

    m_aapl.assert();
    m_missing.assert();

    assert_eq!(response.data.entries.len(), 1);
    assert_eq!(response.data.entries[0].instrument.symbol.as_str(), "AAPL");
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::DroppedItem {
            endpoint: "download",
            item: "download_entry",
            key: Some(key),
            reason: ProjectionIssue::MissingRequiredField {
                field: "chart.meta.instrumentType",
            },
        } if key == "NOKIND"
    )));
}

#[tokio::test]
async fn strict_download_errors_without_instrument_metadata() {
    let server = common::setup_server();

    let m_missing = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/NOKIND")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(chart_body_without_instrument_type());
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = DownloadBuilder::new(&client)
        .symbols(["NOKIND"])
        .strict()
        .run()
        .await
        .unwrap_err();

    m_missing.assert();

    let YfError::DataQuality(warning) = err else {
        panic!("expected data-quality error");
    };
    assert!(matches!(
        &*warning,
        YfWarning::DroppedItem {
            endpoint: "download",
            item: "download_entry",
            key: Some(key),
            reason: ProjectionIssue::MissingRequiredField {
                field: "chart.meta.instrumentType",
            },
        } if key == "NOKIND"
    ));
}

#[tokio::test]
async fn download_respects_configured_concurrency_limit() {
    let server = common::setup_server();

    let m_aapl = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .delay(std::time::Duration::from_secs(1))
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let m_msft = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/MSFT")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "MSFT", "json"));
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let download = DownloadBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .concurrency(DownloadConcurrency::new(1).unwrap());
    let handle = tokio::spawn(async move { download.run().await });

    assert!(
        wait_for_mock_calls(&m_aapl, 1, std::time::Duration::from_millis(750)).await,
        "first symbol was not requested"
    );
    assert_eq!(
        m_msft.calls_async().await,
        0,
        "second symbol started before the configured concurrency slot was released"
    );

    let res = handle.await.unwrap().unwrap();

    m_aapl.assert();
    m_msft.assert();

    let symbols: Vec<_> = res
        .entries
        .iter()
        .map(|entry| entry.instrument.symbol.as_str())
        .collect();
    assert_eq!(symbols, vec!["AAPL", "MSFT"]);
}

#[test]
fn download_concurrency_rejects_zero() {
    match DownloadConcurrency::new(0) {
        Err(YfError::InvalidParams(message)) => {
            assert!(message.contains("concurrency"));
        }
        Err(other) => panic!("expected invalid params error, got {other:?}"),
        Ok(_) => panic!("expected zero concurrency to be rejected"),
    }
}

#[tokio::test]
async fn download_rejects_invalid_symbol_before_request() {
    let symbol = "A".repeat(65);
    let client = YfClient::default();

    let result = DownloadBuilder::new(&client).symbols([symbol]).run().await;

    match result {
        Err(YfError::InvalidParams(message)) => {
            assert!(message.contains("invalid symbol"));
        }
        Err(other) => panic!("expected invalid symbol error, got {other:?}"),
        Ok(_) => panic!("expected invalid symbol error"),
    }
}

#[tokio::test]
async fn download_between_params_applied_to_all_symbols() {
    use chrono::{TimeZone, Utc};
    let server = httpmock::MockServer::start();

    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();

    let q1 = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("period1", start.timestamp().to_string())
            .query_param("period2", end.timestamp().to_string())
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let q2 = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/MSFT")
            .query_param("period1", start.timestamp().to_string())
            .query_param("period2", end.timestamp().to_string())
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "MSFT", "json"));
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = DownloadBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .between(start, end)
        .interval(Interval::D1)
        .auto_adjust(true)
        .prepost(false)
        .actions(true)
        .run()
        .await
        .unwrap();

    q1.assert();
    q2.assert();

    assert_eq!(res.entries.len(), 2);
    let aapl = res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == "AAPL")
        .unwrap();
    let msft = res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == "MSFT")
        .unwrap();
    assert!(!aapl.history.candles.is_empty());
    assert!(!msft.history.candles.is_empty());
}

/* ---------- Parity knob checks using cached live fixtures ---------- */

#[tokio::test]
async fn download_back_adjust_offline() {
    // Run adjusted and back-adjusted on different mock servers so each mock sees 1 hit.
    let server1 = common::setup_server();
    let server2 = common::setup_server();

    let m1_aapl = server1.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let m2_aapl = server2.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let client1 = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server1.base_url())).unwrap())
        .build()
        .unwrap();

    let client2 = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server2.base_url())).unwrap())
        .build()
        .unwrap();

    let adj = DownloadBuilder::new(&client1)
        .symbols(["AAPL"])
        .auto_adjust(true)
        .back_adjust(false)
        .run()
        .await
        .unwrap();

    let back = DownloadBuilder::new(&client2)
        .symbols(["AAPL"])
        .auto_adjust(false) // back_adjust uses an internal adjusted fetch for OHL
        .back_adjust(true)
        .run()
        .await
        .unwrap();

    m1_aapl.assert(); // exactly 1
    m2_aapl.assert(); // exactly 1

    let a = &adj
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == "AAPL")
        .unwrap()
        .history
        .candles;
    let b = &back
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == "AAPL")
        .unwrap()
        .history
        .candles;

    assert_eq!(a.len(), b.len(), "same number of bars");
    for (ca, cb) in a.iter().zip(b.iter()) {
        assert!((money_to_f64(&ca.open) - money_to_f64(&cb.open)).abs() < 1e-9);
        assert!((money_to_f64(&ca.high) - money_to_f64(&cb.high)).abs() < 1e-9);
        assert!((money_to_f64(&ca.low) - money_to_f64(&cb.low)).abs() < 1e-9);
        // close may differ due to back_adjust
    }
    assert!(!a.is_empty(), "expected some data");
}

#[tokio::test]
async fn download_repair_is_noop_on_clean_data_offline() {
    // Run base and repair=true on different mock servers so each mock sees 1 hit.
    let server1 = common::setup_server();
    let server2 = common::setup_server();

    let m1_aapl = server1.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let m2_aapl = server2.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param("range", "6mo")
            .query_param("interval", "1d")
            .query_param("includePrePost", "false")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture("history_chart", "AAPL", "json"));
    });

    let client1 = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server1.base_url())).unwrap())
        .build()
        .unwrap();

    let client2 = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server2.base_url())).unwrap())
        .build()
        .unwrap();

    let base_run = DownloadBuilder::new(&client1)
        .symbols(["AAPL"])
        .run()
        .await
        .unwrap();

    let repair_run = DownloadBuilder::new(&client2)
        .symbols(["AAPL"])
        .repair(true)
        .run()
        .await
        .unwrap();

    m1_aapl.assert(); // exactly 1
    m2_aapl.assert(); // exactly 1

    let a = &base_run
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == "AAPL")
        .unwrap()
        .history
        .candles;
    let b = &repair_run
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == "AAPL")
        .unwrap()
        .history
        .candles;

    assert_eq!(a.len(), b.len());
    for (ca, cb) in a.iter().zip(b.iter()) {
        assert!((money_to_f64(&ca.open) - money_to_f64(&cb.open)).abs() < 1e-12);
        assert!((money_to_f64(&ca.high) - money_to_f64(&cb.high)).abs() < 1e-12);
        assert!((money_to_f64(&ca.low) - money_to_f64(&cb.low)).abs() < 1e-12);
        assert!((money_to_f64(&ca.close) - money_to_f64(&cb.close)).abs() < 1e-12);
    }
}

#[tokio::test]
async fn rounding_two_decimals() {
    use yfinance_rs::core::conversions::money_to_f64;

    let server = MockServer::start();

    let m_aapl = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/AAPL")
            .query_param_exists("interval")
            .query_param_exists("includePrePost")
            .query_param_exists("range");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("history_chart", "AAPL", "json"));
    });

    let m_msft = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/MSFT")
            .query_param_exists("interval")
            .query_param_exists("includePrePost")
            .query_param_exists("range");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("history_chart", "MSFT", "json"));
    });

    let client = yfinance_rs::YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = yfinance_rs::DownloadBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .rounding(true)
        .run()
        .await
        .unwrap();

    m_aapl.assert();
    m_msft.assert();

    for entry in &res.entries {
        for c in &entry.history.candles {
            assert!(!has_more_than_two_decimals(money_to_f64(&c.open)));
            assert!(!has_more_than_two_decimals(money_to_f64(&c.high)));
            assert!(!has_more_than_two_decimals(money_to_f64(&c.low)));
            assert!(!has_more_than_two_decimals(money_to_f64(&c.close)));
        }
    }
}
