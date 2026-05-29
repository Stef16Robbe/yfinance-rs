use paft::domain::AssetKind;
use serde_json::Value;
use url::Url;
use yfinance_rs::core::conversions::{money_to_currency_str, money_to_f64};
use yfinance_rs::{ProjectionIssue, Ticker, YfClient, YfError, YfWarning};

#[tokio::test]
async fn options_expirations_happy() {
    let server = crate::common::setup_server();
    let symbol = "AAPL";

    let mock = crate::common::mock_options_v7(&server, symbol);

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .build()
        .unwrap();
    let t = Ticker::new(&client, symbol);

    let expiries = t.options().await.unwrap();
    mock.assert();

    assert!(
        !expiries.is_empty(),
        "record {symbol} options fixtures first via YF_RECORD=1 cargo test --test ticker -- options"
    );
}

#[tokio::test]
async fn options_expirations_surface_yahoo_error() {
    let server = crate::common::setup_server();
    let symbol = "AAPL";

    let body = r#"{
      "optionChain": {
        "result": null,
        "error": { "description": "No options found" }
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/options/AAPL")
            .is_true(|req| !req.query_params().iter().any(|(k, _)| k == "date"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, symbol);

    let err = ticker.options().await.unwrap_err();
    mock.assert();

    assert_yahoo_api_error(err, "No options found");
}

#[tokio::test]
async fn option_chain_for_specific_date() {
    let server = crate::common::setup_server();
    let symbol = "AAPL";

    let exp_mock = crate::common::mock_options_v7(&server, symbol);
    let quote_mock = crate::common::mock_quote_v7(&server, symbol);

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();
    let t = Ticker::new(&client, symbol);

    let expiries = t.options().await.unwrap();
    exp_mock.assert();

    assert!(
        !expiries.is_empty(),
        "record {symbol} options fixtures first via YF_RECORD=1 cargo test --test ticker -- options"
    );

    let date = expiries[0];
    let chain_mock = crate::common::mock_options_v7_for_date(&server, symbol, date);

    let chain = t.option_chain(Some(date)).await.unwrap();
    chain_mock.assert();
    assert_eq!(
        quote_mock.calls(),
        0,
        "options currency should prevent quote fallback"
    );

    let calls = chain.calls().collect::<Vec<_>>();
    let puts = chain.puts().collect::<Vec<_>>();

    assert!(
        !calls.is_empty(),
        "recorded {symbol} chain should include call contracts"
    );
    assert!(
        !puts.is_empty(),
        "recorded {symbol} chain should include put contracts"
    );

    let c = calls[0];
    assert_eq!(money_to_currency_str(&c.key.strike).as_deref(), Some("USD"));
    assert_eq!(c.expiration_at.unwrap().timestamp(), date);

    let p = puts[0];
    if let Some(price) = p.price.as_ref() {
        assert_eq!(money_to_currency_str(price).as_deref(), Some("USD"));
    }
    assert_eq!(p.expiration_at.unwrap().timestamp(), date);
}

#[tokio::test]
async fn option_chain_with_diagnostics_surfaces_yahoo_error() {
    let server = crate::common::setup_server();
    let date = 1_737_072_000_i64;

    let body = r#"{
      "optionChain": {
        "result": null,
        "error": { "description": "No data found, symbol may be delisted" }
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/options/AAPL")
            .query_param("date", date.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "AAPL");

    let err = ticker
        .option_chain_with_diagnostics(Some(date))
        .await
        .unwrap_err();
    mock.assert();

    assert_yahoo_api_error(err, "No data found");
}

#[tokio::test]
async fn option_chain_uses_response_underlying_identity() {
    let server = crate::common::setup_server();
    let date = 1_737_072_000_i64;

    let body = r#"{
      "optionChain": {
        "result": [{
          "underlyingSymbol":"SPY",
          "quote": {
            "symbol":"SPY",
            "quoteType":"ETF",
            "fullExchangeName":"NYSE",
            "currency":"USD"
          },
          "options": [{
            "expirationDate": 1737072000,
            "calls": [{
              "contractSymbol":"SPY250117C00500000",
              "strike":500.0,
              "expiration":1737072000
            }],
            "puts": []
          }]
        }],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/options/SPY")
            .query_param("date", date.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "SPY");

    let chain = ticker.option_chain(Some(date)).await.unwrap();
    mock.assert();

    let contract = chain.calls().next().expect("call contract");
    assert_eq!(contract.key.underlying.symbol.as_str(), "SPY");
    assert!(matches!(&contract.key.underlying.kind, AssetKind::Fund));
    assert_eq!(
        contract
            .key
            .underlying
            .exchange
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("NYSE")
    );
}

#[tokio::test]
async fn option_chain_with_diagnostics_errors_when_direct_currency_is_invalid() {
    let server = crate::common::setup_server();
    let date = 1_737_072_000_i64;

    let body = r#"{
      "optionChain": {
        "result": [{
          "underlyingSymbol":"AAPL",
          "quote": {
            "symbol":"AAPL",
            "quoteType":"EQUITY",
            "currency":"!!!"
          },
          "options": [{
            "expirationDate": 1737072000,
            "calls": [{
              "contractSymbol":"AAPL250117C00180000",
              "strike":180.0,
              "expiration":1737072000
            }],
            "puts": []
          }]
        }],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/options/AAPL")
            .query_param("date", date.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "AAPL");

    let err = ticker
        .option_chain_with_diagnostics(Some(date))
        .await
        .expect_err("invalid direct option-chain currency should fail");
    mock.assert();

    assert!(matches!(err, YfError::InvalidData(_)));
}

#[tokio::test]
async fn option_chain_currency_fallback_fetches_quote() {
    let server = crate::common::setup_server();
    let symbol = "AAPL";

    assert_fixture_present(symbol);

    let mut base_json = load_options_json(symbol);
    let expiries = extract_expiration_dates(&base_json);
    assert!(
        !expiries.is_empty(),
        "recorded {symbol} options fixture missing expiration dates"
    );
    strip_quote_currency(&mut base_json);
    let base_payload = base_json.to_string();

    let date = expiries[0];
    let fixture_key = format!("{symbol}_{date}");
    assert_fixture_present(&fixture_key);

    let mut dated_json = load_options_json(&fixture_key);
    strip_quote_currency(&mut dated_json);
    let dated_payload = dated_json.to_string();

    let base_mock = mock_base_options_request(&server, symbol, base_payload);
    let chain_mock = mock_dated_options_request(&server, symbol, date, dated_payload);
    let quote_mock = crate::common::mock_quote_v7(&server, symbol);

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, symbol);

    let expiries_resp = ticker.options().await.unwrap();
    base_mock.assert();
    assert_eq!(expiries_resp, expiries);

    let chain = ticker.option_chain(Some(date)).await.unwrap();

    chain_mock.assert();
    quote_mock.assert();

    assert!(
        quote_mock.calls() >= 1,
        "fallback should hit quote endpoint at least once"
    );

    let combined = chain.calls().chain(chain.puts()).collect::<Vec<_>>();
    assert!(
        !combined.is_empty(),
        "recorded chain for {symbol} should include contracts"
    );

    for contract in combined {
        assert_eq!(
            money_to_currency_str(&contract.key.strike).as_deref(),
            Some("USD")
        );
        assert_eq!(contract.expiration_at.unwrap().timestamp(), date);
    }
}

#[tokio::test]
async fn option_chain_missing_currency_uses_resolver_inference_after_quote_failure() {
    let server = crate::common::setup_server();
    let symbol = "TSCO.L";
    let date = 1_737_072_000_i64;

    let body = r#"{
      "optionChain": {
        "result": [{
          "underlyingSymbol":"TSCO.L",
          "quote": {
            "symbol":"TSCO.L",
            "quoteType":"EQUITY",
            "fullExchangeName":"London Stock Exchange",
            "exchange":"LSE"
          },
          "options": [{
            "expirationDate": 1737072000,
            "calls": [{
              "contractSymbol":"TSCO250117C00444100",
              "strike":444.1,
              "expiration":1737072000
            }],
            "puts": []
          }]
        }],
        "error": null
      }
    }"#;

    let options_mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path(format!("/v7/finance/options/{symbol}"))
            .query_param("date", date.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });
    let quote_mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/quote")
            .query_param("symbols", symbol);
        then.status(500);
    });

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let chain = Ticker::new(&client, symbol)
        .option_chain(Some(date))
        .await
        .unwrap();

    options_mock.assert();
    assert!(quote_mock.calls() >= 1);
    let contract = chain.calls().next().expect("call contract");
    assert_eq!(
        money_to_currency_str(&contract.key.strike).as_deref(),
        Some("GBP")
    );
    assert!((money_to_f64(&contract.key.strike) - 4.441).abs() < 1e-9);
}

