use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{
    ApiPreference, ProjectionIssue, YfClient, YfError, YfWarning, analysis::AnalysisBuilder,
};

#[tokio::test]
async fn recommendation_trend_missing_period_reports_dropped_row() {
    let sym = "AAPL";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "recommendationTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "recommendationTrend": {
                        "trend": [{
                          "strongBuy": 1,
                          "buy": 2,
                          "hold": 3,
                          "sell": 4,
                          "strongSell": 5
                        }]
                      }
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .recommendations_with_diagnostics()
        .await
        .unwrap();

    assert!(response.data.is_empty());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "recommendation_trend",
            reason: ProjectionIssue::MissingRequiredField { field: "period" },
            ..
        })
    ));

    let err = AnalysisBuilder::new(&client, sym)
        .strict()
        .recommendations()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, YfError::DataQuality(_)));
}

#[tokio::test]
async fn recommendation_counts_report_invalid_present_values() {
    let sym = "AAPL";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "recommendationTrend")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "recommendationTrend": {
                        "trend": [{
                          "period": "0m",
                          "strongBuy": -1,
                          "buy": 2,
                          "hold": 3,
                          "sell": 4,
                          "strongSell": 5
                        }]
                      }
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .recommendations_with_diagnostics()
        .await
        .unwrap();

    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].strong_buy, None);
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::OmittedPresentField {
            path: "recommendationTrend.trend[].strongBuy",
            reason: ProjectionIssue::InvalidField {
                field: "strongBuy",
                ..
            },
            ..
        }
    )));

    let err = AnalysisBuilder::new(&client, sym)
        .strict()
        .recommendations()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, YfError::DataQuality(_)));
}

#[tokio::test]
async fn price_target_reports_present_prices_when_currency_cannot_be_resolved() {
    let sym = "NOCURRENCY";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "financialData")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "financialData": {
                        "targetMeanPrice": { "raw": 10.0 },
                        "targetHighPrice": { "raw": 12.0 },
                        "targetLowPrice": { "raw": 8.0 },
                        "numberOfAnalystOpinions": { "raw": 7 }
                      }
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
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .analyst_price_target_with_diagnostics(None)
        .await
        .unwrap();

    mock.assert();
    assert!(response.data.mean.is_none());
    assert_eq!(response.data.number_of_analysts, Some(7));
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::OmittedPresentField {
            path: "financialData.targetMeanPrice",
            reason: ProjectionIssue::CurrencyUnresolved,
            ..
        }
    )));
}

#[tokio::test]
async fn analyst_count_fractional_rounding_is_diagnostic() {
    let sym = "MSFT";
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "financialData")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "financialData": {
                        "numberOfAnalystOpinions": { "raw": 12.7 }
                      }
                    }],
                    "error": null
                  }
                }"#,
            );
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._api_preference(ApiPreference::ApiOnly)
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = AnalysisBuilder::new(&client, sym)
        .analyst_price_target_with_diagnostics(None)
        .await
        .unwrap();

    assert_eq!(response.data.number_of_analysts, Some(13));
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::CoercedPresentField {
            path: "financialData.numberOfAnalystOpinions",
            ..
        }
    )));

    let err = AnalysisBuilder::new(&client, sym)
        .strict()
        .analyst_price_target(None)
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, YfError::DataQuality(_)));
}
