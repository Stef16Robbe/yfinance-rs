// tests/holders/live.rs

use yfinance_rs::{Ticker, YfClient};

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_holders_smoke_and_or_record() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = YfClient::builder().build().unwrap();
    let t = Ticker::new(&client, "AAPL");

    // AAPL is the stable full holder/insider fixture.
    let major = t.major_holders().await.unwrap();
    let institutional = t.institutional_holders().await.unwrap();
    let mutual_fund = t.mutual_fund_holders().await.unwrap();
    let insider_trans = t.insider_transactions().await.unwrap();
    let insider_roster = t.insider_roster_holders().await.unwrap();
    let net_purchase = t.net_share_purchase_activity().await.unwrap();

    if crate::common::is_recording() {
        for endpoint in [
            "quote_v7",
            "holders_api_majorHoldersBreakdown",
            "holders_api_institutionOwnership",
            "holders_api_fundOwnership",
            "holders_api_insiderTransactions",
            "holders_api_insiderHolders",
            "holders_api_netSharePurchaseActivity",
        ] {
            assert!(
                crate::common::fixture_exists(endpoint, "AAPL", "json"),
                "recording pass should persist {endpoint} fixture for AAPL"
            );
        }

        for sym in ["SAP", "TSCO.L"] {
            let t = Ticker::new(&client, sym);
            let institutional = t.institutional_holders().await.unwrap();
            assert!(
                !institutional.is_empty(),
                "recording pass should capture institutional holders for {sym}"
            );
            for endpoint in ["quote_v7", "holders_api_institutionOwnership"] {
                assert!(
                    crate::common::fixture_exists(endpoint, sym, "json"),
                    "recording pass should persist {endpoint} fixture for {sym}"
                );
            }
        }
    } else {
        assert!(!major.is_empty(), "expected major holders");
        assert!(!institutional.is_empty(), "expected institutional holders");
        assert!(!mutual_fund.is_empty(), "expected mutual fund holders");
        assert!(!insider_roster.is_empty(), "expected insider roster");
        assert!(net_purchase.is_some(), "expected net purchase activity");
        // Insider transactions can often be empty, so we don't assert on it.
        let _ = insider_trans;
    }
}
