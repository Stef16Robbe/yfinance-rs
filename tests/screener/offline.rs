use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;
use url::Url;
use yfinance_rs::{
    CacheMode, EquityQuery, PredefinedScreener, Region, ScreenerBuilder, YfClient, equity_fields,
};

fn fixture(endpoint: &str, key: &str) -> String {
    crate::common::fixture(endpoint, key, "json")
}

#[tokio::test]
async fn offline_predefined_day_gainers_uses_get_with_expected_params() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/finance/screener/predefined/saved")
            .query_param("scrIds", "day_gainers")
            .query_param("corsDomain", "finance.yahoo.com")
            .query_param("formatted", "false")
            .query_param("lang", "en-US")
            .query_param("region", "US");
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("screener_predefined", "day_gainers"));
    });

    let client = YfClient::default();
    let base = Url::parse(&format!(
        "{}/v1/finance/screener/predefined/saved",
        server.base_url()
    ))
    .unwrap();
    let response = ScreenerBuilder::predefined(&client, PredefinedScreener::DayGainers)
        .predefined_screener_base(base)
        .cache_mode(CacheMode::Bypass)
        .fetch()
        .await
        .unwrap();

    mock.assert();
    assert!(response.count.is_some_and(|count| count > 0));
    assert!(!response.results.is_empty());
    assert!(response.results[0].symbol.is_some());
    assert!(response.results[0].quote_type.is_some());
}

#[tokio::test]
async fn offline_custom_equity_query_posts_python_wire_shape() {
    let server = MockServer::start();
    let expected_body = json!({
        "offset": 0,
        "count": 25,
        "sortField": "ticker",
        "sortType": "DESC",
        "userId": "",
        "userIdType": "guid",
        "quoteType": "EQUITY",
        "query": {
            "operator": "AND",
            "operands": [
                {"operator": "GT", "operands": ["percentchange", 3.0]},
                {"operator": "EQ", "operands": ["region", "us"]}
            ]
        }
    });

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/finance/screener")
            .query_param("corsDomain", "finance.yahoo.com")
            .query_param("formatted", "false")
            .query_param("lang", "en-US")
            .query_param("region", "US")
            .json_body(expected_body);
        then.status(200)
            .header("content-type", "application/json")
            .body(fixture("screener_custom", "equity"));
    });

    let client = YfClient::default();
    let query = EquityQuery::and(vec![
        equity_fields::PERCENT_CHANGE.gt(yfinance_rs::PercentPoints::new(3.0).unwrap()),
        equity_fields::REGION.eq(Region::Us),
    ])
    .unwrap();
    let base = Url::parse(&format!("{}/v1/finance/screener", server.base_url())).unwrap();
    let response = ScreenerBuilder::equity(&client, query)
        .screener_base(base)
        .fetch()
        .await
        .unwrap();

    mock.assert();
    assert!(!response.results.is_empty());
    assert!(response.results[0].symbol.is_some());
}
