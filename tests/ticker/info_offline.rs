use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{ApiPreference, Ticker, YfClient};

#[tokio::test]
async fn offline_info_uses_recorded_fixtures() {
    let server = MockServer::start();
    let sym = "MSFT";
    let crumb = "test-crumb";
    let quote_fixture = crate::common::fixture("quote_v7", sym, "json");
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

    // 2. Mock for Profile::load -> uses `profile_api_assetProfile-quoteType-fundProfile_MSFT.json`
    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ));
    });

    // 3. Mock for price_target -> uses `analysis_api_financialData_MSFT.json`
    let price_target_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "financialData");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(
                "analysis_api_financialData",
                sym,
                "json",
            ));
    });

    // 4. Mock for recommendations_summary -> uses `analysis_api_recommendationTrend-financialData_MSFT.json`
    let rec_summary_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "recommendationTrend,financialData");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(
                "analysis_api_recommendationTrend-financialData",
                sym,
                "json",
            ));
    });

    // 5. Mock for esg_scores -> uses `esg_api_esgScores_MSFT.json`
    let esg_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "esgScores");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("esg_api_esgScores", sym, "json"));
    });

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
    quote_mock.assert();
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
    assert!(
        info.calendar
            .and_then(|calendar| calendar.dividend_payment_date)
            .is_some(),
        "dividend payment date should fall back to v7 quote dividendDate"
    );
}
