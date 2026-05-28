use serde::Serialize;

use crate::{
    core::{
        YfClient, YfError,
        client::{CacheEndpoint, CacheMode, RetryConfig},
        conversions::i64_to_datetime,
        net,
    },
    news::{NewsTab, model::NewsArticle, tab_as_str, wire},
};

#[derive(Serialize)]
struct ServiceConfig<'a> {
    #[serde(rename = "snippetCount")]
    snippet_count: u32,
    s: &'a [&'a str],
}

#[derive(Serialize)]
struct NewsPayload<'a> {
    #[serde(rename = "serviceConfig")]
    service_config: ServiceConfig<'a>,
}

pub(super) async fn fetch_news(
    client: &YfClient,
    symbol: &str,
    count: u32,
    tab: NewsTab,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<NewsArticle>, YfError> {
    let mut url = client.base_news().join("xhr/ncp")?;
    url.query_pairs_mut()
        .append_pair("queryRef", tab_as_str(tab))
        .append_pair("serviceKey", "ncp_fin");

    let payload = NewsPayload {
        service_config: ServiceConfig {
            snippet_count: count,
            s: &[symbol],
        },
    };

    let endpoint = format!("news_{}", tab_as_str(tab));
    let body_json = serde_json::to_string(&payload).map_err(YfError::Json)?;
    let envelope: wire::NewsEnvelope = net::fetch_json_post_cached(
        client,
        &url,
        &body_json,
        net::CacheFetchConfig {
            cache_endpoint: CacheEndpoint::News,
            cache_mode,
            retry_override,
            endpoint: &endpoint,
            fixture_key: symbol,
            ext: "json",
        },
    )
    .await?;

    let articles = envelope
        .data
        .and_then(|d| d.ticker_stream)
        .and_then(|ts| ts.stream)
        .unwrap_or_default();

    let results = articles
        .into_iter()
        .filter_map(|raw_item| {
            // Filter out ads or items that are not valid articles
            if raw_item.ad.is_some() {
                return None;
            }

            let content = raw_item.content?;
            let title = content.title?;
            let pub_date_str = content.pub_date?;

            // Parse the RFC3339 string to a timestamp
            let timestamp = chrono::DateTime::parse_from_rfc3339(&pub_date_str)
                .ok()?
                .timestamp();
            let published_at = i64_to_datetime(timestamp).ok()?;

            Some(NewsArticle {
                uuid: raw_item.id,
                title,
                publisher: content.provider.and_then(|p| p.display_name),
                link: content.canonical_url.and_then(|u| u.url),
                published_at,
                provider: (),
            })
        })
        .collect();

    Ok(results)
}
