use std::time::Duration;

use httpmock::{Method::GET, MockServer};
use paft::money::{Currency, IsoCurrency};
use url::Url;
use yfinance_rs::{FundamentalsBuilder, ProjectionIssue, Ticker, YfClient, YfError, YfWarning};

#[tokio::test]
async fn quote_summary_http_error_maps_status_and_is_not_cached() {
    let server = MockServer::start();
    let sym = "AAPL";

    let mut error = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earnings")
            .query_param("crumb", "crumb");
        then.status(500)
            .header("content-type", "text/html")
            .body("<html>server error</html>");
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .cache_ttl(Duration::from_secs(61))
        .retry_enabled(false)
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, sym);
    let err = ticker
        .earnings(Some(Currency::Iso(IsoCurrency::USD)))
        .await
        .unwrap_err();

    error.assert();
    match err {
        YfError::ServerError { status, url } => {
            assert_eq!(status, 500);
            assert!(url.contains("/v10/finance/quoteSummary/"));
        }
        other => panic!("expected ServerError, got {other:?}"),
    }

    error.delete();
    let ok = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earnings")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "earnings": {
                        "financialsChart": { "yearly": [], "quarterly": [] },
                        "earningsChart": { "quarterly": [] }
                      }
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let earnings = ticker
        .earnings(Some(Currency::Iso(IsoCurrency::USD)))
        .await
        .unwrap();

    ok.assert();
    assert!(earnings.yearly.is_empty());
    assert!(earnings.quarterly.is_empty());
    assert!(earnings.quarterly_eps.is_empty());
}

#[tokio::test]
async fn fundamentals_timeseries_http_error_maps_status_and_is_not_cached() {
    let server = MockServer::start();
    let sym = "MSFT";

    let mut rate_limited = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param("type", "annualBasicAverageShares")
            .query_param("crumb", "crumb")
            .query_param_exists("period1")
            .query_param_exists("period2");
        then.status(429)
            .header("content-type", "text/html")
            .body("<html>rate limited</html>");
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
        .cache_ttl(Duration::from_secs(61))
        .retry_enabled(false)
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, sym);
    let err = ticker.shares().await.unwrap_err();

    rate_limited.assert();
    match err {
        YfError::RateLimited { url } => {
            assert!(url.contains("/ws/fundamentals-timeseries/"));
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }

    rate_limited.delete();
    let ok = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param("type", "annualBasicAverageShares")
            .query_param("crumb", "crumb")
            .query_param_exists("period1")
            .query_param_exists("period2");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"timeseries":{"result":[]}}"#);
    });

    let shares = ticker.shares().await.unwrap();

    ok.assert();
    assert!(shares.is_empty());
}

#[tokio::test]
async fn malformed_timeseries_values_are_reported_as_projection_loss() {
    let server = MockServer::start();
    let sym = "NOCURRENCY";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{sym}"
            ))
            .query_param("symbol", sym)
            .query_param(
                "type",
                "annualTotalRevenue,annualGrossProfit,annualOperatingIncome,annualNetIncome,annualInterestExpense,annualTaxProvision,annualDepreciationAndAmortization",
            )
            .query_param("crumb", "crumb")
            .query_param_exists("period1")
            .query_param_exists("period2");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "timeseries": {
                    "result": [{
                      "timestamp": [1704067200],
                      "meta": {},
                      "annualTotalRevenue": [{
                        "reportedValue": { "raw": "not-a-number" }
                      }]
                    }]
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = FundamentalsBuilder::new(&client, sym)
        .income_statement_with_diagnostics(false, None)
        .await
        .unwrap();

    mock.assert();
    assert!(response.data.is_empty());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "timeseries_item",
            reason: ProjectionIssue::InvalidField {
                field: "values",
                ..
            },
            ..
        })
    ));
}