#[tokio::test]
async fn option_chain_skips_bad_strikes_and_keeps_valid_contracts() {
    let server = crate::common::setup_server();
    let date = 1_737_072_000_i64;

    let body = r#"{
      "optionChain": {
        "result": [{
          "underlyingSymbol": "AAPL",
          "quote": { "symbol": "AAPL", "quoteType": "EQUITY", "currency": "USD" },
          "options": [{
            "expirationDate": 1737072000,
            "calls": [
              {
                "contractSymbol":"AAPL250117C00170000",
                "strike":1e30,
                "expiration":1737072000,
                "lastPrice":5.0
              },
              {
                "contractSymbol":"AAPL250117C00175000",
                "strike":"not-a-number",
                "expiration":1737072000,
                "lastPrice":4.0
              },
              {
                "contractSymbol":"AAPL250117C00180000",
                "strike":180.0,
                "expiration":1737072000,
                "lastPrice":1e30,
                "bid":1.25,
                "ask":1e30,
                "impliedVolatility":1e30
              }
            ],
            "puts": [{
              "contractSymbol":"AAPL250117P00175000",
              "strike":175.0,
              "expiration":1737072000,
              "lastPrice":2.0
            }]
          }]
        }],
        "error": null
      }
    }"#;

    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path("/v7/finance/options/AAPL")
            .query_param("date", date.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    });

    let client = YfClient::builder()
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, "AAPL");

    let response = ticker
        .option_chain_with_diagnostics(Some(date))
        .await
        .unwrap();
    mock.assert();

    let calls = response.data.calls().collect::<Vec<_>>();
    let puts_count = response.data.puts().count();

    assert_eq!(
        calls.len(),
        1,
        "contracts with malformed or unrepresentable strikes are skipped"
    );
    assert_eq!(puts_count, 1, "valid sibling put survives");
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::DroppedItem {
            endpoint: "options",
            item: "option_contract",
            key: Some(key),
            reason: ProjectionIssue::InvalidField {
                field: "contract",
                ..
            },
        } if key == "AAPL250117C00175000"
    )));

    let call = calls[0];
    assert!((money_to_f64(&call.key.strike) - 180.0).abs() < 1e-9);
    assert_eq!(call.price, None, "invalid optional last price becomes None");
    assert!(call.bid.is_some(), "valid optional bid survives");
    assert_eq!(call.ask, None, "invalid optional ask becomes None");
    assert_eq!(
        call.implied_volatility, None,
        "invalid optional IV becomes None"
    );
}

