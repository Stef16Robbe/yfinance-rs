use httpmock::Method::GET;
use httpmock::MockServer;
use paft::money::{Currency, IsoCurrency};
use url::Url;
use yfinance_rs::core::{Interval, Range};
use yfinance_rs::{Ticker, YfClient};

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

fn has_fixture(endpoint: &str, symbol: &str) -> bool {
    crate::common::fixture_exists(endpoint, symbol, "json")
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn offline_currency_inference_uses_timeseries_currency_code_first() {
    let symbol = "TSCO.L";

    assert!(
        has_fixture("profile_api_assetProfile-quoteType-fundProfile", symbol),
        "missing fixture profile_api_assetProfile-quoteType-fundProfile_{symbol}.json"
    );
    assert!(
        has_fixture("timeseries_income_statement_annual", symbol),
        "missing fixture timeseries_income_statement_annual_{symbol}.json"
    );

    let server = MockServer::start();

    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", symbol);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"TSCO.L","quoteType":"EQUITY","currency":"GBp","exchange":"LSE","fullExchangeName":"London Stock Exchange"}],"error":null}}"#,
            );
    });

    let quote_summary_currency_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "financialData,earnings")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteSummary":{"result":[{"financialData":{},"earnings":{}}],"error":null}}"#,
            );
    });

    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                symbol,
            ));
    });

    let fundamentals_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{symbol}"
            ))
            .query_param_exists("type")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("timeseries_income_statement_annual", symbol));
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, symbol);

    let rows = ticker.income_stmt(None).await.unwrap();
    let inferred_currency = rows
        .first()
        .and_then(|row| row.total_revenue.as_ref().map(|m| m.currency().clone()));
    assert_eq!(inferred_currency, Some(Currency::Iso(IsoCurrency::GBP)));

    let cached_before_override = ticker.income_stmt(None).await.unwrap();
    let cached_before_currency = cached_before_override
        .first()
        .and_then(|row| row.total_revenue.as_ref().map(|m| m.currency().clone()));
    assert_eq!(
        cached_before_currency,
        Some(Currency::Iso(IsoCurrency::GBP))
    );

    let rows_override = ticker
        .income_stmt(Some(Currency::Iso(IsoCurrency::USD)))
        .await
        .unwrap();
    let override_currency = rows_override
        .first()
        .and_then(|row| row.total_revenue.as_ref().map(|m| m.currency().clone()));
    assert_eq!(override_currency, Some(Currency::Iso(IsoCurrency::USD)));

    let rows_cached = ticker.income_stmt(None).await.unwrap();
    let cached_currency = rows_cached
        .first()
        .and_then(|row| row.total_revenue.as_ref().map(|m| m.currency().clone()));
    assert_eq!(cached_currency, Some(Currency::Iso(IsoCurrency::GBP)));

    assert_eq!(
        quote_mock.calls(),
        0,
        "timeseries currencyCode should avoid quote enrichment"
    );
    assert_eq!(
        quote_summary_currency_mock.calls(),
        0,
        "timeseries currencyCode should avoid quoteSummary enrichment"
    );
    assert_eq!(
        profile_mock.calls(),
        0,
        "timeseries currencyCode should avoid profile enrichment"
    );
    assert_eq!(
        fundamentals_mock.calls(),
        4,
        "fundamentals should be fetched four times"
    );
}

