use httpmock::{Method::GET, MockServer};
use paft::fundamentals::profile::Profile;
use url::Url;
use yfinance_rs::{Ticker, YfClient, YfError};

#[tokio::test]
async fn offline_profile_uses_recorded_fixture() {
    let server = MockServer::start();
    let sym = "AAPL";
    let crumb = "test-crumb";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(crate::common::fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ));
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", crumb)
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, sym);

    let profile = ticker.profile().await.unwrap();
    mock.assert_calls(1);

    match profile {
        Profile::Company(company) => {
            assert_eq!(company.name, "Apple Inc.");
            assert_eq!(company.sector.as_deref(), Some("Technology"));
            assert_eq!(company.industry.as_deref(), Some("Consumer Electronics"));
            assert_eq!(company.website.as_deref(), Some("https://www.apple.com"));
        }
        _ => panic!("expected company profile"),
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_profile_smoke_and_or_record() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = YfClient::builder().build().unwrap();
    let sym = "AAPL";
    let ticker = Ticker::new(&client, sym);
    let profile = ticker.profile().await.unwrap();

    if crate::common::is_recording() {
        assert!(
            crate::common::fixture_exists(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ),
            "recording pass should persist quoteSummary profile fixture for {sym}"
        );
    } else {
        match profile {
            Profile::Company(company) => {
                assert!(!company.name.is_empty());
                assert!(
                    company.sector.is_some() || company.industry.is_some(),
                    "live company profile for {sym} should include classification metadata"
                );
            }
            _ => panic!("expected company profile"),
        }
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_profile_unsupported_quote_types_are_provider_data_errors() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = YfClient::builder().build().unwrap();

    for (sym, quote_type) in [("^GSPC", "INDEX"), ("BTC-USD", "CRYPTOCURRENCY")] {
        let err = Ticker::new(&client, sym).profile().await.unwrap_err();
        assert!(
            matches!(
                err,
                YfError::InvalidData(ref message)
                    if message.contains("profile unavailable")
                        && message.contains(quote_type)
            ),
            "{sym} should report unsupported profile quoteType {quote_type} as provider data, got {err:?}"
        );
    }
}
