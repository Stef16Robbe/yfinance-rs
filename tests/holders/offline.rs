use httpmock::{Method::GET, Mock, MockServer};
use paft::Decimal;
use serde_json::Value;
use url::Url;
use yfinance_rs::core::conversions::*;
use yfinance_rs::{Ticker, YfClient};

const INSTITUTION_OWNERSHIP: &str = "institutionOwnership";
const FUND_OWNERSHIP: &str = "fundOwnership";
const MAJOR_HOLDERS: &str = "majorHoldersBreakdown";
const INSIDER_TRANSACTIONS: &str = "insiderTransactions";
const INSIDER_HOLDERS: &str = "insiderHolders";
const NET_SHARE_PURCHASE_ACTIVITY: &str = "netSharePurchaseActivity";

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

fn setup_holders_mock<'a>(server: &'a MockServer, symbol: &'a str, modules: &'a str) -> Mock<'a> {
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{symbol}"))
            .query_param("modules", modules)
            .query_param("crumb", "crumb");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture(&format!("holders_api_{modules}"), symbol));
    })
}

fn holders_client(server: &MockServer) -> YfClient {
    YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", "crumb")
        .build()
        .unwrap()
}

fn quote_result(raw: &Value, symbol: &str) -> Value {
    raw["quoteResponse"]["result"]
        .as_array()
        .and_then(|quotes| {
            quotes
                .iter()
                .find(|quote| quote["symbol"].as_str() == Some(symbol))
        })
        .cloned()
        .unwrap_or_else(|| panic!("quote fixture should contain {symbol}"))
}

fn major_currency_code(code: &str) -> &str {
    match code {
        "GBp" | "GBX" => "GBP",
        "ZAc" => "ZAR",
        "ILA" => "ILS",
        other => other,
    }
}

fn assert_quote_fixture_currency(symbol: &str, quote_currency: &str, financial_currency: &str) {
    let raw: Value = serde_json::from_str(&fixture("quote_v7", symbol)).unwrap();
    let quote = quote_result(&raw, symbol);
    assert_eq!(quote["currency"].as_str(), Some(quote_currency));
    assert_eq!(
        quote["financialCurrency"].as_str(),
        Some(financial_currency)
    );
}

fn first_ownership_value(raw: &str, module: &str) -> u64 {
    let raw: Value = serde_json::from_str(raw).unwrap();
    raw["quoteSummary"]["result"]
        .as_array()
        .and_then(|results| results.first())
        .and_then(|result| result[module]["ownershipList"].as_array())
        .and_then(|rows| rows.iter().find_map(|row| row["value"]["raw"].as_u64()))
        .expect("ownership fixture should contain a raw value")
}

fn first_insider_transaction_value(raw: &str) -> u64 {
    let raw: Value = serde_json::from_str(raw).unwrap();
    raw["quoteSummary"]["result"]
        .as_array()
        .and_then(|results| results.first())
        .and_then(|result| result["insiderTransactions"]["transactions"].as_array())
        .and_then(|rows| rows.iter().find_map(|row| row["value"]["raw"].as_u64()))
        .expect("insider transaction fixture should contain a raw value")
}

