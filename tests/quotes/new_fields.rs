use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::YfClient;
use yfinance_rs::core::conversions::money_to_f64;

#[tokio::test]
async fn quote_v7_bid_ask_are_mapped_to_book_levels() {
    let server = MockServer::start();
    let fixture = crate::common::fixture("quote_v7", "AAPL", "json");
    let raw: serde_json::Value = serde_json::from_str(&fixture).unwrap();
    let raw_quote = raw["quoteResponse"]["result"]
        .as_array()
        .and_then(|quotes| quotes.first())
        .expect("quote fixture should contain AAPL");
    let expected_bid = raw_quote["bid"].as_f64().expect("fixture bid");
    let expected_bid_size = raw_quote["bidSize"].as_u64().expect("fixture bid size");
    let expected_ask = raw_quote["ask"].as_f64().expect("fixture ask");
    let expected_ask_size = raw_quote["askSize"].as_u64().expect("fixture ask size");

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "AAPL");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let quotes = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL"])
        .fetch()
        .await
        .unwrap();

    mock.assert();
    let quote = quotes.first().expect("quote fixture should contain AAPL");
    let bid = quote.bid.as_ref().expect("bid should be mapped");
    let ask = quote.ask.as_ref().expect("ask should be mapped");

    assert!((money_to_f64(&bid.price) - expected_bid).abs() < 1e-9);
    assert_eq!(bid.size, Some(paft::Decimal::from(expected_bid_size)));
    assert!((money_to_f64(&ask.price) - expected_ask).abs() < 1e-9);
    assert_eq!(ask.size, Some(paft::Decimal::from(expected_ask_size)));

    #[cfg(feature = "dataframe")]
    {
        use paft::prelude::ToDataFrame;

        let df = quote.to_dataframe().unwrap();
        assert_eq!(df.height(), 1);
    }
}
