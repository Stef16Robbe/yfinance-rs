use paft::fundamentals::profile::Profile;

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_profile_company() {
    if !(std::env::var("YF_LIVE").ok().as_deref() == Some("1")
        || std::env::var("YF_RECORD").ok().as_deref() == Some("1"))
    {
        return;
    }
    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let prof = yfinance_rs::profile::load_profile(&client, "AAPL")
        .await
        .unwrap();
    match prof {
        Profile::Company(c) => {
            assert!(!c.name.is_empty());
        }
        _ => panic!("expected company"),
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_profile_fund_for_record() {
    if std::env::var("YF_RECORD").ok().as_deref() != Some("1") {
        return;
    }
    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let _ = yfinance_rs::profile::load_profile(&client, "QQQ").await;
    let _ = yfinance_rs::profile::load_profile(&client, "VTSAX").await;
}