#[tokio::test]
async fn offline_all_holders_from_fixture() {
    let sym = "AAPL";
    let server = MockServer::start();
    let major_mock = setup_holders_mock(&server, sym, MAJOR_HOLDERS);
    let institutional_mock = setup_holders_mock(&server, sym, INSTITUTION_OWNERSHIP);
    let mutual_fund_mock = setup_holders_mock(&server, sym, FUND_OWNERSHIP);
    let insider_transactions_mock = setup_holders_mock(&server, sym, INSIDER_TRANSACTIONS);
    let insider_roster_mock = setup_holders_mock(&server, sym, INSIDER_HOLDERS);
    let net_purchase_mock = setup_holders_mock(&server, sym, NET_SHARE_PURCHASE_ACTIVITY);
    let quote_mock = crate::common::mock_quote_v7(&server, sym);

    let client = holders_client(&server);

    // Test each method; each will make an independent API call which the mock will serve.
    let t = Ticker::new(&client, sym);

    // Major Holders
    let major = t.major_holders().await.unwrap();
    assert!(!major.is_empty(), "major holders missing from fixture");
    assert!(
        major
            .iter()
            .any(|h| h.category.contains("Held by All Insider"))
    );
    assert!(
        major
            .iter()
            .any(|h| h.category.contains("Held by Institutions"))
    );

    // Institutional Holders
    let institutional = t.institutional_holders().await.unwrap();
    assert!(
        !institutional.is_empty(),
        "institutional holders missing from fixture"
    );
    assert!(institutional[0].shares.unwrap_or(0) > 0);

    // Mutual Fund Holders
    let mutual_fund = t.mutual_fund_holders().await.unwrap();
    assert!(
        !mutual_fund.is_empty(),
        "mutual fund holders missing from fixture"
    );
    assert!(money_to_f64(mutual_fund[0].value.as_ref().unwrap()) > 0.0);

    // Insider Roster
    let insider_roster = t.insider_roster_holders().await.unwrap();
    assert!(
        !insider_roster.is_empty(),
        "insider roster missing from fixture"
    );
    assert!(
        insider_roster
            .iter()
            .any(|h| h.name.to_lowercase().contains("cook"))
    );

    // Net Share Purchase Activity
    let net_purchase = t.net_share_purchase_activity().await.unwrap().unwrap();
    assert!(!net_purchase.period.to_string().is_empty());
    assert!(net_purchase.total_insider_shares.unwrap_or(0) > 0);

    // Insider Transactions (can be empty)
    let _insider_trans = t.insider_transactions().await.unwrap();

    major_mock.assert();
    institutional_mock.assert();
    mutual_fund_mock.assert();
    insider_transactions_mock.assert();
    insider_roster_mock.assert();
    net_purchase_mock.assert();
    quote_mock.assert();
}

#[tokio::test]
async fn recorded_holder_values_use_trading_major_currency() {
    for (sym, quote_currency, financial_currency) in
        [("SAP", "USD", "EUR"), ("TSCO.L", "GBp", "GBP")]
    {
        assert_quote_fixture_currency(sym, quote_currency, financial_currency);

        let server = MockServer::start();
        let holders_fixture = fixture("holders_api_institutionOwnership", sym);
        let expected_value = first_ownership_value(&holders_fixture, INSTITUTION_OWNERSHIP);
        let holders_mock = setup_holders_mock(&server, sym, INSTITUTION_OWNERSHIP);
        let quote_mock = crate::common::mock_quote_v7(&server, sym);
        let client = holders_client(&server);

        let rows = Ticker::new(&client, sym)
            .institutional_holders()
            .await
            .unwrap();

        holders_mock.assert();
        quote_mock.assert();
        let value = rows
            .iter()
            .find_map(|holder| holder.value.as_ref())
            .unwrap_or_else(|| panic!("{sym} fixture should map at least one holder value"));
        assert_eq!(
            value.currency().to_string(),
            major_currency_code(quote_currency)
        );
        assert_eq!(value.amount(), Decimal::from(expected_value));
    }
}

#[tokio::test]
async fn recorded_insider_transaction_values_use_trading_major_currency() {
    let sym = "AAPL";
    assert_quote_fixture_currency(sym, "USD", "USD");

    let server = MockServer::start();
    let transactions_fixture = fixture("holders_api_insiderTransactions", sym);
    let expected_value = first_insider_transaction_value(&transactions_fixture);
    let transactions_mock = setup_holders_mock(&server, sym, INSIDER_TRANSACTIONS);
    let quote_mock = crate::common::mock_quote_v7(&server, sym);
    let client = holders_client(&server);

    let rows = Ticker::new(&client, sym)
        .insider_transactions()
        .await
        .unwrap();

    transactions_mock.assert();
    quote_mock.assert();
    let value = rows
        .iter()
        .find_map(|transaction| transaction.value.as_ref())
        .expect("AAPL fixture should map at least one insider transaction value");
    assert_eq!(value.currency().to_string(), "USD");
    assert_eq!(value.amount(), Decimal::from(expected_value));
}
