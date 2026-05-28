use httpmock::Method::GET;
use httpmock::MockServer;
use paft::fundamentals::statistics::KeyStatistics;
use paft::money::{Currency, IsoCurrency};
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
async fn key_statistics_market_cap_uses_major_units_for_minor_unit_quote_currency() {
    let server = MockServer::start();
    let sym = "TSCO.L";
    let crumb = "test-crumb";
    let quote_body = r#"{
      "quoteResponse": {
        "result": [{
          "symbol": "TSCO.L",
          "quoteType": "EQUITY",
          "currency": "GBp",
          "financialCurrency": "GBP",
          "regularMarketPrice": 444.1,
          "fiftyTwoWeekHigh": 455.0,
          "fiftyTwoWeekLow": 300.0,
          "epsTrailingTwelveMonths": 0.27,
          "dividendRate": 0.15,
          "marketCap": 28024838144,
          "sharesOutstanding": 6310478847
        }],
        "error": null
      }
    }"#;

    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(quote_body);
    });
    let key_statistics_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", crate::common::KEY_STATISTICS_MODULES)
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteSummary":{"result":[{}],"error":null}}"#);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", crumb)
        .build()
        .unwrap();

    let stats = Ticker::new(&client, sym).key_statistics().await.unwrap();

    quote_mock.assert();
    key_statistics_mock.assert();
    let market_cap = stats.market_cap.as_ref().expect("market cap should map");
    assert_eq!(market_cap.currency(), &Currency::Iso(IsoCurrency::GBP));
    assert!((money_to_f64(market_cap) - 28_024_838_144.0).abs() < 0.1);

    let eps = stats
        .eps_trailing_twelve_months
        .as_ref()
        .expect("EPS should map");
    assert_eq!(eps.currency(), &Currency::Iso(IsoCurrency::GBP));
    assert!((money_to_f64(eps) - 0.27).abs() < 1e-9);

    let dividend = stats
        .dividend_per_share_forward
        .as_ref()
        .expect("dividend should map");
    assert_eq!(dividend.currency(), &Currency::Iso(IsoCurrency::GBP));
    assert!((money_to_f64(dividend) - 0.15).abs() < 1e-9);

    let high = stats
        .fifty_two_week_high
        .as_ref()
        .expect("52-week high should map");
    assert_eq!(high.currency(), &Currency::Iso(IsoCurrency::GBP));
    assert!((money_to_f64(high) - 4.55).abs() < 1e-9);
}

#[tokio::test]
async fn key_statistics_market_cap_preserves_large_integer_precision() {
    let server = MockServer::start();
    let sym = "BIG";
    let crumb = "test-crumb";
    let exact = 9_007_199_254_740_993_i64;
    let quote_body = format!(
        r#"{{
      "quoteResponse": {{
        "result": [{{
          "symbol": "{sym}",
          "quoteType": "EQUITY",
          "currency": "USD",
          "regularMarketPrice": 10.0,
          "marketCap": {exact}
        }}],
        "error": null
      }}
    }}"#
    );

    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", sym);
        then.status(200)
            .header("content-type", "application/json")
            .body(quote_body);
    });
    let key_statistics_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", crate::common::KEY_STATISTICS_MODULES)
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteSummary":{"result":[{}],"error":null}}"#);
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", crumb)
        .build()
        .unwrap();

    let stats = Ticker::new(&client, sym).key_statistics().await.unwrap();

    quote_mock.assert();
    key_statistics_mock.assert();
    let market_cap = stats.market_cap.as_ref().expect("market cap should map");
    assert_eq!(market_cap.amount(), paft::Decimal::from(exact));
    assert_eq!(market_cap.currency(), &Currency::Iso(IsoCurrency::USD));
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
