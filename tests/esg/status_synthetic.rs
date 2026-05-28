use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{EsgBuilder, ProjectionIssue, Ticker, YfClient, YfError, YfWarning};

fn preauthed_client(server: &MockServer) -> YfClient {
    YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap()
}

const NOT_FOUND_BODY: &str = r#"{
  "quoteSummary": {
    "error": {
      "code": "Not Found",
      "description": "No fundamentals data found for symbol: MSFT"
    },
    "result": null
  }
}"#;

#[tokio::test]
async fn esg_http_not_found_returns_error() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores")
            .query_param("crumb", "crumb");
        then.status(404)
            .header("content-type", "application/json")
            .body(NOT_FOUND_BODY);
    });

    let ticker = Ticker::new(&preauthed_client(&server), sym);
    let err = ticker.sustainability().await.unwrap_err();

    mock.assert();
    assert!(err.to_string().contains("Not found"));
}

#[tokio::test]
async fn esg_not_found_body_returns_error() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(NOT_FOUND_BODY);
    });

    let ticker = Ticker::new(&preauthed_client(&server), sym);
    let err = ticker.sustainability().await.unwrap_err();

    mock.assert();
    assert!(err.to_string().contains("No fundamentals data found"));
}

#[tokio::test]
async fn missing_esg_module_is_reported_as_unavailable() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{}],
                    "error": null
                  }
                }"#,
            );
    });

    let client = preauthed_client(&server);
    let response = EsgBuilder::new(&client, sym)
        .fetch_with_diagnostics()
        .await
        .unwrap();

    mock.assert();
    assert!(response.data.scores.is_none());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::ProviderFeatureUnavailable {
            feature: "esgScores",
            reason: ProjectionIssue::ProviderUnavailable {
                feature: "esgScores"
            },
            ..
        })
    ));

    let err = EsgBuilder::new(&client, sym)
        .strict()
        .fetch()
        .await
        .unwrap_err();
    assert!(matches!(err, YfError::DataQuality(_)));
}
