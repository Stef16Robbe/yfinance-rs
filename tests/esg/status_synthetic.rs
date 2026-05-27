use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{Ticker, YfClient};

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

fn preauthed_client(server: &MockServer) -> YfClient {
    YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap()
}

#[tokio::test]
async fn esg_http_not_found_returns_empty_summary() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores")
            .query_param("crumb", "crumb");
        then.status(404)
            .header("content-type", "application/json")
            .body(fixture("esg_api_esgScores", sym));
    });

    let ticker = Ticker::new(&preauthed_client(&server), sym);
    let summary = ticker.sustainability().await.unwrap();

    mock.assert();
    assert!(summary.scores.is_none());
    assert!(summary.involvement.is_empty());
}

#[tokio::test]
async fn esg_not_found_body_returns_empty_summary() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("esg_api_esgScores", sym));
    });

    let ticker = Ticker::new(&preauthed_client(&server), sym);
    let summary = ticker.sustainability().await.unwrap();

    mock.assert();
    assert!(summary.scores.is_none());
    assert!(summary.involvement.is_empty());
}
