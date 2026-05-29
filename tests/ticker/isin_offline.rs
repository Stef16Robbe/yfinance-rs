use crate::common::fixture;
use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{RetryConfig, Ticker, YfClient, YfError};

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

#[tokio::test]
async fn offline_isin_non_success_statuses_are_errors() {
    for status in [404, 429, 500, 418] {
        let err = lookup_isin_response("AAPL", status, "").await.unwrap_err();
        let matches_expected = match status {
            404 => matches!(err, YfError::NotFound { .. }),
            429 => matches!(err, YfError::RateLimited { .. }),
            500 => matches!(err, YfError::ServerError { status: 500, .. }),
            418 => matches!(err, YfError::Status { status: 418, .. }),
            _ => unreachable!("test statuses are fixed"),
        };

        assert!(
            matches_expected,
            "unexpected error for status {status}: {err:?}"
        );
    }
}

#[tokio::test]
async fn offline_isin_does_not_match_base_symbol_for_suffix_qualified_query() {
    let isin = lookup_isin("VOD.L", r#"[{"symbol":"VOD","isin":"US0378331005"}]"#).await;

    assert_eq!(isin, None);
}

#[tokio::test]
async fn offline_isin_raw_fallback_preserves_exchange_suffix() {
    let isin = lookup_isin(
        "VOD.L",
        r#"mmSuggestDeliver(0, new Array("Name", "Category", "Keywords"), new Array(new Array("Vodafone Group PLC", "Stocks", "VOD|US0378331005|VOD||VOD")), 1, 0);"#,
    )
    .await;

    assert_eq!(isin, None);
}

#[tokio::test]
async fn offline_isin_accepts_exact_suffix_qualified_symbol() {
    let isin = lookup_isin("VOD.L", r#"[{"symbol":"VOD.L","isin":"US0378331005"}]"#).await;

    assert_eq!(isin, Some("US0378331005".to_string()));
}

async fn lookup_isin(sym: &str, body: impl Into<String>) -> Option<String> {
    lookup_isin_response(sym, 200, body).await.unwrap()
}

async fn lookup_isin_response(
    sym: &str,
    status: u16,
    body: impl Into<String>,
) -> Result<Option<String>, YfError> {
    let server = MockServer::start();
    let isin_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/ajax/SearchController_Suggest")
            .query_param("query", sym);
        then.status(status)
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

    let ticker = Ticker::new(&client, sym).retry_policy(Some(RetryConfig {
        enabled: false,
        ..RetryConfig::default()
    }));
    let isin = ticker.isin().await;

    isin_mock.assert();
    isin
}
