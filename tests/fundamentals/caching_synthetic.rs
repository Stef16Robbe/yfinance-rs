use std::time::Duration;

use httpmock::{Method::GET, MockServer};
use paft::money::{Currency, IsoCurrency};
use tokio::time::sleep;
use url::Url;
use yfinance_rs::{FundamentalsBuilder, Ticker, YfClient};

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

#[tokio::test]
async fn default_timeseries_windows_hit_fundamentals_cache_across_seconds() {
    let server = MockServer::start();
    let sym = "MSFT";

    let statement_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param_includes("type", "annualTotalAssets")
            .query_param("crumb", "crumb")
            .query_param_exists("period1")
            .query_param_exists("period2");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("timeseries_balance_sheet_annual", sym));
    });

    let shares_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param("type", "annualOrdinarySharesNumber")
            .query_param("crumb", "crumb")
            .query_param_exists("period1")
            .query_param_exists("period2");
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
        ._preauth("cookie", "crumb")
        .cache_ttl(Duration::from_mins(1))
        .retry_enabled(false)
        .build()
        .unwrap();

    let fundamentals = FundamentalsBuilder::new(&client, sym);
    let ticker = Ticker::new(&client, sym);

    let first_statement = fundamentals
        .balance_sheet(false, Some(Currency::Iso(IsoCurrency::USD)))
        .await
        .unwrap();
    let first_shares = ticker.shares().await.unwrap();

    statement_mock.assert_calls(1);
    shares_mock.assert_calls(1);

    sleep(Duration::from_millis(1_100)).await;

    let second_statement = fundamentals
        .balance_sheet(false, Some(Currency::Iso(IsoCurrency::USD)))
        .await
        .unwrap();
    let second_shares = ticker.shares().await.unwrap();

    statement_mock.assert_calls(1);
    shares_mock.assert_calls(1);
    assert_eq!(first_statement.len(), second_statement.len());
    assert_eq!(first_shares.len(), second_shares.len());
}
