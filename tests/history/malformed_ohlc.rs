use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{HistoryBuilder, ProjectionIssue, YfClient, YfError, YfWarning};

#[tokio::test]
async fn history_drops_malformed_ohlc_rows() {
    let server = MockServer::start();

    let body = r#"{
      "chart":{"result":[{"timestamp":[1,2],
        "indicators":{"quote":[{
          "open":[100.0,null],
          "high":[101.0,null],
          "low":[ 99.0,null],
          "close":[100.5,null],
          "volume":[1000,2000]
        }]}}],"error":null}
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let bars = HistoryBuilder::new(&client, "AAPL").fetch().await.unwrap();

    mock.assert();

    assert_eq!(bars.len(), 1, "null OHLC row is dropped");
    assert!((money_to_f64(&bars[0].open) - 100.0).abs() < 1e-9);
    assert!((money_to_f64(&bars[0].close) - 100.5).abs() < 1e-9);
    assert_eq!(bars[0].volume, Some(1000));
}

#[tokio::test]
async fn history_reports_dropped_malformed_ohlc_rows() {
    let server = MockServer::start();

    let body = r#"{
      "chart":{"result":[{"timestamp":[1,2],
        "indicators":{"quote":[{
          "open":[100.0,null],
          "high":[101.0,null],
          "low":[ 99.0,null],
          "close":[100.5,null],
          "volume":[1000,2000]
        }]}}],"error":null}
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "AAPL")
        .fetch_with_diagnostics()
        .await
        .unwrap();

    mock.assert();
    assert_eq!(response.data.len(), 1);
    assert!(!response.is_lossless());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "candle",
            reason: ProjectionIssue::MissingRequiredFields { .. },
            ..
        })
    ));
}

#[tokio::test]
async fn strict_history_errors_on_dropped_malformed_ohlc_rows() {
    let server = MockServer::start();

    let body = r#"{
      "chart":{"result":[{"timestamp":[1,2],
        "indicators":{"quote":[{
          "open":[100.0,null],
          "high":[101.0,null],
          "low":[ 99.0,null],
          "close":[100.5,null],
          "volume":[1000,2000]
        }]}}],"error":null}
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = HistoryBuilder::new(&client, "AAPL")
        .strict()
        .fetch()
        .await
        .unwrap_err();

    mock.assert();
    assert!(matches!(err, YfError::DataQuality(_)));
}

#[tokio::test]
async fn history_drops_unrepresentable_raw_ohlc_before_adjustment() {
    let server = MockServer::start();

    let body = r#"{
      "chart":{"result":[{"timestamp":[1,2],
        "indicators":{
          "quote":[{
            "open":[100.0,1e30],
            "high":[101.0,1e30],
            "low":[ 99.0,1e30],
            "close":[100.5,1e30],
            "volume":[1000,2000]
          }],
          "adjclose":[{"adjclose":[100.5,100.0]}]
        }}],"error":null}
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let bars = HistoryBuilder::new(&client, "AAPL").fetch().await.unwrap();

    mock.assert();

    assert_eq!(
        bars.len(),
        1,
        "raw malformed OHLC row is dropped even if adjusted values would fit"
    );
    assert_eq!(bars[0].volume, Some(1000));
}
