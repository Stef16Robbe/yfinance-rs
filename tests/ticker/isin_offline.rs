use crate::common::fixture;
use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{Ticker, YfClient};

#[tokio::test]
async fn offline_isin_happy_path() {
    let sym = "AAPL";
    let isin = lookup_isin(sym, fixture("isin_search", sym, "json")).await;

    assert_eq!(
        isin,
        Some("US0378331005".to_string()),
        "ISIN not parsed from fixture. Did you run `just test-record ticker` first?"
    );
}

#[tokio::test]
async fn offline_isin_rejects_invalid_check_digit() {
    let isin = lookup_isin(
        "AAPL",
        r#"mmSuggestDeliver(0, new Array("Name", "Category", "Keywords"), new Array(new Array("Apple Inc.", "Stocks", "AAPL|US0378331006|AAPL||AAPL")), 1, 0);"#,
    )
    .await;

    assert_eq!(isin, None);
}

#[tokio::test]
async fn offline_isin_raw_fallback_requires_symbol_match() {
    let isin = lookup_isin(
        "AAPL",
        r#"mmSuggestDeliver(0, new Array("Name", "Category", "Keywords"), new Array(new Array("Microsoft Corp.", "Stocks", "MSFT|US5949181045|MSFT||MSFT")), 1, 0);"#,
    )
    .await;

    assert_eq!(isin, None);
}

#[tokio::test]
async fn offline_isin_json_array_requires_symbol_match() {
    let isin = lookup_isin("AAPL", r#"[{"symbol":"MSFT","isin":"US5949181045"}]"#).await;

    assert_eq!(isin, None);
}

async fn lookup_isin(sym: &str, body: impl Into<String>) -> Option<String> {
    let server = MockServer::start();
    let isin_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/ajax/SearchController_Suggest")
            .query_param("query", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(body.into());
    });

    let client = YfClient::builder()
        .base_insider_search(
            Url::parse(&format!(
                "{}/ajax/SearchController_Suggest",
                server.base_url()
            ))
            .unwrap(),
        )
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, sym);
    let isin = ticker.isin().await.unwrap();

    isin_mock.assert();
    isin
}
