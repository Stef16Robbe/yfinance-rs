use crate::common;
use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::{YfClient, YfError};

#[tokio::test]
async fn missing_set_cookie_header_is_an_error() {
    let server = MockServer::start();
    let sym = "AAPL";

    // Cookie endpoint returns 200 but no Set-Cookie header.
    let cookie = server.mock(|when, then| {
        when.method(GET).path("/consent");
        then.status(200); // no set-cookie
    });
    let crumb = server.mock(|when, then| {
        // won't be reached, but good to have
        when.method(GET).path("/v1/test/getcrumb");
        then.status(200).body("crumb-value");
    });

    // Any API body (won't be reached if ensure_credentials fails early)
    let api = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"));
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteSummary":{"result":[],"error":null}}"#);
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .cookie_url(Url::parse(&format!("{}/consent", server.base_url())).unwrap())
        .crumb_url(Url::parse(&format!("{}/v1/test/getcrumb", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap_err();
    cookie.assert();

    match err {
        YfError::Auth(s) => assert!(s.contains("No cookie received"), "unexpected error: {s}"),
        other => panic!("expected Auth error, got {other:?}"),
    }
    assert_eq!(
        crumb.calls(),
        0,
        "crumb endpoint should not be called if cookie fails"
    );
    assert_eq!(
        api.calls(),
        0,
        "API should not be called if credentials fail"
    );
}

#[tokio::test]
async fn invalid_crumb_body_is_an_error() {
    let server = MockServer::start();
    let sym = "AAPL";

    // Proper Set-Cookie
    let _cookie = server.mock(|when, then| {
        when.method(GET).path("/consent");
        then.status(200).header("set-cookie", "A=B; Path=/");
    });
    // Crumb endpoint returns malformed credential text, which should be rejected
    // without echoing the credential back in the public error.
    let _crumb = server.mock(|when, then| {
        when.method(GET).path("/v1/test/getcrumb");
        then.status(200).body("secret-crumb<");
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .cookie_url(Url::parse(&format!("{}/consent", server.base_url())).unwrap())
        .crumb_url(Url::parse(&format!("{}/v1/test/getcrumb", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap_err();

    match err {
        YfError::Auth(s) => {
            assert!(s.contains("Received invalid crumb"), "unexpected: {s}");
            assert!(!s.contains("secret-crumb"), "crumb leaked in error: {s}");
        }
        other => panic!("expected Auth error, got {other:?}"),
    }
}

#[tokio::test]
async fn crumb_error_phrase_body_is_not_used_as_credential() {
    let server = MockServer::start();
    let sym = "AAPL";

    let _cookie = server.mock(|when, then| {
        when.method(GET).path("/consent");
        then.status(200).header("set-cookie", "A=B; Path=/");
    });
    let _crumb = server.mock(|when, then| {
        when.method(GET).path("/v1/test/getcrumb");
        then.status(200).body("Too Many Requests");
    });
    let api = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "Too Many Requests");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ));
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .cookie_url(Url::parse(&format!("{}/consent", server.base_url())).unwrap())
        .crumb_url(Url::parse(&format!("{}/v1/test/getcrumb", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap_err();

    match err {
        YfError::Auth(s) => {
            assert!(s.contains("Received invalid crumb"), "unexpected: {s}");
            assert!(
                !s.contains("Too Many Requests"),
                "crumb leaked in error: {s}"
            );
        }
        other => panic!("expected Auth error, got {other:?}"),
    }
    assert_eq!(
        api.calls(),
        0,
        "API should not be called with an error phrase as the crumb"
    );
}

#[tokio::test]
async fn non_success_crumb_response_is_not_stored() {
    let server = MockServer::start();
    let sym = "AAPL";

    let _cookie = server.mock(|when, then| {
        when.method(GET).path("/consent");
        then.status(200).header("set-cookie", "A=B; Path=/");
    });
    let _crumb = server.mock(|when, then| {
        when.method(GET).path("/v1/test/getcrumb");
        then.status(429).body("crumb-value");
    });
    let api = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "crumb-value");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ));
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .cookie_url(Url::parse(&format!("{}/consent", server.base_url())).unwrap())
        .crumb_url(Url::parse(&format!("{}/v1/test/getcrumb", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap_err();

    match err {
        YfError::RateLimited { .. } => {}
        other => panic!("expected RateLimited error, got {other:?}"),
    }
    assert_eq!(
        api.calls(),
        0,
        "API should not be called with the failed crumb response body"
    );
}

#[tokio::test]
async fn successful_crumb_response_is_trimmed() {
    let server = MockServer::start();
    let sym = "AAPL";

    let _cookie = server.mock(|when, then| {
        when.method(GET).path("/consent");
        then.status(200).header("set-cookie", "A=B; Path=/");
    });
    let _crumb = server.mock(|when, then| {
        when.method(GET).path("/v1/test/getcrumb");
        then.status(200).body("  crumb-value\n");
    });
    let api = server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v10/finance/quoteSummary/{sym}"))
            .query_param("modules", "assetProfile,quoteType,fundProfile")
            .query_param("crumb", "crumb-value");
        then.status(200)
            .header("content-type", "application/json")
            .body(common::fixture(
                "profile_api_assetProfile-quoteType-fundProfile",
                sym,
                "json",
            ));
    });

    let client = YfClient::builder()
        .base_quote_api(
            Url::parse(&format!("{}/v10/finance/quoteSummary/", server.base_url())).unwrap(),
        )
        .cookie_url(Url::parse(&format!("{}/consent", server.base_url())).unwrap())
        .crumb_url(Url::parse(&format!("{}/v1/test/getcrumb", server.base_url())).unwrap())
        .build()
        .unwrap();

    yfinance_rs::profile::load_profile(&client, sym)
        .await
        .unwrap();

    api.assert();
}
