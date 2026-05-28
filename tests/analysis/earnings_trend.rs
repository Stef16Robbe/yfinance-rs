use httpmock::{Method::GET, MockServer};
use paft::money::{Currency, IsoCurrency};
use url::Url;
use yfinance_rs::{ApiPreference, Ticker, YfClient};

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

const EARNINGS_TREND_WITH_USD_REVENUE: &str = r#"{
  "quoteSummary": {
    "result": [{
      "earningsTrend": {
        "trend": [{
          "period": "0q",
          "revenueEstimate": {
            "avg": { "raw": 1000 },
            "revenueCurrency": "USD"
          }
        }]
      }
    }],
    "error": null
  }
}"#;

const TIMESERIES_REVENUE_WITHOUT_CURRENCY: &str = r#"{
  "timeseries": {
    "result": [{
      "meta": { "type": ["annualTotalRevenue"] },
      "timestamp": [1704067200],
      "annualTotalRevenue": [{
        "asOfDate": "2024-01-01",
        "periodType": "12M",
        "reportedValue": { "raw": 42.0 }
      }]
    }],
    "error": null
  }
}"#;

const QUOTE_WITH_GBP_TRADING_CURRENCY: &str = r#"{
  "quoteResponse": {
    "result": [{
      "symbol": "TSCO.L",
      "quoteType": "EQUITY",
      "currency": "GBp"
    }],
    "error": null
  }
}"#;

const REPORTING_WITHOUT_CURRENCY: &str =
    r#"{"quoteSummary":{"result":[{"financialData":{},"earnings":{}}],"error":null}}"#;

const PROFILE_UNITED_KINGDOM: &str = r#"{
  "quoteSummary": {
    "result": [{
      "quoteType": {
        "quoteType": "EQUITY",
        "longName": "Tesco PLC",
        "exchange": "LSE"
      },
      "assetProfile": {
        "country": "United Kingdom"
      }
    }],
    "error": null
  }
}"#;

#[tokio::test]
async fn offline_earnings_trend_uses_recorded_fixture() {
    let sym = "AAPL";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earningsTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("analysis_api_earningsTrend", sym));
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let t = Ticker::new(&client, sym);
    let rows = t.earnings_trend(None).await.unwrap();

    mock.assert();
    assert_eq!(rows.len(), 4, "record with YF_RECORD=1 first");

    // Find any row with earnings estimate data
    let current_year = rows
        .iter()
        .find(|r| r.earnings_estimate.avg.is_some())
        .expect("Should find a row with earnings estimate");
    assert!(current_year.earnings_estimate.avg.is_some());
    assert!(current_year.revenue_estimate.avg.is_some());
    assert!(current_year.eps_trend.current.is_some());
    assert!(!current_year.eps_revisions.historical.is_empty());
}

#[tokio::test]
async fn earnings_trend_revenue_currency_does_not_poison_reporting_currency_cache() {
    let symbol = "TSCO.L";
    let server = MockServer::start();

    let earnings_trend_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "earningsTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(EARNINGS_TREND_WITH_USD_REVENUE);
    });

    let fundamentals_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{symbol}"
            ))
            .query_param_exists("type")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(TIMESERIES_REVENUE_WITHOUT_CURRENCY);
    });

    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", symbol);
        then.status(200)
            .header("content-type", "application/json")
            .body(QUOTE_WITH_GBP_TRADING_CURRENCY);
    });

    let reporting_currency_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "financialData,earnings")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(REPORTING_WITHOUT_CURRENCY);
    });

    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(PROFILE_UNITED_KINGDOM);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, symbol);

    let trend = ticker.earnings_trend(None).await.unwrap();
    let revenue_currency = trend
        .first()
        .and_then(|row| row.revenue_estimate.avg.as_ref())
        .map(|money| money.currency().clone());
    assert_eq!(revenue_currency, Some(Currency::Iso(IsoCurrency::USD)));

    let income = ticker.income_stmt(None).await.unwrap();
    let reporting_currency = income
        .first()
        .and_then(|row| row.total_revenue.as_ref())
        .map(|money| money.currency().clone());
    assert_eq!(reporting_currency, Some(Currency::Iso(IsoCurrency::GBP)));

    earnings_trend_mock.assert();
    fundamentals_mock.assert();
    assert_eq!(quote_mock.calls(), 1);
    reporting_currency_mock.assert();
    profile_mock.assert();
}
