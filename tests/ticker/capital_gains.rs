use httpmock::{Method::GET, MockServer};
use url::Url;
use yfinance_rs::core::Range;
use yfinance_rs::{Ticker, YfClient};

#[tokio::test]
async fn offline_capital_gains_from_history() {
    let server = MockServer::start();
    let sym = "VFINX";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v8/finance/chart/{sym}"))
            .query_param("range", "max")
            .query_param("interval", "1d")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("history_chart", sym, "json"));
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let t = Ticker::new(&client, sym);
    let gains = t.capital_gains(Some(Range::Max)).await.unwrap();

    mock.assert();
    assert!(
        gains.iter().all(|(_, amount)| *amount > 0.0),
        "capital gain amounts should be positive when Yahoo includes them"
    );
}