fn assert_fixture_present(id: &str) {
    assert!(
        crate::common::fixture_exists("options_v7", id, "json"),
        "record {id} options fixtures via YF_RECORD=1 cargo test --test ticker -- options"
    );
}

fn load_options_json(id: &str) -> Value {
    let body = crate::common::fixture("options_v7", id, "json");
    serde_json::from_str(&body).expect("options fixture json")
}

fn extract_expiration_dates(json: &Value) -> Vec<i64> {
    json["optionChain"]["result"][0]["expirationDates"]
        .as_array()
        .expect("expirationDates array")
        .iter()
        .map(|v| v.as_i64().expect("epoch"))
        .collect()
}

fn strip_quote_currency(json: &mut Value) {
    if let Some(obj) = json
        .get_mut("optionChain")
        .and_then(|oc| oc.get_mut("result"))
        .and_then(|arr| arr.get_mut(0))
        .and_then(|node| node.get_mut("quote"))
        .and_then(|quote| quote.as_object_mut())
    {
        obj.remove("currency");
    }
}

fn mock_base_options_request<'a>(
    server: &'a httpmock::MockServer,
    symbol: &str,
    payload: String,
) -> httpmock::Mock<'a> {
    let symbol = symbol.to_string();
    let body = payload;
    server.mock(move |when, then| {
        when.method(httpmock::Method::GET)
            .path(format!("/v7/finance/options/{symbol}"))
            .is_true(|req| !req.query_params().iter().any(|(k, _)| k == "date"));
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    })
}

