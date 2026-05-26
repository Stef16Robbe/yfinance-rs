use httpmock::Method::GET;
use httpmock::MockServer;
use paft::fundamentals::statistics::KeyStatistics;
use url::Url;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{Ticker, YfClient};

fn assert_v7_key_statistics(stats: &KeyStatistics, raw_quote: &serde_json::Value) {
    assert_eq!(
        stats.shares_outstanding,
        raw_quote["sharesOutstanding"].as_u64()
    );
    assert_eq!(
        stats.average_daily_volume_3m,
        raw_quote["averageDailyVolume3Month"].as_u64()
    );
    assert_eq!(stats.ex_dividend_date, None);
    assert!(
        (money_to_f64(stats.market_cap.as_ref().unwrap())
            - raw_quote["marketCap"].as_f64().unwrap())
        .abs()
            < 0.1
    );
    assert!(
        (money_to_f64(stats.eps_trailing_twelve_months.as_ref().unwrap())
            - raw_quote["epsTrailingTwelveMonths"].as_f64().unwrap())
        .abs()
            < 1e-9
    );
    assert!(
        (money_to_f64(stats.dividend_per_share_forward.as_ref().unwrap())
            - raw_quote["dividendRate"].as_f64().unwrap())
        .abs()
            < 1e-9
    );
    assert!(
        (money_to_f64(stats.fifty_two_week_high.as_ref().unwrap())
            - raw_quote["fiftyTwoWeekHigh"].as_f64().unwrap())
        .abs()
            < 1e-9
    );
    assert!(
        (money_to_f64(stats.fifty_two_week_low.as_ref().unwrap())
            - raw_quote["fiftyTwoWeekLow"].as_f64().unwrap())
        .abs()
            < 1e-9
    );
    assert_eq!(
        stats.pe_trailing_twelve_months,
        paft::Decimal::try_from(raw_quote["trailingPE"].as_f64().unwrap()).ok()
    );
    assert_eq!(
        stats.dividend_yield_trailing,
        paft::Decimal::try_from(raw_quote["trailingAnnualDividendYield"].as_f64().unwrap()).ok()
    );
    assert_eq!(
        stats.dividend_yield_forward,
        paft::Decimal::try_from(raw_quote["dividendYield"].as_f64().unwrap())
            .ok()
            .map(|v| v / paft::Decimal::from(100))
    );
}

#[tokio::test]
async fn key_statistics_merge_v7_quote_and_quote_summary_fixtures() {
    let server = MockServer::start();
    let sym = "MSFT";
    let crumb = "test-crumb";
    let fixture = crate::common::fixture("quote_v7", sym, "json");
    let key_statistics_fixture =
        crate::common::fixture(crate::common::KEY_STATISTICS_FIXTURE_ENDPOINT, sym, "json");
    let raw: serde_json::Value = serde_json::from_str(&fixture).unwrap();
    let raw_quote = raw["quoteResponse"]["result"]
        .as_array()
        .and_then(|quotes| quotes.first())
        .expect("quote fixture should contain MSFT");
    assert!(
        raw_quote.get("beta").is_none(),
        "v7 quote fixture must not drive the beta assertion"
    );

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture);
    });
    let key_statistics_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", crate::common::KEY_STATISTICS_MODULES)
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(key_statistics_fixture.clone());
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", crumb)
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, sym);

    let stats = ticker.key_statistics().await.unwrap();
    mock.assert();
    key_statistics_mock.assert();

    assert_v7_key_statistics(&stats, raw_quote);
    assert_eq!(
        stats.beta,
        Some(crate::common::quote_summary_beta(&key_statistics_fixture))
    );

    #[cfg(feature = "dataframe")]
    {
        use paft::prelude::ToDataFrame;

        let df = stats.to_dataframe().unwrap();
        assert_eq!(df.height(), 1);
    }
}
