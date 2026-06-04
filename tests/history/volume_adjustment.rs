use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::Interval;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{DownloadBuilder, YfClient};

#[tokio::test]
async fn download_auto_adjust_leaves_yahoo_volume_unchanged_across_split() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET).path("/v8/finance/chart/NVDA");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(
                "history_chart",
                "NVDA_SPLIT_VOLUME",
                "json",
            ));
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = DownloadBuilder::new(&client)
        .symbols(["NVDA"])
        .interval(Interval::D1)
        .run()
        .await
        .unwrap();

    mock.assert();
    let candles = &response.entries[0].history.candles;
    assert_eq!(candles.len(), 4);
    assert!((money_to_f64(&candles[1].ohlc.close) - 120.82).abs() < 1e-9);
    assert_eq!(
        candles[1].volume.as_ref().map(ToString::to_string),
        Some("412386000".into())
    );
    assert!((money_to_f64(&candles[2].ohlc.close) - 121.72).abs() < 1e-9);
    assert_eq!(
        candles[2].volume.as_ref().map(ToString::to_string),
        Some("313434100".into())
    );
}
