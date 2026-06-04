use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::YfClient;
use yfinance_rs::core::conversions::*;

#[tokio::test]
async fn download_repair_simple_100x_fix() {
    // Well-formed JSON: adjclose inside indicators; braces balanced.
    // Middle row is 100x too high -> should be scaled down when repair=true.
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "FIX", "instrumentType": "EQUITY" },
          "timestamp": [1, 2, 3],
          "indicators": {
            "quote": [{
              "open":  [ 10.0, 1000.0, 10.5],
              "high":  [ 11.0, 1100.0, 11.0],
              "low":   [  9.0,  900.0, 10.0],
              "close": [ 10.5, 1050.0, 10.8],
              "volume":[ 100,    100,   100]
            }],
            "adjclose": [{
              "adjclose": [10.5, 1050.0, 10.8]
            }]
          }
        }],
        "error": null
      }
    }"#;

    let server = MockServer::start();
    let sym = "FIX";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{sym}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = yfinance_rs::DownloadBuilder::new(&client)
        .symbols([sym])
        .repair(true)
        .run()
        .await
        .unwrap();

    mock.assert();

    let v = &res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == sym)
        .unwrap()
        .history
        .candles;
    // middle row scaled ~0.01
    assert!((money_to_f64(&v[1].ohlc.close) - 10.5).abs() < 1e-9);
    assert!((money_to_f64(&v[1].ohlc.open) - 10.0).abs() < 1e-9);
    assert!((money_to_f64(&v[1].ohlc.high) - 11.0).abs() < 1e-9);
    assert!((money_to_f64(&v[1].ohlc.low) - 9.0).abs() < 1e-9);
    assert!((money_to_f64(v[1].close_unadj.as_ref().unwrap()) - 10.5).abs() < 1e-9);
}

#[tokio::test]
async fn download_repair_leaves_row_unchanged_if_any_scaled_price_fails() {
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "FIX", "instrumentType": "EQUITY" },
          "timestamp": [1, 2, 3],
          "indicators": {
            "quote": [{
              "open":  [100.0, 7e28, 100.0],
              "high":  [101.0, 7e28, 101.0],
              "low":   [ 99.0,  0.1,  99.0],
              "close": [100.0,  0.1, 100.0],
              "volume":[  100,  100,   100]
            }],
            "adjclose": [{
              "adjclose": [100.0, 0.1, 100.0]
            }]
          }
        }],
        "error": null
      }
    }"#;

    let server = MockServer::start();
    let sym = "FIX";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{sym}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = yfinance_rs::DownloadBuilder::new(&client)
        .symbols([sym])
        .repair(true)
        .run()
        .await
        .unwrap();

    mock.assert();

    let v = &res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == sym)
        .unwrap()
        .history
        .candles;

    assert_eq!(v.len(), 3);
    assert!((money_to_f64(&v[1].ohlc.close) - 0.1).abs() < 1e-9);
    assert!(money_to_f64(&v[1].ohlc.open) > 1e28);
}

#[tokio::test]
async fn download_repair_uses_repaired_previous_row_as_next_baseline() {
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "FIX", "instrumentType": "EQUITY" },
          "timestamp": [1, 2, 3, 4],
          "indicators": {
            "quote": [{
              "open":  [1000.0,   10.0, 1000.0,   10.0],
              "high":  [1001.0,   10.1, 1001.0,   10.1],
              "low":   [ 999.0,    9.9,  999.0,    9.9],
              "close": [1000.0,   10.0, 1000.0,   10.0],
              "volume":[   100,    100,    100,    100]
            }],
            "adjclose": [{
              "adjclose": [1000.0, 10.0, 1000.0, 10.0]
            }]
          }
        }],
        "error": null
      }
    }"#;

    let server = MockServer::start();
    let sym = "FIX";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{sym}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = yfinance_rs::DownloadBuilder::new(&client)
        .symbols([sym])
        .repair(true)
        .run()
        .await
        .unwrap();

    mock.assert();

    let v = &res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == sym)
        .unwrap()
        .history
        .candles;

    assert_eq!(v.len(), 4);
    // Row 1 is repaired from 10 to 1000, then that repaired close is used as
    // row 2's previous-neighbor baseline in the same forward pass.
    assert!((money_to_f64(&v[1].ohlc.close) - 1000.0).abs() < 1e-9);
    assert!((money_to_f64(&v[2].ohlc.close) - 1000.0).abs() < 1e-9);
}
