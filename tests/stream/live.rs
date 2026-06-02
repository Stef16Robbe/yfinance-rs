use tokio::time::{Duration, Instant, timeout};
use yfinance_rs::StreamMethod;

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_stream_smoke() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();

    let builder = yfinance_rs::StreamBuilder::new(&client)
        .symbols(["BTC-USD", "AAPL"])
        .method(StreamMethod::Websocket);

    let (handle, mut rx) = builder.start().await.unwrap();

    let deadline = Instant::now() + Duration::from_mins(2);
    let mut saw_btc_price = false;
    let mut saw_aapl = false;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Some(update) = timeout(remaining, rx.recv())
            .await
            .expect("no live update within timeout")
        else {
            panic!("stream closed without emitting");
        };

        match update.instrument.symbol.as_str() {
            "BTC-USD" => {
                assert!(
                    update
                        .price
                        .as_ref()
                        .map_or(0.0, yfinance_rs::core::conversions::money_to_f64)
                        > 0.0,
                    "BTC-USD price should be positive"
                );
                saw_btc_price = true;
            }
            "AAPL" => {
                saw_aapl = true;
                assert!(
                    update
                        .price
                        .as_ref()
                        .map_or(0.0, yfinance_rs::core::conversions::money_to_f64)
                        > 0.0,
                    "AAPL price should be positive when an equity WebSocket tick is emitted"
                );
            }
            _ => {}
        }

        if saw_btc_price && saw_aapl {
            break;
        }
    }

    handle.abort();
    assert!(saw_btc_price, "BTC-USD should emit a priced 24/7 tick");
}
