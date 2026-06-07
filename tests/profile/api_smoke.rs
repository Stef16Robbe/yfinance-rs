use crate::common::setup_server;
use httpmock::Method::GET;
use paft::fundamentals::profile::Profile;
use url::Url;
use yfinance_rs::{YfClient, YfError};

#[tokio::test]
async fn profile_api_company_happy() {
    let server = setup_server();
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

    let prof = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap();
    mock.assert();

    match prof {
        Profile::Company(c) => {
            assert_eq!(c.name, "Apple Inc.");
            assert_eq!(c.sector.as_deref(), Some("Technology"));
            assert_eq!(c.industry.as_deref(), Some("Consumer Electronics"));
            assert_eq!(c.website.as_deref(), Some("https://www.apple.com"));
        }
        _ => panic!("expected Company"),
    }
}

#[tokio::test]
async fn profile_api_wrong_type_optional_field_does_not_fail_profile() {
    let server = setup_server();
    let sym = "AAPL";
    let crumb = "test-crumb";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                serde_json::json!({
                    "quoteSummary": {
                        "error": null,
                        "result": [{
                            "quoteType": {
                                "quoteType": "EQUITY",
                                "longName": "Apple Inc.",
                                "symbol": sym
                            },
                            "assetProfile": {
                                "sector": 42,
                                "industry": "Consumer Electronics",
                                "country": "United States"
                            }
                        }]
                    }
                })
                .to_string(),
            );
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", crumb)
        .build()
        .unwrap();

    let prof = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap();
    mock.assert();

    match prof {
        Profile::Company(c) => {
            assert_eq!(c.name, "Apple Inc.");
            assert_eq!(c.sector, None);
            assert_eq!(c.industry.as_deref(), Some("Consumer Electronics"));
        }
        _ => panic!("expected Company"),
    }
}

#[tokio::test]
async fn profile_api_fund_happy() {
    let server = setup_server();
    let sym = "QQQ";
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

    let prof = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap();
    mock.assert();

    match prof {
        Profile::Fund(f) => {
            assert_eq!(f.name, "Invesco QQQ Trust");
            assert_eq!(f.family.as_deref(), Some("Invesco"));
            assert_eq!(f.kind.to_string(), "ETF");
        }
        _ => panic!("expected Fund"),
    }
}

#[tokio::test]
async fn profile_api_synthetic_etf_missing_legal_type_uses_quote_type() {
    let server = setup_server();
    let sym = "SYNTH_ETF";
    let crumb = "test-crumb";

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", crumb);
        then.status(200)
            .header("content-type", "application/json")
            .body(
                serde_json::json!({
                    "quoteSummary": {
                        "error": null,
                        "result": [{
                            "fundProfile": {
                                "family": "Synthetic Family"
                            },
                            "quoteType": {
                                "quoteType": "ETF",
                                "longName": "Synthetic ETF",
                                "symbol": sym
                            }
                        }]
                    }
                })
                .to_string(),
            );
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        ._preauth("cookie", crumb)
        .build()
        .unwrap();

    let prof = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap();
    mock.assert();

    match prof {
        Profile::Fund(f) => {
            assert_eq!(f.name, "Synthetic ETF");
            assert_eq!(f.family.as_deref(), Some("Synthetic Family"));
            assert_eq!(f.kind.to_string(), "ETF");
        }
        _ => panic!("expected Fund"),
    }
}

#[tokio::test]
async fn profile_api_mutual_fund_happy() {
    let server = setup_server();
    let sym = "VTSAX";
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

    let prof = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap();
    mock.assert();

    match prof {
        Profile::Fund(f) => {
            assert_eq!(f.name, "Vanguard Total Stock Mkt Idx Adm");
            assert_eq!(f.family.as_deref(), Some("Vanguard"));
            assert_eq!(f.kind.to_string(), "MUTUAL_FUND");
        }
        _ => panic!("expected Fund"),
    }
}

#[tokio::test]
async fn profile_api_unsupported_quote_type_is_provider_data_error() {
    for (sym, quote_type) in [("^GSPC", "INDEX"), ("BTC-USD", "CRYPTOCURRENCY")] {
        let server = setup_server();
        let crumb = "test-crumb";

        let mock = server.mock(|when, then| {
            when.method(GET)
                .path(format!("/v10/finance/quoteSummary/{sym}"))
                .query_param("modules", "assetProfile,quoteType,fundProfile")
                .query_param("crumb", crumb);
            then.status(200)
                .header("content-type", "application/json")
                .body(
                    serde_json::json!({
                        "quoteSummary": {
                            "error": null,
                            "result": [{
                                "quoteType": {
                                    "quoteType": quote_type,
                                    "longName": sym,
                                    "symbol": sym
                                }
                            }]
                        }
                    })
                    .to_string(),
                );
        });

        let client = YfClient::builder()
            .base_quote_api(
                Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
            )
            ._preauth("cookie", crumb)
            .build()
            .unwrap();

        let err = yfinance_rs::profile::load_profile(&client, sym)
            .await
            .unwrap_err();
        mock.assert();

        assert!(
            matches!(
                err,
                YfError::InvalidData(ref message)
                    if message.contains("profile unavailable")
                        && message.contains(quote_type)
            ),
            "{sym} {quote_type} should fail as provider data, got {err:?}"
        );
    }
}
