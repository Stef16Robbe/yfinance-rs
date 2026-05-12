use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::YfClient;
use yfinance_rs::core::conversions::money_to_f64;

#[tokio::test]
async fn quote_v7_bid_ask_are_mapped_to_book_levels() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture("quote_v7", "AAPL", "json"));
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let quotes = yfinance_rs::QuotesBuilder::new(client)
        .symbols(["AAPL"])
        .fetch()
        .await
        .unwrap();

    mock.assert();
    let quote = quotes.first().expect("quote fixture should contain AAPL");
    let bid = quote.bid.as_ref().expect("bid should be mapped");
    let ask = quote.ask.as_ref().expect("ask should be mapped");

    assert!((money_to_f64(&bid.price) - 255.72).abs() < 1e-9);
    assert_eq!(bid.size, Some(paft::Decimal::from(1)));
    assert!((money_to_f64(&ask.price) - 269.86).abs() < 1e-9);
    assert_eq!(ask.size, Some(paft::Decimal::from(1)));

    #[cfg(feature = "dataframe")]
    {
        use paft::prelude::ToDataFrame;

        let df = quote.to_dataframe().unwrap();
        assert_eq!(df.height(), 1);
    }
}
