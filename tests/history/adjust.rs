use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::Interval;
use yfinance_rs::core::conversions::*;
use yfinance_rs::{
    Action, HistoryBuilder, OhlcPriceBasis, PriceBasis, YfClient, YfError, YfWarning,
};

fn date_from_ts(timestamp: i64) -> chrono::NaiveDate {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap()
        .date_naive()
}

#[tokio::test]
async fn sparse_adjclose_uses_one_split_adjusted_basis_with_diagnostic() {
    let server = MockServer::start();

    let body = r#"{
      "chart":{
        "result":[
          {
            "meta":{"currency":"USD","symbol":"TEST","instrumentType":"EQUITY"},
            "timestamp":[1000,2000,3000],
            "indicators":{
              "quote":[{
                "open":[100.0,100.0,100.0],
                "high":[101.0,101.0,101.0],
                "low":[99.0,99.0,99.0],
                "close":[100.0,100.0,100.0],
                "volume":[10,10,10]
              }],
              "adjclose":[{"adjclose":[50.0,null,99.0]}]
            },
            "events":{
              "splits":{
                "2000":{"date":2000,"numerator":2,"denominator":1}
              },
              "dividends":{
                "3000":{"date":3000,"amount":1.0}
              }
            }
          }
        ],
        "error":null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/TEST");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = HistoryBuilder::new(&client, "TEST")
        .interval(Interval::D1)
        .auto_adjust(true)
        .fetch_full_with_diagnostics()
        .await
        .unwrap();

    let resp = response.data;
    assert_eq!(
        resp.price_basis,
        OhlcPriceBasis::uniform(PriceBasis::split_adjusted_latest())
    );
    assert_eq!(resp.candles.len(), 3);
    assert!((money_to_f64(&resp.candles[0].ohlc.close) - 50.0).abs() < 1e-9);
    assert!((money_to_f64(&resp.candles[1].ohlc.close) - 100.0).abs() < 1e-9);
    assert!((money_to_f64(&resp.candles[2].ohlc.close) - 100.0).abs() < 1e-9);
    assert!(
        response
            .diagnostics
            .warnings
            .iter()
            .any(|warning| matches!(
                warning,
                YfWarning::RepairedData {
                    endpoint: "history_chart",
                    item: "candle_adjustment",
                    repair:
                        "ignored sparse chart.indicators.adjclose and used split-only adjustment for all candles",
                    ..
                }
            ))
    );

    let err = HistoryBuilder::new(&client, "TEST")
        .interval(Interval::D1)
        .auto_adjust(true)
        .strict()
        .fetch_full()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(
        err,
        YfError::DataQuality(warning)
            if matches!(
                *warning,
                YfWarning::RepairedData {
                    endpoint: "history_chart",
                    item: "candle_adjustment",
                    ..
                }
            )
    ));
}

#[tokio::test]
async fn history_auto_adjust_and_actions() {
    let server = MockServer::start();

    // Three days:
    // t1=1000 (before 2:1 split), t2=2000 (split date), t3=3000 (dividend date)
    // OHLC all ~100, volume = 10 each day
    // adjclose encodes: 0.5 factor on t1 (split), 1.0 on t2, 0.99 on t3 (dividend)
    let body = r#"{
      "chart":{
        "result":[
          {
            "timestamp":[1000,2000,3000],
            "indicators":{
              "quote":[{
                "open":[100.0,100.0,100.0],
                "high":[101.0,101.0,101.0],
                "low":[99.0,99.0,99.0],
                "close":[100.0,100.0,100.0],
                "volume":[10,10,10]
              }],
              "adjclose":[{"adjclose":[50.0,100.0,99.0]}]
            },
            "events":{
              "splits":{
                "2000":{"date":2000,"numerator":2,"denominator":1}
              },
              "dividends":{
                "3000":{"date":3000,"amount":1.0}
              }
            }
          }
        ],
        "error":null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/TEST");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let resp = HistoryBuilder::new(&client, "TEST")
        .interval(Interval::D1)
        .auto_adjust(true)
        .fetch_full()
        .await
        .unwrap();

    mock.assert();

    assert_eq!(
        resp.price_basis,
        OhlcPriceBasis::uniform(PriceBasis::provider_latest_adjusted())
    );
    assert_eq!(resp.candles.len(), 3);

    // t1 (1000): prices halved, volume stays as reported by Yahoo.
    let c0 = &resp.candles[0];
    assert!((money_to_f64(&c0.ohlc.open) - 50.0).abs() < 1e-9);
    assert!((money_to_f64(&c0.ohlc.high) - 50.5).abs() < 1e-9);
    assert!((money_to_f64(&c0.ohlc.low) - 49.5).abs() < 1e-9);
    assert!((money_to_f64(&c0.ohlc.close) - 50.0).abs() < 1e-9);
    assert_eq!(
        c0.volume.as_ref().map(ToString::to_string),
        Some("10".into())
    );

    // t2 (2000): unchanged prices, unchanged volume
    let c1 = &resp.candles[1];
    assert!((money_to_f64(&c1.ohlc.close) - 100.0).abs() < 1e-9);
    assert_eq!(
        c1.volume.as_ref().map(ToString::to_string),
        Some("10".into())
    );

    // t3 (3000): dividend -> adjclose=99 => factor 0.99
    let c2 = &resp.candles[2];
    assert!((money_to_f64(&c2.ohlc.close) - 99.0).abs() < 1e-9);
    assert_eq!(
        c2.volume.as_ref().map(ToString::to_string),
        Some("10".into())
    );

    // Actions preserve corporate-action calendar dates and payloads.
    assert_eq!(resp.actions.len(), 2);
    assert!(resp.actions.iter().any(|action| {
        matches!(
            action,
            Action::Split { date, numerator, denominator }
                if *date == date_from_ts(2000) && numerator.get() == 2 && denominator.get() == 1
        )
    }));
    assert!(resp.actions.iter().any(|action| {
        matches!(
            action,
            Action::Dividend { date, amount }
                if *date == date_from_ts(3000) && (money_to_f64(amount)-1.0).abs()<1e-9
        )
    }));
}
