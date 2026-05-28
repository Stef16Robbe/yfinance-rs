use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{ApiPreference, Ticker, YfClient};

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
