use httpmock::Method::GET;
use httpmock::MockServer;
use paft::{Decimal, money::PriceAmount};
use url::Url;
use yfinance_rs::YfClient;

#[tokio::test]
async fn download_drops_malformed_rows_and_rounds_valid_neighbors_to_price_hint() {
    // Well-formed JSON: adjclose belongs inside indicators, and braces are balanced.
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "AAPL", "instrumentType": "EQUITY", "currency": "USD", "priceHint": 3 },
          "timestamp": [10, 20, 30],
          "indicators": {
            "quote": [{
              "open":  [100.0014, null,  99.9944],
              "high":  [101.0094, null, 100.0064],
              "low":   [ 99.0014, null,  98.9944],
              "close": [100.4996, null,  99.9964],
              "volume":[   1000,  2000,    3000]
            }],
            "adjclose": [{
              "adjclose": [100.4996, null, 99.9964]
            }]
          }
        }],
        "error": null
      }
    }"#;

    let server = MockServer::start();
    let sym = "AAPL";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{sym}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = yfinance_rs::DownloadBuilder::new(&client)
        .symbols([sym])
        .rounding(true)
        .run()
        .await
        .unwrap();

    mock.assert();

    let v = &res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == sym)
        .unwrap()
        .history
        .candles;
    assert_eq!(v.len(), 2, "malformed OHLC row is dropped");

    assert_price_eq(&v[0].ohlc.open, Decimal::new(100_001, 3));
    assert_price_eq(&v[0].ohlc.high, Decimal::new(101_009, 3));
    assert_price_eq(&v[0].ohlc.low, Decimal::new(99_001, 3));
    assert_price_eq(&v[0].ohlc.close, Decimal::new(100_500, 3));

    assert_price_eq(&v[1].ohlc.open, Decimal::new(99_994, 3));
    assert_price_eq(&v[1].ohlc.high, Decimal::new(100_006, 3));
    assert_price_eq(&v[1].ohlc.low, Decimal::new(98_994, 3));
    assert_price_eq(&v[1].ohlc.close, Decimal::new(99_996, 3));
    assert_eq!(
        v[1].volume.as_ref().map(ToString::to_string),
        Some("3000".into())
    );
}

#[tokio::test]
async fn download_rounding_without_price_hint_leaves_prices_unchanged() {
    let body = r#"{
      "chart": {
        "result": [{
          "meta": { "symbol": "AAPL", "instrumentType": "EQUITY", "currency": "USD" },
          "timestamp": [10],
          "indicators": {
            "quote": [{
              "open":  [100.001],
              "high":  [101.009],
              "low":   [ 99.001],
              "close": [100.499],
              "volume":[   1000]
            }],
            "adjclose": [{
              "adjclose": [100.499]
            }]
          }
        }],
        "error": null
      }
    }"#;

    let server = MockServer::start();
    let sym = "AAPL";

    let mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{sym}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let res = yfinance_rs::DownloadBuilder::new(&client)
        .symbols([sym])
        .rounding(true)
        .run()
        .await
        .unwrap();

    mock.assert();

    let candle = &res
        .entries
        .iter()
        .find(|e| e.instrument.symbol.as_str() == sym)
        .unwrap()
        .history
        .candles[0];

    assert_price_eq(&candle.ohlc.open, Decimal::new(100_001, 3));
    assert_price_eq(&candle.ohlc.high, Decimal::new(101_009, 3));
    assert_price_eq(&candle.ohlc.low, Decimal::new(99_001, 3));
    assert_price_eq(&candle.ohlc.close, Decimal::new(100_499, 3));
}

fn assert_price_eq(actual: &PriceAmount, expected: Decimal) {
    assert_eq!(actual.as_decimal(), &expected);
}
