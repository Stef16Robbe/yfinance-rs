use httpmock::Method::GET;
use httpmock::MockServer;
use paft::money::{Currency, IsoCurrency};
use url::Url;
use yfinance_rs::{Action, HistoryBuilder, ProjectionIssue, YfClient, YfWarning};

#[tokio::test]
async fn action_currency_prefers_event_then_chart_currency() {
    let server = MockServer::start();
    let body = r#"{
      "chart":{"result":[{
        "meta":{"currency":"USD","timezone":"America/New_York","gmtoffset":-14400},
        "timestamp":[1704067200],
        "indicators":{"quote":[{
          "open":[100.0],"high":[101.0],"low":[99.0],"close":[100.5],"volume":[1000]
        }],"adjclose":[{"adjclose":[100.5]}]},
        "events":{
          "dividends":{"1704067200":{"date":1704067200,"amount":1.0,"currency":"EUR"}},
          "capitalGains":{"1704153600":{"date":1704153600,"amount":2.0}}
        }
      }],"error":null}
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/ACTION");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "ACTION")
        .fetch_full()
        .await
        .unwrap();

    mock.assert();
    assert_eq!(response.actions.len(), 2);
    assert!(matches!(
        &response.actions[0],
        Action::Dividend { amount, .. } if amount.currency() == &Currency::Iso(IsoCurrency::EUR)
    ));
    assert!(matches!(
        &response.actions[1],
        Action::CapitalGain { gain, .. } if gain.currency() == &Currency::Iso(IsoCurrency::USD)
    ));
}

#[tokio::test]
async fn action_without_event_or_default_currency_is_omitted() {
    let server = MockServer::start();
    let body = r#"{
      "chart":{"result":[{
        "meta":{"timezone":"America/New_York","gmtoffset":-14400},
        "timestamp":[],
        "indicators":{"quote":[{"open":[],"high":[],"low":[],"close":[],"volume":[]}],"adjclose":[{"adjclose":[]}]},
        "events":{"dividends":{"1704067200":{"date":1704067200,"amount":1.0}}}
      }],"error":null}
    }"#;

    let chart_mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/NOCURRENCY");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "NOCURRENCY");
        then.status(500);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "NOCURRENCY")
        .fetch_full()
        .await
        .unwrap();

    chart_mock.assert();
    assert!(quote_mock.calls() >= 1);
    assert!(response.actions.is_empty());
}

#[tokio::test]
async fn candles_without_resolved_currency_are_dropped_with_diagnostics() {
    let server = MockServer::start();
    let body = r#"{
      "chart":{"result":[{
        "meta":{"timezone":"America/New_York","gmtoffset":-14400},
        "timestamp":[1704067200],
        "indicators":{"quote":[{
          "open":[100.0],"high":[101.0],"low":[99.0],"close":[100.5],"volume":[1000]
        }],"adjclose":[{"adjclose":[100.5]}]}
      }],"error":null}
    }"#;

    let chart_mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/NOCURRENCY");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "NOCURRENCY");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteResponse":{"result":[],"error":null}}"#);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "NOCURRENCY")
        .fetch_full_with_diagnostics()
        .await
        .unwrap();

    chart_mock.assert();
    quote_mock.assert();
    assert!(response.data.candles.is_empty());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "candle",
            reason: ProjectionIssue::CurrencyUnresolved,
            ..
        })
    ));
}

#[tokio::test]
async fn candles_with_invalid_chart_currency_are_dropped_with_diagnostics() {
    let server = MockServer::start();
    let body = r#"{
      "chart":{"result":[{
        "meta":{"currency":"!!!","timezone":"America/New_York","gmtoffset":-14400},
        "timestamp":[1704067200],
        "indicators":{"quote":[{
          "open":[100.0],"high":[101.0],"low":[99.0],"close":[100.5],"volume":[1000]
        }],"adjclose":[{"adjclose":[100.5]}]}
      }],"error":null}
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/BADCURRENCY");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "BADCURRENCY")
        .fetch_full_with_diagnostics()
        .await
        .expect("best-effort invalid chart currency should not abort history");

    assert!(response.data.candles.is_empty());
    assert!(matches!(
        response.diagnostics.warnings.first(),
        Some(YfWarning::DroppedItem {
            item: "candle",
            reason: ProjectionIssue::InvalidCurrency { code },
            ..
        }) if code == "!!!"
    ));

    let err = HistoryBuilder::new(&client, "BADCURRENCY")
        .strict()
        .fetch_full()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, yfinance_rs::YfError::DataQuality(_)));
}
