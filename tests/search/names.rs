use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::{SearchBuilder, YfClient};

#[tokio::test]
async fn search_prefers_long_name_for_display_name() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/finance/search")
            .query_param("q", "milk")
            .query_param("quotesCount", "10");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quotes": [{
                    "symbol": "A2M.AX",
                    "shortname": "A2 MILK FPO NZ [A2M]",
                    "longname": "The a2 Milk Company Limited",
                    "quoteType": "EQUITY",
                    "exchange": "ASX"
                  }]
                }"#,
            );
    });

    let client = YfClient::builder().build().unwrap();
    let response = SearchBuilder::new(&client, "milk")
        .search_base(Url::parse(&format!("{}/v1/finance/search", server.base_url())).unwrap())
        .fetch()
        .await
        .unwrap();

    mock.assert();
    assert_eq!(
        response.results[0].name.as_deref(),
        Some("The a2 Milk Company Limited")
    );
}