#[tokio::test]
async fn offline_currency_inference_falls_back_to_profile_country_when_provider_currency_missing() {
    let symbol = "TSCO.L";

    assert!(
        has_fixture("profile_api_assetProfile-quoteType-fundProfile", symbol),
        "missing fixture profile_api_assetProfile-quoteType-fundProfile_{symbol}.json"
    );

    let server = MockServer::start();

    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", symbol);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"TSCO.L","quoteType":"EQUITY","currency":"GBp","exchange":"LSE","fullExchangeName":"London Stock Exchange"}],"error":null}}"#,
            );
    });

    let quote_summary_currency_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "financialData,earnings")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteSummary":{"result":[{"financialData":{},"earnings":{}}],"error":null}}"#,
            );
    });

    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                symbol,
            ));
    });

    let body = r#"{
      "timeseries": {
        "result": [{
          "meta": { "type": ["annualTotalRevenue"] },
          "timestamp": [1704067200],
          "annualTotalRevenue": [{
            "asOfDate": "2024-01-01",
            "periodType": "12M",
            "reportedValue": {"raw": 42.0}
          }]
        }],
        "error": null
      }
    }"#;
    let fundamentals_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{symbol}"
            ))
            .query_param_exists("type")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let rows = Ticker::new(&client, symbol)
        .income_stmt(None)
        .await
        .unwrap();
    let inferred_currency = rows
        .first()
        .and_then(|row| row.total_revenue.as_ref().map(|m| m.currency().clone()));

    assert_eq!(inferred_currency, Some(Currency::Iso(IsoCurrency::GBP)));
    assert_eq!(quote_mock.calls(), 1);
    assert_eq!(quote_summary_currency_mock.calls(), 1);
    assert_eq!(profile_mock.calls(), 1);
    assert_eq!(fundamentals_mock.calls(), 1);
}

#[tokio::test]
async fn offline_gs2c_dual_listing_currency() {
    let symbol = "GS2C.DE";

    assert!(
        has_fixture("quote_v7", symbol),
        "missing fixture quote_v7_{symbol}.json"
    );
    assert!(
        has_fixture("profile_api_assetProfile-quoteType-fundProfile", symbol),
        "missing fixture profile_api_assetProfile-quoteType-fundProfile_{symbol}.json"
    );
    assert!(
        has_fixture("timeseries_income_statement_annual", symbol),
        "missing fixture timeseries_income_statement_annual_{symbol}.json"
    );
    assert!(
        has_fixture("history_chart", symbol),
        "missing fixture history_chart_{symbol}.json"
    );

    let server = MockServer::start();

    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", symbol);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("quote_v7", symbol));
    });

    let profile_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                symbol,
            ));
    });

    let fundamentals_mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!(
                "/ws/fundamentals-timeseries/v1/finance/timeseries/{symbol}"
            ))
            .query_param_exists("type")
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("timeseries_income_statement_annual", symbol));
    });

    let chart_mock = server.mock(|when, then| {
        when.method(GET).path(format!("/v8/finance/chart/{symbol}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("history_chart", symbol));
    });

    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .base_timeseries(
            Url::parse(&format!(
                "{}/ws/fundamentals-timeseries/v1/finance/timeseries/",
                server.base_url()
            ))
            .unwrap(),
        )
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        ._preauth("cookie", "crumb")
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, symbol);

    let fast = ticker.fast_info().await.unwrap();
    assert_eq!(
        fast.snapshot
            .last
            .as_ref()
            .or(fast.snapshot.previous_close.as_ref())
            .map(|price| price.currency().to_string())
            .as_deref(),
        Some("EUR")
    );

    let fundamentals = ticker.income_stmt(None).await.unwrap();
    let fundamentals_currency = fundamentals
        .first()
        .and_then(|row| row.total_revenue.as_ref().map(|m| m.currency().clone()));
    assert_eq!(fundamentals_currency, Some(Currency::Iso(IsoCurrency::USD)));

    let history = ticker
        .history(Some(Range::D5), Some(Interval::D1), false)
        .await
        .unwrap();
    let history_currency = history.first().map(|bar| bar.close.currency().clone());
    assert_eq!(history_currency, Some(Currency::Iso(IsoCurrency::EUR)));

    assert_eq!(quote_mock.calls(), 1);
    assert_eq!(profile_mock.calls(), 0);
    assert_eq!(fundamentals_mock.calls(), 1);
    assert_eq!(chart_mock.calls(), 1);
}
