use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{Ticker, YfClient};

#[tokio::test]
async fn key_statistics_are_mapped_from_v7_quote_fixture() {
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
    let ticker = Ticker::new(&client, "AAPL");

    let stats = ticker.key_statistics().await.unwrap();
    mock.assert();

    assert_eq!(stats.shares_outstanding, Some(14_840_390_000));
    assert_eq!(stats.average_daily_volume_3m, Some(54_683_671));
    assert_eq!(stats.ex_dividend_date, None);
    assert!((money_to_f64(stats.market_cap.as_ref().unwrap()) - 4_002_453_389_312.0).abs() < 0.1);
    assert!((money_to_f64(stats.eps_trailing_twelve_months.as_ref().unwrap()) - 6.59).abs() < 1e-9);
    assert!((money_to_f64(stats.dividend_per_share_forward.as_ref().unwrap()) - 1.04).abs() < 1e-9);
    assert!((money_to_f64(stats.fifty_two_week_high.as_ref().unwrap()) - 271.41).abs() < 1e-9);
    assert!((money_to_f64(stats.fifty_two_week_low.as_ref().unwrap()) - 169.21).abs() < 1e-9);
    assert_eq!(
        stats.pe_trailing_twelve_months,
        paft::Decimal::try_from(40.925644_f64).ok()
    );
    assert_eq!(
        stats.dividend_yield_trailing,
        paft::Decimal::try_from(0.0037546468_f64).ok()
    );
    assert_eq!(
        stats.dividend_yield_forward,
        paft::Decimal::try_from(0.39_f64)
            .ok()
            .map(|v| v / paft::Decimal::from(100))
    );

    #[cfg(feature = "dataframe")]
    {
        use paft::prelude::ToDataFrame;

        let df = stats.to_dataframe().unwrap();
        assert_eq!(df.height(), 1);
    }
}
