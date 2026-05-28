use httpmock::Method::GET;
use httpmock::MockServer;
use url::Url;
use yfinance_rs::core::Range;
use yfinance_rs::{ProjectionIssue, Ticker, YfClient, YfError, YfWarning};

fn meta_body() -> String {
    r#"{
      "chart":{
        "result":[
          {
            "meta": { "timezone":"America/New_York", "gmtoffset": -14400 },
            "timestamp": [],
            "indicators": {
              "quote":[{ "open":[], "high":[], "low":[], "close":[], "volume":[] }],
              "adjclose":[{ "adjclose":[] }]
            }
          }
        ],
        "error": null
      }
    }"#
    .to_string()
}

fn malformed_timezone_body() -> String {
    r#"{
      "chart":{
        "result":[
          {
            "meta": { "symbol": "MSFT", "timezone":"Not/A_Timezone", "gmtoffset": -14400 },
            "timestamp": [],
            "indicators": {
              "quote":[{ "open":[], "high":[], "low":[], "close":[], "volume":[] }],
              "adjclose":[{ "adjclose":[] }]
            }
          }
        ],
        "error": null
      }
    }"#
    .to_string()
}

#[tokio::test]
async fn get_history_metadata_returns_timezone() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/MSFT")
            .query_param("range", "1d")
            .query_param("interval", "1d")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(meta_body());
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let t = Ticker::new(&client, "MSFT");
    let meta = t.get_history_metadata(Some(Range::D1)).await.unwrap();

    mock.assert();
    let m = meta.expect("meta should be Some");
    assert_eq!(
        m.timezone.as_ref().map(std::string::ToString::to_string),
        Some("America/New_York".to_string())
    );
    assert_eq!(m.utc_offset_seconds, Some(-14400));
}

#[tokio::test]
async fn malformed_history_timezone_is_reported_as_projection_loss() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v8/finance/chart/MSFT")
            .query_param("range", "1d")
            .query_param("interval", "1d")
            .query_param("events", "div|split|capitalGains");
        then.status(200)
            .header("content-type", "application/json")
            .body(malformed_timezone_body());
    });

    let client = YfClient::builder()
        .base_chart(Url::parse(&format!("{}/v8/finance/chart/", server.base_url())).unwrap())
        .build()
        .unwrap();

    let response = Ticker::new(&client, "MSFT")
        .history_builder()
        .range(Range::D1)
        .fetch_full_with_diagnostics()
        .await
        .unwrap();

    assert!(response.data.meta.is_some());
    assert!(
        response
            .data
            .meta
            .as_ref()
            .is_some_and(|meta| meta.timezone.is_none())
    );
    assert!(response.diagnostics.warnings.iter().any(|warning| matches!(
        warning,
        YfWarning::OmittedPresentField {
            path: "chart.meta.timezone",
            reason: ProjectionIssue::InvalidField {
                field: "timezone",
                ..
            },
            ..
        }
    )));

    let err = Ticker::new(&client, "MSFT")
        .history_builder()
        .strict()
        .range(Range::D1)
        .fetch_full()
        .await
        .unwrap_err();

    mock.assert_calls(2);
    assert!(matches!(err, YfError::DataQuality(_)));
}
