use httpmock::Method::GET;
use httpmock::MockServer;
use paft::fundamentals::statistics::KeyStatistics;
use paft::money::{Currency, IsoCurrency};
use url::Url;
use yfinance_rs::core::conversions::money_to_f64;
use yfinance_rs::{ProjectionIssue, Ticker, YfClient, YfWarning};

fn assert_v7_key_statistics(stats: &KeyStatistics, raw_quote: &serde_json::Value) {
    assert_eq!(
        stats.shares_outstanding,
        raw_quote["sharesOutstanding"].as_u64()
    );
    assert_eq!(
        stats.average_daily_volume_3m,
        raw_quote["averageDailyVolume3Month"].as_u64()
    );
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

fn assert_quote_summary_backfilled_statistics(stats: &KeyStatistics, fixture: &str) {
    let raw: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let quote_summary = raw["quoteSummary"]["result"]
        .as_array()
        .and_then(|results| results.first())
        .expect("quoteSummary fixture should contain MSFT");
    let summary_detail = &quote_summary["summaryDetail"];
    let default_key_statistics = &quote_summary["defaultKeyStatistics"];

    assert_quote_summary_valuation_fields(stats, summary_detail, default_key_statistics);
    assert_quote_summary_dividend_fields(stats, summary_detail, fixture);
    assert_quote_summary_range_fields(stats, summary_detail);
    assert_eq!(
        stats.average_daily_volume_3m,
        summary_detail["averageVolume"]["raw"].as_u64()
    );
    assert_eq!(stats.beta, Some(crate::common::quote_summary_beta(fixture)));
}

fn assert_quote_summary_valuation_fields(
    stats: &KeyStatistics,
    summary_detail: &serde_json::Value,
    default_key_statistics: &serde_json::Value,
) {
    assert_eq!(
        stats.market_cap.as_ref().unwrap().amount(),
        paft::Decimal::from(summary_detail["marketCap"]["raw"].as_i64().unwrap())
    );
    assert_eq!(
        stats.market_cap.as_ref().unwrap().currency(),
        &Currency::Iso(IsoCurrency::USD)
    );
    assert_eq!(
        stats.shares_outstanding,
        default_key_statistics["sharesOutstanding"]["raw"].as_u64()
    );
    assert!(
        (money_to_f64(stats.eps_trailing_twelve_months.as_ref().unwrap())
            - default_key_statistics["trailingEps"]["raw"]
                .as_f64()
                .unwrap())
        .abs()
            < 1e-9
    );
    assert_eq!(
        stats.pe_trailing_twelve_months,
        paft::Decimal::try_from(summary_detail["trailingPE"]["raw"].as_f64().unwrap()).ok()
    );
}

fn assert_quote_summary_dividend_fields(
    stats: &KeyStatistics,
    summary_detail: &serde_json::Value,
    fixture: &str,
) {
    assert!(
        (money_to_f64(stats.dividend_per_share_forward.as_ref().unwrap())
            - summary_detail["dividendRate"]["raw"].as_f64().unwrap())
        .abs()
            < 1e-9
    );
    assert_eq!(
        stats.dividend_yield_trailing,
        paft::Decimal::try_from(
            summary_detail["trailingAnnualDividendYield"]["raw"]
                .as_f64()
                .unwrap()
        )
        .ok()
    );
    assert_eq!(
        stats.dividend_yield_forward,
        paft::Decimal::try_from(summary_detail["dividendYield"]["raw"].as_f64().unwrap()).ok()
    );
    assert_eq!(
        stats.ex_dividend_date,
        Some(crate::common::quote_summary_ex_dividend_date(fixture))
    );
}

fn assert_quote_summary_range_fields(stats: &KeyStatistics, summary_detail: &serde_json::Value) {
    assert!(
        (money_to_f64(stats.fifty_two_week_high.as_ref().unwrap())
            - summary_detail["fiftyTwoWeekHigh"]["raw"].as_f64().unwrap())
        .abs()
            < 1e-9
    );
    assert!(
        (money_to_f64(stats.fifty_two_week_low.as_ref().unwrap())
            - summary_detail["fiftyTwoWeekLow"]["raw"].as_f64().unwrap())
        .abs()
            < 1e-9
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
async fn key_statistics_with_diagnostics_reports_invalid_financial_currency() {
    let server = MockServer::start();
    let sym = "AAPL";
    let crumb = "test-crumb";
    let quote_body = r#"{
      "quoteResponse": {
        "result": [{
          "symbol": "AAPL",
          "quoteType": "EQUITY",
          "currency": "USD",
          "financialCurrency": "!!!",
          "regularMarketPrice": 190.25,
          "epsTrailingTwelveMonths": 6.43,
          "dividendRate": 1.04,
          "marketCap": 3000000000000
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

    let response = Ticker::new(&client, sym)
        .key_statistics_with_diagnostics()
        .await
        .unwrap();

    quote_mock.assert();
    key_statistics_mock.assert();
    assert!(response.data.market_cap.is_some());
    assert!(response.data.eps_trailing_twelve_months.is_none());
    assert!(response.diagnostics.warnings.iter().any(|warning| {
        matches!(
            warning,
            YfWarning::OmittedPresentField {
                endpoint: "key_statistics",
                path: "epsTrailingTwelveMonths",
                key: Some(key),
                reason: ProjectionIssue::InvalidCurrency { code },
            } if key == sym && code == "!!!"
        )
    }));
}

#[tokio::test]
async fn key_statistics_with_diagnostics_drops_malformed_v7_node_before_valid_sibling() {
    let server = MockServer::start();
    let sym = "AAPL";
    let crumb = "test-crumb";
    let quote_body = r#"{
      "quoteResponse": {
        "result": [
          {
            "symbol": "AAPL",
            "quoteType": "EQUITY",
            "marketCap": "not-a-number"
          },
          {
            "symbol": "AAPL",
            "quoteType": "EQUITY",
            "currency": "USD",
            "marketCap": 3000000000000
          }
        ],
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

    let response = Ticker::new(&client, sym)
        .key_statistics_with_diagnostics()
        .await
        .unwrap();

    quote_mock.assert();
    key_statistics_mock.assert();
    assert!(response.data.market_cap.is_some());
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::DroppedItem {
            endpoint: "key_statistics",
            item: "quote",
            key: Some(key),
            reason: ProjectionIssue::InvalidField {
                field: "quote",
                ..
            },
        } if key == sym
    )));
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
    assert_eq!(
        stats.ex_dividend_date,
        Some(crate::common::quote_summary_ex_dividend_date(
            &key_statistics_fixture
        ))
    );

    #[cfg(feature = "dataframe")]
    {
        use paft::prelude::ToDataFrame;

        let df = stats.to_dataframe().unwrap();
        assert_eq!(df.height(), 1);
    }
}

#[tokio::test]
async fn key_statistics_backfills_recorded_quote_summary_when_v7_omits_fields() {
    let server = MockServer::start();
    let sym = "MSFT";
    let crumb = "test-crumb";
    let key_statistics_fixture =
        crate::common::fixture(crate::common::KEY_STATISTICS_FIXTURE_ENDPOINT, sym, "json");
    let quote_body = r#"{
      "quoteResponse": {
        "result": [{
          "symbol": "MSFT",
          "quoteType": "EQUITY",
          "currency": "USD"
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

    let stats = Ticker::new(&client, sym).key_statistics().await.unwrap();

    quote_mock.assert();
    key_statistics_mock.assert();
    assert_quote_summary_backfilled_statistics(&stats, &key_statistics_fixture);
}
