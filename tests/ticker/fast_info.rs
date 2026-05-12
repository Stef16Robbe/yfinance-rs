use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{Ticker, YfClient};

#[tokio::test]
async fn fast_info_uses_previous_close_when_price_missing() {
    let server = MockServer::start();

    let body = r#"{
      "quoteResponse": {
        "result": [{
          "symbol": "AAPL",
          "regularMarketPrice": null,
          "regularMarketPreviousClose": 199.5,
          "currency": "USD",
          "fullExchangeName": "NasdaqGS"
        }],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();
    let t = Ticker::new(&client, "AAPL");

    let fi = t.fast_info().await.unwrap();
    mock.assert();

    assert_eq!(fi.instrument.symbol.as_str(), "AAPL");
    assert!(
        (yfinance_rs::core::conversions::money_to_f64(&fi.previous_close.unwrap()) - 199.5).abs()
            < 1e-9
    );
    assert_eq!(
        fi.exchange.map(|e| e.to_string()).as_deref(),
        Some("NASDAQ")
    );
}

#[tokio::test]
async fn fast_info_maps_snapshot_session_fields_from_v7_quote() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
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
    let ticker = Ticker::new(&client, "AAPL");

    let snapshot = ticker.fast_info().await.unwrap();
    mock.assert();

    let open = money_to_f64(snapshot.open.as_ref().unwrap());
    assert!(
        (open - 269.28).abs() < 1e-9,
        "expected open 269.28 after USD money rounding, got {open}"
    );
    assert!((money_to_f64(snapshot.day_high.as_ref().unwrap()) - 271.41).abs() < 1e-4);
    assert!((money_to_f64(snapshot.day_low.as_ref().unwrap()) - 267.11).abs() < 1e-4);
    assert_eq!(snapshot.volume, Some(43_332_154));

    #[cfg(feature = "dataframe")]
    {
        use paft::prelude::ToDataFrame;

        let df = snapshot.to_dataframe().unwrap();
        assert_eq!(df.height(), 1);
    }
}
