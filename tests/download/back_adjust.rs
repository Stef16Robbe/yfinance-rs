use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::conversions::*;
use yfinance_rs::{OhlcPriceBasis, PriceBasis, YfClient, YfError};

#[tokio::test]
async fn download_back_adjust_sets_close_to_raw() {
    // One day has adjclose=50 while raw close=100 (e.g., dividend/split adjustment)
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "TEST", "instrumentType": "EQUITY" },
          "timestamp":[1000,2000],
          "indicators":{
            "quote":[{ "open":[100.0,100.0], "high":[105.0,105.0], "low":[95.0,95.0], "close":[100.0,100.0], "volume":[1000,1000] }],
            "adjclose":[{ "adjclose":[50.0,100.0] }]
          }
        }],
        "error": null
      }
    }"#;

    let server = MockServer::start();
    let sym = "TEST";

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
        .auto_adjust(false)
        .back_adjust(true)
        .run()
        .await
        .unwrap();

    mock.assert();

    let history = &res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == sym)
        .expect("symbol data")
        .history;
    let adjusted = PriceBasis::provider_latest_adjusted();
    assert_eq!(
        history.price_basis,
        OhlcPriceBasis::per_field(adjusted, adjusted, adjusted, PriceBasis::raw())
    );

    let s = &history.candles;
    // first bar got 50% adjustment factor; OHLC adjusted => open≈50, high≈52.5, low≈47.5
    assert!((money_to_f64(&s[0].ohlc.open) - 50.0).abs() < 1e-9);
    // back_adjust keeps raw Close
    assert!((money_to_f64(&s[0].ohlc.close) - 100.0).abs() < 1e-9);
    // second bar unchanged
    assert!((money_to_f64(&s[1].ohlc.open) - 100.0).abs() < 1e-9);
    assert!((money_to_f64(&s[1].ohlc.close) - 100.0).abs() < 1e-9);
}

#[tokio::test]
async fn download_rejects_auto_adjust_with_back_adjust() {
    let client = YfClient::default();

    let result = yfinance_rs::DownloadBuilder::new(&client)
        .symbols(["TEST"])
        .auto_adjust(true)
        .back_adjust(true)
        .run()
        .await;

    match result {
        Err(YfError::InvalidParams(msg)) => {
            assert!(msg.contains("auto_adjust"));
            assert!(msg.contains("back_adjust"));
        }
        Err(err) => panic!("expected InvalidParams, got {err}"),
        Ok(_) => panic!("expected InvalidParams"),
    }
}
