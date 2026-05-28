use httpmock::{Method::POST, MockServer};
use serde_json::json;
use std::time::Duration;
use url::Url;
use yfinance_rs::{CacheMode, NewsTab, Ticker, YfClient};

fn fixture(endpoint: &str, symbol: &str) -> String {
    crate::common::fixture(endpoint, symbol, "json")
}

#[tokio::test]
async fn offline_news_uses_recorded_fixture() {
    let server = MockServer::start();
    let sym = "AAPL";

    let expected_payload = json!({
        "serviceConfig": {
            "snippetCount": 10,
            "s": [sym]
        }
    });

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/xhr/ncp")
            .query_param("queryRef", "latestNews")
            .query_param("serviceKey", "ncp_fin")
            .json_body(expected_payload);
        then.status(200)
            .header("content-type", "application/json")
            // Use the new, specific fixture name
            .body(fixture("news_latestNews", sym));
    });

    let client = YfClient::builder()
        .base_news(Url::parse(&server.base_url()).unwrap())
        .build()
        .unwrap();

    let ticker = Ticker::new(&client, sym);
    let articles = ticker.news().await.unwrap();

    mock.assert();

    // Make the assertion flexible: check that we got some articles, not an exact number.
    assert!(
        !articles.is_empty(),
        "Expected to parse at least one article from the fixture"
    );

    // Perform general checks on the first article
    let first = &articles[0];
    assert!(!first.uuid.is_empty());
    assert!(!first.title.is_empty());
    assert!(first.published_at.timestamp() > 0);
}

#[tokio::test]
async fn offline_news_builder_configures_request() {
    let server = MockServer::start();
    let sym = "AAPL";

    let expected_payload = json!({
        "serviceConfig": {
            "snippetCount": 5,
            "s": [sym]
        }
    });

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/xhr/ncp")
            .query_param("queryRef", "pressRelease") // Corresponds to NewsTab::PressReleases
            .query_param("serviceKey", "ncp_fin")
            .json_body(expected_payload);
        then.status(200)
            .header("content-type", "application/json")
            // Use the new, specific fixture for press releases
            .body(fixture("news_pressRelease", sym));
    });

    let client = YfClient::builder()
        .base_news(Url::parse(&server.base_url()).unwrap())
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, sym);

    let _articles = ticker
        .news_builder()
        .count(5)
        .tab(NewsTab::PressReleases)
        .fetch()
        .await
        .unwrap();

    mock.assert();
}

#[tokio::test]
async fn default_news_cache_mode_bypasses_response_cache() {
    let server = MockServer::start();
    let sym = "AAPL";
    let expected_payload = json!({
        "serviceConfig": {
            "snippetCount": 10,
            "s": [sym]
        }
    });

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/xhr/ncp")
            .query_param("queryRef", "latestNews")
            .query_param("serviceKey", "ncp_fin")
            .json_body(expected_payload);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("news_latestNews", sym));
    });

    let client = YfClient::builder()
        .base_news(Url::parse(&server.base_url()).unwrap())
        .cache_ttl(Duration::from_mins(1))
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, sym);

    assert!(!ticker.news().await.unwrap().is_empty());
    assert!(!ticker.news().await.unwrap().is_empty());
    mock.assert_calls(2);
}

#[tokio::test]
async fn explicit_news_cache_mode_uses_body_aware_response_cache() {
    let server = MockServer::start();
    let sym = "AAPL";
    let latest_payload = json!({
        "serviceConfig": {
            "snippetCount": 10,
            "s": [sym]
        }
    });
    let shorter_payload = json!({
        "serviceConfig": {
            "snippetCount": 5,
            "s": [sym]
        }
    });

    let latest = server.mock(|when, then| {
        when.method(POST)
            .path("/xhr/ncp")
            .query_param("queryRef", "latestNews")
            .query_param("serviceKey", "ncp_fin")
            .json_body(latest_payload);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("news_latestNews", sym));
    });
    let shorter = server.mock(|when, then| {
        when.method(POST)
            .path("/xhr/ncp")
            .query_param("queryRef", "latestNews")
            .query_param("serviceKey", "ncp_fin")
            .json_body(shorter_payload);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("news_latestNews", sym));
    });

    let client = YfClient::builder()
        .base_news(Url::parse(&server.base_url()).unwrap())
        .cache_ttl(Duration::from_mins(1))
        .build()
        .unwrap();
    let ticker = Ticker::new(&client, sym);

    assert!(
        !ticker
            .news_builder()
            .cache_mode(CacheMode::Use)
            .fetch()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !ticker
            .news_builder()
            .count(5)
            .cache_mode(CacheMode::Use)
            .fetch()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !ticker
            .news_builder()
            .cache_mode(CacheMode::Use)
            .fetch()
            .await
            .unwrap()
            .is_empty()
    );

    latest.assert_calls(1);
    shorter.assert_calls(1);
}
