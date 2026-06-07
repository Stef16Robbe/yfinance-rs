use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::YfClient;
use yfinance_rs::core::conversions::money_to_f64;

#[tokio::test]
async fn download_preserves_suspicious_price_outliers() {
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
        .run()
        .await
        .unwrap();

    mock.assert();

    let candles = &res
        .entries
        .iter()
        .find(|entry| entry.instrument.symbol.as_str() == sym)
        .unwrap()
        .history
        .candles;

    assert_eq!(candles.len(), 3);
    assert!((money_to_f64(&candles[1].ohlc.open) - 1000.0).abs() < 1e-9);
    assert!((money_to_f64(&candles[1].ohlc.high) - 1100.0).abs() < 1e-9);
    assert!((money_to_f64(&candles[1].ohlc.low) - 900.0).abs() < 1e-9);
    assert!((money_to_f64(&candles[1].ohlc.close) - 1050.0).abs() < 1e-9);
    assert!((money_to_f64(candles[1].close_unadj.as_ref().unwrap()) - 1050.0).abs() < 1e-9);
}
