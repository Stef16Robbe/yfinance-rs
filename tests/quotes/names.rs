use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::{QuotesBuilder, YfClient};

#[tokio::test]
async fn quote_prefers_long_name_for_display_name() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "BHP.MU");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteResponse": {
                    "result": [{
                      "symbol": "BHP.MU",
                      "quoteType": "EQUITY",
                      "shortName": "BHP Group Ltd.                R",
                      "longName": "BHP Group Ltd",
                      "regularMarketPrice": 27.5,
                      "currency": "EUR"
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let quotes = QuotesBuilder::new(&client)
        .symbols(["BHP.MU"])
        .fetch()
        .await
        .unwrap();

    mock.assert();
    assert_eq!(quotes[0].name.as_deref(), Some("BHP Group Ltd"));
}
