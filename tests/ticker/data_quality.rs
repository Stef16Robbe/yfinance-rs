use httpmock::{
    Method::{GET, POST},
    MockServer,
};
use serde_json::json;
use url::Url;
use yfinance_rs::{DataQuality, Ticker, YfClient, YfError};

#[tokio::test]
async fn ticker_data_quality_strict_rejects_direct_quote_projection_warnings() {
    let server = MockServer::start();

    let body = r#"{
      "quoteResponse": {
        "result": [
          {
            "symbol":"AAPL",
            "quoteType": "EQUITY",
            "regularMarketPrice": 190.25,
            "currency": "USD",
            "fullExchangeName": "!!!"
          }
        ],
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

    let err = Ticker::new(&client, "AAPL")
        .data_quality(DataQuality::Strict)
        .quote()
        .await
        .unwrap_err();
    let diagnostics_err = Ticker::new(&client, "AAPL")
        .strict()
        .quote_with_diagnostics()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, YfError::DataQuality(_)));
    assert!(matches!(diagnostics_err, YfError::DataQuality(_)));
}

#[tokio::test]
async fn ticker_strict_propagates_to_builder_backed_convenience_methods() {
    let server = MockServer::start();
    let sym = "AAPL";
    let expected_payload = json!({
        "serviceConfig": {
            "snippetCount": 10,
            "s": [sym]
        }
    });
    let body = r#"{
      "data": {
        "tickerStream": {
          "stream": [
            {
              "id": "bad-news",
              "content": {
                "title": "Missing publication date"
              }
            }
          ]
        }
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/xhr/ncp")
            .query_param("queryRef", "latestNews")
            .query_param("serviceKey", "ncp_fin")
            .json_body(expected_payload);
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_news(Url::parse(&server.base_url()).unwrap())
        .build()
        .unwrap();

    let err = Ticker::new(&client, sym).strict().news().await.unwrap_err();

    mock.assert();
    assert!(matches!(err, YfError::DataQuality(_)));
}
