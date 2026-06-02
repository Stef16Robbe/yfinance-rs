use httpmock::{Method::GET, MockServer};
use paft::money::{Currency, IsoCurrency};
use url::Url;
use yfinance_rs::{
    ProjectionIssue, Ticker, YfClient, YfError, YfWarning, analysis::AnalysisBuilder,
};

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

const EARNINGS_TREND_HALF_PRESENT_REVISIONS: &str = r#"{
  "quoteSummary": {
    "result": [{
      "earningsTrend": {
        "trend": [{
          "period": "0q",
          "epsRevisions": {
            "upLast7days": { "raw": 2 },
            "downLast30days": { "raw": 1 }
          }
        }]
      }
    }],
    "error": null
  }
}"#;

const EARNINGS_TREND_WITH_YAHOO_REVISION_CASING: &str = r#"{
  "quoteSummary": {
    "result": [{
      "earningsTrend": {
        "trend": [{
          "period": "0q",
          "epsRevisions": {
            "upLast7days": { "raw": 2 },
            "downLast7Days": { "raw": 1 },
            "upLast30days": { "raw": 4 },
            "downLast30Days": { "raw": 3 }
          }
        }]
      }
    }],
    "error": null
  }
}"#;

#[tokio::test]
async fn earnings_trend_uses_quote_enrichment_without_currency_diagnostic() {
    let sym = "AAPL";
    let server = MockServer::start();

    let trend_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earningsTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "earningsTrend": {
                        "trend": [{
                          "period": "0q",
                          "earningsEstimate": {
                            "avg": { "raw": 1.25 }
                          }
                        }]
                      }
                    }],
                    "error": null
                  }
                }"#,
            );
    });
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteResponse": {
                    "result": [{
                      "symbol": "AAPL",
                      "quoteType": "EQUITY",
                      "financialCurrency": "USD"
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .earnings_trend_with_diagnostics(None)
        .await
        .unwrap();

    trend_mock.assert();
    quote_mock.assert();
    assert!(response.data[0].earnings_estimate.avg.is_some());
    assert!(response.diagnostics.is_empty());

    let strict_rows = AnalysisBuilder::new(&client, sym)
        .strict()
        .earnings_trend(None)
        .await
        .unwrap();

    assert!(strict_rows[0].earnings_estimate.avg.is_some());
    trend_mock.assert_calls(2);
    quote_mock.assert_calls(1);
}

#[tokio::test]
async fn earnings_trend_validates_period_before_currency_enrichment() {
    let sym = "BADPERIOD";
    let server = MockServer::start();

    let trend_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earningsTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "earningsTrend": {
                        "trend": [{
                          "revenueEstimate": {
                            "avg": { "raw": 1000 }
                          }
                        }, {
                          "period": "0q",
                          "revenueEstimate": {
                            "avg": { "raw": 2000 },
                            "revenueCurrency": "USD"
                          }
                        }]
                      }
                    }],
                    "error": null
                  }
                }"#,
            );
    });
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteResponse": {
                    "result": [{
                      "symbol": "BADPERIOD",
                      "quoteType": "EQUITY",
                      "financialCurrency": "USD"
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .earnings_trend_with_diagnostics(None)
        .await
        .unwrap();

    assert_eq!(response.data.len(), 1);
    assert_eq!(
        response.data[0]
            .revenue_estimate
            .avg
            .as_ref()
            .map(|money| money.currency().clone()),
        Some(Currency::Iso(IsoCurrency::USD))
    );
    assert_eq!(response.diagnostics.warnings.len(), 1);
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "earnings_trend",
            reason: ProjectionIssue::MissingRequiredField { field: "period" },
            ..
        })
    ));

    let err = AnalysisBuilder::new(&client, sym)
        .strict()
        .earnings_trend(None)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        YfError::DataQuality(warning) if matches!(
            warning.as_ref(),
            YfWarning::DroppedItem {
                item: "earnings_trend",
                reason: ProjectionIssue::MissingRequiredField { field: "period" },
                ..
            }
        )
    ));

    trend_mock.assert_calls(2);
    quote_mock.assert_calls(0);
}

#[tokio::test]
async fn earnings_trend_reports_half_present_eps_revision_fields() {
    let sym = "HALFREV";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earningsTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(EARNINGS_TREND_HALF_PRESENT_REVISIONS);
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .earnings_trend_with_diagnostics(None)
        .await
        .unwrap();

    assert_eq!(response.data.len(), 1);
    assert!(response.data[0].eps_revisions.historical.is_empty());
    for (path, missing_field) in [
        ("earningsTrend[].epsRevisions.upLast7days", "downLast7days"),
        (
            "earningsTrend[].epsRevisions.downLast30days",
            "upLast30days",
        ),
    ] {
        assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
            warning,
            YfWarning::OmittedPresentField {
                path: warning_path,
                key: Some(key),
                reason: ProjectionIssue::MissingRequiredField { field },
                ..
            } if *warning_path == path && key == "0q" && *field == missing_field
        )));
    }

    let err = AnalysisBuilder::new(&client, sym)
        .strict()
        .earnings_trend(None)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        YfError::DataQuality(warning) if matches!(
            warning.as_ref(),
            YfWarning::OmittedPresentField {
                path: "earningsTrend[].epsRevisions.upLast7days",
                key: Some(key),
                reason: ProjectionIssue::MissingRequiredField {
                    field: "downLast7days"
                },
                ..
            } if key == "0q"
        )
    ));

    mock.assert_calls(2);
}

#[tokio::test]
async fn earnings_trend_accepts_yahoo_eps_revision_down_days_casing() {
    let sym = "REVCASING";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "earningsTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(EARNINGS_TREND_WITH_YAHOO_REVISION_CASING);
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .earnings_trend_with_diagnostics(None)
        .await
        .unwrap();

    mock.assert();
    assert!(response.diagnostics.is_empty());
    assert_eq!(response.data.len(), 1);
    let revisions = &response.data[0].eps_revisions;
    assert_eq!(revisions.historical.len(), 2);
    let seven_days = revisions
        .find_by_period_str("7d")
        .unwrap()
        .expect("7d revision point should map");
    assert_eq!(seven_days.up_count, 2);
    assert_eq!(seven_days.down_count, 1);
    let thirty_days = revisions
        .find_by_period_str("30d")
        .unwrap()
        .expect("30d revision point should map");
    assert_eq!(thirty_days.up_count, 4);
    assert_eq!(thirty_days.down_count, 3);
}

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
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .earnings_trend_with_diagnostics(None)
        .await
        .unwrap();

    mock.assert();
    assert!(response.diagnostics.is_empty());
    let rows = response.data;
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
