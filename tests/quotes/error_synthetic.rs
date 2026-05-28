use crate::common::setup_server;
use httpmock::Method::GET;
use url::Url;
use yfinance_rs::{QuotesBuilder, YfClient, YfError};

#[tokio::test]
async fn quote_v7_yahoo_error_with_null_result_returns_api_error() {
    let server = setup_server();
    let api_err = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteResponse":{"result":null,"error":{"description":"No data found"}}}"#);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = QuotesBuilder::new(&client)
        .symbols(["AAPL"])
        .fetch()
        .await
        .unwrap_err();

    api_err.assert();

    match err {
        YfError::Api(message) => assert!(
            message.contains("yahoo error:") && message.contains("No data found"),
            "expected Yahoo API error to be surfaced; got {message}"
        ),
        other => panic!("expected Api error, got {other:?}"),
    }
}
