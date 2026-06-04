use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::Interval;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{Action, HistoryBuilder, YfClient};

fn date_from_ts(timestamp: i64) -> chrono::NaiveDate {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap()
        .date_naive()
}

#[tokio::test]
async fn history_tolerates_fractional_split_components() {
    let server = MockServer::start();
    let symbol = "AXIA-P";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{symbol}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("history_chart", symbol, "json"));
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, symbol)
        .interval(Interval::D1)
        .auto_adjust(true)
        .actions(true)
        .fetch_full()
        .await
        .unwrap();

    mock.assert();
    assert_eq!(response.candles.len(), 2);
    assert!((money_to_f64(&response.candles[0].ohlc.close) - 100.0).abs() < 1e-9);
    assert_eq!(
        response.candles[0].volume.as_ref().map(ToString::to_string),
        Some("10".into())
    );
    assert_eq!(response.actions.len(), 1);
    assert!(
        matches!(
            response.actions[0],
            Action::Split {
                date,
                numerator,
                denominator,
            } if date == date_from_ts(2000)
                && numerator.get() == 631_419
                && denominator.get() == 500_000
        ),
        "fractional split should be normalized and gcd-simplified"
    );
}

#[tokio::test]
async fn history_skips_split_components_that_overflow_action_ratio() {
    let server = MockServer::start();
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "currency": "USD" },
          "timestamp": [1000, 2000],
          "indicators": {
            "quote": [{
              "open": [100.0, 100.0],
              "high": [100.0, 100.0],
              "low": [100.0, 100.0],
              "close": [100.0, 100.0],
              "volume": [10, 10]
            }],
            "adjclose": []
          },
          "events": {
            "splits": {
              "2000": { "date": 2000, "numerator": 4294967296, "denominator": 1 }
            }
          }
        }],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/OVERFLOW");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "OVERFLOW")
        .interval(Interval::D1)
        .auto_adjust(true)
        .actions(true)
        .fetch_full()
        .await
        .unwrap();

    mock.assert();
    assert_eq!(response.candles.len(), 2);
    assert!(response.actions.is_empty());
    assert!((money_to_f64(&response.candles[0].ohlc.close) - 100.0).abs() < 1e-9);
    assert_eq!(
        response.candles[0].volume.as_ref().map(ToString::to_string),
        Some("10".into())
    );
}
