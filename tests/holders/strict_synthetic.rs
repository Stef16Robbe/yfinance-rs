use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{
    ApiPreference, HoldersBuilder, ProjectionIssue, Ticker, YfClient, YfError, YfWarning,
};

#[tokio::test]
async fn insider_roster_missing_position_is_not_defaulted_to_officer() {
    let sym = "AAPL";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "insiderHolders": {
                        "holders": [{
                          "name": "MISSING POSITION",
                          "transactionDescription": "Sale",
                          "latestTransDate": { "raw": 1704067200 },
                          "positionDirectDate": { "raw": 1704067200 }
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
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let rows = Ticker::new(&client, sym)
        .insider_roster_holders()
        .await
        .unwrap();

    mock.assert();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn optional_holder_value_is_omitted_when_currency_cannot_be_resolved() {
    let sym = "NOCURRENCY";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let holders_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "institutionOwnership": {
                        "ownershipList": [{
                          "organization": "No Currency Capital",
                          "position": { "raw": 10 },
                          "reportDate": { "raw": 1704067200 },
                          "value": { "raw": 12345 }
                        }]
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

    let rows = Ticker::new(&client, sym)
        .institutional_holders()
        .await
        .unwrap();

    holders_mock.assert();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].holder, "No Currency Capital");
    assert_eq!(rows[0].shares, Some(10));
    assert!(rows[0].value.is_none());
}

#[tokio::test]
async fn holder_diagnostics_report_present_value_with_unresolved_currency() {
    let sym = "NOCURRENCY";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let holders_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "institutionOwnership": {
                        "ownershipList": [{
                          "organization": "No Currency Capital",
                          "position": { "raw": 10 },
                          "reportDate": { "raw": 1704067200 },
                          "value": { "raw": 12345 }
                        }]
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

    let response = HoldersBuilder::new(&client, sym)
        .institutional_holders_with_diagnostics()
        .await
        .unwrap();

    holders_mock.assert();
    assert_eq!(response.data.len(), 1);
    assert!(response.data[0].value.is_none());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::OmittedPresentField {
            path: "ownershipList[].value",
            reason: ProjectionIssue::CurrencyUnresolved,
            ..
        })
    ));
}

#[tokio::test]
async fn major_holder_decimal_conversion_failure_is_reported() {
    let sym = "MAJOR";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let holders_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "majorHoldersBreakdown": {
                        "insidersPercentHeld": { "raw": 1e30 },
                        "institutionsPercentHeld": { "raw": 0.25 },
                        "institutionsCount": { "raw": 42 }
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
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = HoldersBuilder::new(&client, sym)
        .major_holders_with_diagnostics()
        .await
        .unwrap();

    holders_mock.assert();
    assert_eq!(response.data.len(), 2);
    assert!(
        response
            .data
            .iter()
            .any(|holder| { holder.category.contains("% of Shares Held by Institutions") })
    );
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::OmittedPresentField {
            path: "majorHoldersBreakdown.insidersPercentHeld",
            reason: ProjectionIssue::ConversionFailed {
                target: "major holder percent"
            },
            ..
        }
    )));
}

#[tokio::test]
async fn net_share_purchase_activity_missing_period_is_reported() {
    let sym = "NETBAD";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let holders_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "netSharePurchaseActivity": {
                        "period": "",
                        "buyInfoShares": { "raw": 10 }
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
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = HoldersBuilder::new(&client, sym)
        .net_share_purchase_activity_with_diagnostics()
        .await
        .unwrap();

    holders_mock.assert();
    assert!(response.data.is_none());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "net_share_purchase_activity",
            reason: ProjectionIssue::MissingRequiredField { field: "period" },
            ..
        })
    ));
}

#[tokio::test]
async fn strict_net_share_purchase_activity_errors_on_missing_period() {
    let sym = "NETBAD";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let holders_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "netSharePurchaseActivity": { "period": "" }
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
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let err = HoldersBuilder::new(&client, sym)
        .strict()
        .net_share_purchase_activity()
        .await
        .unwrap_err();

    holders_mock.assert();
    assert!(matches!(err, YfError::DataQuality(_)));
}

#[tokio::test]
async fn net_share_purchase_activity_percent_conversion_failure_is_reported() {
    let sym = "NETPCT";
    let server = MockServer::start();
    let modules = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

    let holders_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{
                  "quoteSummary": {
                    "result": [{
                      "netSharePurchaseActivity": {
                        "period": "6m",
                        "buyInfoShares": { "raw": 10 },
                        "netPercentInsiderShares": { "raw": 1e30 }
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
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = HoldersBuilder::new(&client, sym)
        .net_share_purchase_activity_with_diagnostics()
        .await
        .unwrap();

    holders_mock.assert();
    let activity = response
        .data
        .expect("valid period should keep the activity");
    assert_eq!(activity.buy_shares, Some(10));
    assert!(activity.net_percent_insider_shares.is_none());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::OmittedPresentField {
            path: "netSharePurchaseActivity.netPercentInsiderShares",
            reason: ProjectionIssue::ConversionFailed {
                target: "net percent insider shares"
            },
            ..
        })
    ));
}