fn mock_dated_options_request<'a>(
    server: &'a httpmock::MockServer,
    symbol: &str,
    date: i64,
    payload: String,
) -> httpmock::Mock<'a> {
    let symbol = symbol.to_string();
    let body = payload;
    server.mock(move |when, then| {
        when.method(httpmock::Method::GET)
            .path(format!("/v7/finance/options/{symbol}"))
            .query_param("date", date.to_string());
        then.status(200)
            .header("content-type", "application/json")
            .body(body);
    })
}

fn assert_yahoo_api_error(err: YfError, expected: &str) {
    match err {
        YfError::Api(message) => assert!(
            message.contains("yahoo error:") && message.contains(expected),
            "expected Yahoo API error containing {expected:?}; got {message}"
        ),
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn options_retry_with_crumb_on_403() {
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use url::Url;
    use yfinance_rs::{Ticker, YfClient};

    let server = MockServer::start();

    // First call returns 403 (unauthorized) ONLY when the crumb is missing.
    let date = 1_737_072_000_i64;
    let first = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/options/MSFT")
            .query_param("date", date.to_string())
            .is_true(|req| !req.query_params().iter().any(|(k, _)| k == "crumb"));
        then.status(403);
    });

    // Cookie + crumb endpoints for ensure_credentials()
    let cookie = server.mock(|when, then| {
        when.method(GET).path("/consent");
        then.status(200).header(
            "set-cookie",
            "A=B; Max-Age=315360000; Domain=.yahoo.com; Path=/; Secure; SameSite=None",
        );
    });

    let crumb = server.mock(|when, then| {
        when.method(GET).path("/v1/test/getcrumb");
        then.status(200).body("crumb-value");
    });

    let stale = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/options/MSFT")
            .query_param("date", date.to_string())
            .query_param("crumb", "stale-crumb");
        then.status(403);
    });

    // Second attempt with ?crumb= should succeed
    let ok_body = r#"{
      "optionChain": {
        "result": [{
          "underlyingSymbol":"MSFT",
          "expirationDates":[1737072000],
          "quote": {
            "symbol": "MSFT",
            "quoteType": "EQUITY",
            "currency": "USD"
          },
          "options": [{
            "expirationDate": 1737072000,
            "calls": [],
            "puts": []
          }]
        }],
        "error": null
      }
    }"#;

    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/options/MSFT")
            .query_param("date", date.to_string())
            .query_param("crumb", "crumb-value");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok_body);
    });

    let client = YfClient::builder()
        .cookie_url(Url::parse(&format!("{}/consent", server.base_url())).unwrap())
        .crumb_url(Url::parse(&format!("{}/v1/test/getcrumb", server.base_url())).unwrap())
        .base_options_v7(Url::parse(&format!("{}/v7/finance/options/", server.base_url())).unwrap())
        ._preauth("cookie", "stale-crumb")
        .build()
        .unwrap();

    let t = Ticker::new(&client, "MSFT");

    let chain = t.option_chain(Some(date)).await.unwrap();
    assert!(chain.calls().next().is_none() && chain.puts().next().is_none());

    first.assert();
    stale.assert();
    cookie.assert();
    crumb.assert();
    second.assert();
}
