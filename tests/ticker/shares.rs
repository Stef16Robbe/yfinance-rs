use chrono::{TimeZone, Utc};
use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{Ticker, YfClient, YfError};

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

#[tokio::test]
async fn offline_shares_uses_recorded_fixture() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock_annual = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param("type", "annualOrdinarySharesNumber")
            .query_param_exists("period1")
            .query_param_exists("period2");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("timeseries_annualOrdinarySharesNumber", sym));
    });

    let mock_quarterly = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param("type", "quarterlyOrdinarySharesNumber")
            .query_param_exists("period1")
            .query_param_exists("period2");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("timeseries_quarterlyOrdinarySharesNumber", sym));
    });

    let client = YfClient::builder()
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        .build()
        .unwrap();

    let t = Ticker::new(&client, sym);

    let annual = t.shares().await.unwrap();
    mock_annual.assert();
    assert!(!annual.is_empty(), "annual shares missing from fixture");
    assert!(annual[0].shares > 0, "shares count should be positive");

    let quarterly = t.quarterly_shares().await.unwrap();
    mock_quarterly.assert();
    assert!(
        !quarterly.is_empty(),
        "quarterly shares missing from fixture"
    );
    assert!(quarterly[0].shares > 0, "shares count should be positive");
}

#[tokio::test]
async fn offline_shares_between_uses_explicit_window() {
    let sym = "MSFT";
    let start = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let start_ts = start.timestamp().to_string();
    let end_ts = end.timestamp().to_string();
    let server = MockServer::start();

    let mock_annual = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param("type", "annualOrdinarySharesNumber")
            .query_param("period1", start_ts.as_str())
            .query_param("period2", end_ts.as_str());
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("timeseries_annualOrdinarySharesNumber", sym));
    });

    let client = YfClient::builder()
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        .build()
        .unwrap();

    let t = Ticker::new(&client, sym);

    let annual = t.shares_between(start, end).await.unwrap();
    mock_annual.assert();
    assert!(!annual.is_empty(), "annual shares missing from fixture");
}

#[tokio::test]
async fn offline_shares_between_rejects_inverted_window() {
    let sym = "MSFT";
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let client = YfClient::default();
    let t = Ticker::new(&client, sym);

    let err = t.shares_between(start, end).await.unwrap_err();

    assert!(matches!(err, YfError::InvalidDates));
}
