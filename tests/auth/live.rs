use yfinance_rs::{PredefinedScreener, QuotesBuilder, SearchBuilder, Ticker, YfClient, screen};

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_cached_crumb_optional_endpoints_smoke() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = YfClient::builder().build().unwrap();

    // Seed a real Yahoo cookie + crumb through a required-crumb endpoint, then
    // verify optional-crumb endpoints accept that cached crumb immediately.
    let aapl = Ticker::new(&client, "AAPL");
    let _profile = aapl.profile().await.unwrap();

    let quotes = QuotesBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .fetch()
        .await
        .unwrap();
    assert_eq!(quotes.len(), 2);

    let search = SearchBuilder::new(&client, "apple").fetch().await.unwrap();
    assert!(
        search
            .results
            .iter()
            .any(|q| q.instrument.symbol.as_str() == "AAPL")
    );

    let predefined = screen(&client, PredefinedScreener::DayGainers)
        .await
        .unwrap();
    assert!(!predefined.results.is_empty());

    let expiries = aapl.options().await.unwrap();
    assert!(!expiries.is_empty());
}
