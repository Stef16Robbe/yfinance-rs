use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::{ApiPreference, Ticker, YfClient, YfError};

#[tokio::test]
async fn recommendation_trend_missing_period_returns_error() {
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

    let err = Ticker::new(&client, sym)
        .recommendations()
        .await
        .unwrap_err();

    mock.assert();
    assert!(matches!(err, YfError::MissingData(message) if message.contains("period")));
}
