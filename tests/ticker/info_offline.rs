use httpmock::{Method::GET, Mock, MockServer};
use std::time::Duration;
use url::Url;
use yfinance_rs::{
    ApiPreference, ProjectionIssue, Ticker, YfClient, YfWarning,
    core::client::{Backoff, CacheMode, RetryConfig},
};

fn mock_quote_summary_fixture<'a>(
    server: &'a MockServer,
    sym: &'a str,
    modules: &'a str,
    endpoint: &'a str,
) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules);
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(endpoint, sym, "json"));
    })
}

fn mock_key_statistics<'a>(
    server: &'a MockServer,
    sym: &'a str,
    crumb: &'a str,
    fixture: String,
) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", crate::common::KEY_STATISTICS_MODULES)
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture);
    })
}

fn mock_info_quote<'a>(server: &'a MockServer, sym: &'a str) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", sym, "json"));
    })
}

#[tokio::test]
async fn offline_info_uses_recorded_fixtures() {
    let server = MockServer::start();
    let sym = "MSFT";
    let crumb = "test-crumb";
    let quote_fixture = crate::common::fixture("quote_v7", sym, "json");
    let key_statistics_fixture =
        crate::common::fixture(crate::common::KEY_STATISTICS_FIXTURE_ENDPOINT, sym, "json");
    let raw_quote_fixture: serde_json::Value = serde_json::from_str(&quote_fixture).unwrap();
    let raw_quote = raw_quote_fixture["quoteResponse"]["result"]
        .as_array()
        .and_then(|quotes| quotes.first())
        .expect("quote fixture should contain MSFT");

    // 1. Mock for quote::fetch_quote -> uses `quote_v7_MSFT.json`
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(quote_fixture);
    });

    let key_statistics_mock =
        mock_key_statistics(&server, sym, crumb, key_statistics_fixture.clone());
    let profile_mock = mock_quote_summary_fixture(
        &server,
        sym,
        "assetProfile,quoteType,fundProfile",
        "profile_api_assetProfile-quoteType-fundProfile",
    );
    let price_target_mock =
        mock_quote_summary_fixture(&server, sym, "financialData", "analysis_api_financialData");
    let rec_summary_mock = mock_quote_summary_fixture(
        &server,
        sym,
        "recommendationTrend,financialData",
        "analysis_api_recommendationTrend-financialData",
    );
    let esg_mock = mock_quote_summary_fixture(&server, sym, "esgScores", "esg_api_esgScores");

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", crumb)
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, sym);
    let info = ticker.info().await.unwrap();

    // Assert all mocks were hit
    assert!(quote_mock.calls() >= 1, "v7 quote should populate info");
    key_statistics_mock.assert();
    assert!(
        profile_mock.calls() >= 1,
        "profile fetch should populate structured info"
    );
    price_target_mock.assert();
    rec_summary_mock.assert();
    esg_mock.assert();

    // Verify data aggregation with more robust checks. Run recorders if these fail.
    assert_eq!(info.snapshot.instrument.symbol.as_str(), "MSFT");
    assert!(
        info.snapshot.last.is_some(),
        "Price missing from quote fixture."
    );
    assert!(info.profile.is_some());
    assert_eq!(
        info.key_statistics.shares_outstanding,
        raw_quote["sharesOutstanding"].as_u64()
    );
    assert_eq!(
        info.key_statistics.beta,
        Some(crate::common::quote_summary_beta(&key_statistics_fixture))
    );
    assert!(
        info.calendar
            .and_then(|calendar| calendar.dividend_payment_date)
            .is_some(),
        "dividend payment date should fall back to v7 quote dividendDate"
    );
}

#[tokio::test]
async fn info_with_diagnostics_keeps_nested_esg_unavailable_warning() {
    let server = MockServer::start();
    let sym = "MSFT";
    let crumb = "crumb";
    let _quote_mock = mock_info_quote(&server, sym);
    let esg_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores")
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteSummary":{"result":[{}],"error":null}}"#);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", crumb)
        .retry_enabled(false)
        .build()
        .unwrap();

    let response = Ticker::new(&client, sym)
        .info_with_diagnostics()
        .await
        .unwrap();

    esg_mock.assert();
    assert!(response.data.esg_scores.is_none());
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::ProviderFeatureUnavailable {
            feature: "esgScores",
            reason: ProjectionIssue::ProviderUnavailable {
                feature: "esgScores"
            },
            ..
        }
    )));
}

#[tokio::test]
async fn ticker_info_profile_respects_ticker_cache_bypass() {
    let server = MockServer::start();
    let sym = "AAPL";
    let crumb = "crumb";
    let _quote_mock = mock_info_quote(&server, sym);
    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ));
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", crumb)
        .cache_ttl(Duration::from_mins(1))
        .retry_enabled(false)
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, sym).cache_mode(CacheMode::Bypass);

    assert!(ticker.info().await.unwrap().profile.is_some());
    assert!(ticker.info().await.unwrap().profile.is_some());
    profile_mock.assert_calls(2);
}

#[tokio::test]
async fn ticker_info_profile_respects_ticker_retry_policy() {
    let server = MockServer::start();
    let sym = "AAPL";
    let crumb = "crumb";
    let _quote_mock = mock_info_quote(&server, sym);
    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", crumb);
        then.status(503).body("Service Unavailable");
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", crumb)
        .retry_enabled(false)
        .build()
        .unwrap();
    let max_retries = 2;
    let ticker_retry = RetryConfig {
        backoff: Backoff::Fixed(Duration::from_millis(1)),
        max_retries,
        ..RetryConfig::default()
    };

    let ticker = Ticker::new(&client, sym).retry_policy(Some(ticker_retry));
    let info = ticker.info().await.unwrap();

    profile_mock.assert_calls((1 + max_retries) as usize);
    assert!(info.profile.is_none());
}
