use serde::Serialize;
use serde_json::Value;

use crate::{
    core::{
        CallOptions, ProjectionContext, ProjectionIssue, YfClient, YfError,
        client::{CacheEndpoint, normalize_symbol},
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

#[allow(clippy::too_many_lines)]
pub(super) async fn fetch_news(
    client: &YfClient,
    symbol: &str,
    count: u32,
    tab: NewsTab,
    options: &CallOptions,
) -> Result<crate::YfResponse<Vec<NewsArticle>>, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let mut ctx = ProjectionContext::new("news", options.data_quality());
    let mut url = client.base_news().join("xhr/ncp")?;
    url.query_pairs_mut()
        .append_pair("queryRef", tab_as_str(tab))
        .append_pair("serviceKey", "ncp_fin");

    let symbols = [symbol.as_str()];
    let payload = NewsPayload {
        service_config: ServiceConfig {
            snippet_count: count,
            s: &symbols,
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
            options,
            endpoint: &endpoint,
            fixture_key: &symbol,
            ext: "json",
        },
    )
    .await?;

    let articles = envelope
        .data
        .and_then(|d| d.ticker_stream)
        .and_then(|ts| ts.stream)
        .unwrap_or_default();

    let mut results = Vec::new();
    for (idx, raw_item) in articles.into_iter().enumerate() {
        let raw_key = Some(news_stream_item_diag_key(&raw_item, idx));
        let raw_item = match serde_json::from_value::<wire::StreamItem>(raw_item) {
            Ok(raw_item) => raw_item,
            Err(err) => {
                ctx.dropped_item(
                    "news_article",
                    raw_key.clone(),
                    ProjectionIssue::InvalidField {
                        field: "article",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        if raw_item.ad.is_some() {
            ctx.dropped_item(
                "news_article",
                raw_key.clone(),
                ProjectionIssue::ProviderUnavailable { feature: "article" },
            )?;
            continue;
        }

        let Some(id) = raw_item
            .id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
        else {
            ctx.dropped_item(
                "news_article",
                raw_key,
                ProjectionIssue::MissingRequiredField { field: "id" },
            )?;
            continue;
        };
        let key = Some(id.clone());

        let Some(content) = raw_item.content else {
            ctx.dropped_item(
                "news_article",
                key.clone(),
                ProjectionIssue::MissingRequiredField { field: "content" },
            )?;
            continue;
        };
        let Some(title) = content.title else {
            ctx.dropped_item(
                "news_article",
                key.clone(),
                ProjectionIssue::MissingRequiredField { field: "title" },
            )?;
            continue;
        };
        let Some(pub_date_str) = content.pub_date else {
            ctx.dropped_item(
                "news_article",
                key.clone(),
                ProjectionIssue::MissingRequiredField { field: "pubDate" },
            )?;
            continue;
        };

        let timestamp = match chrono::DateTime::parse_from_rfc3339(&pub_date_str) {
            Ok(date) => date.timestamp(),
            Err(err) => {
                ctx.dropped_item(
                    "news_article",
                    key.clone(),
                    ProjectionIssue::InvalidField {
                        field: "pubDate",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let published_at = match i64_to_datetime(timestamp) {
            Ok(date) => date,
            Err(err) => {
                ctx.dropped_item(
                    "news_article",
                    key.clone(),
                    ProjectionIssue::InvalidField {
                        field: "pubDate",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };

        results.push(NewsArticle {
            uuid: id,
            title,
            publisher: content.provider.and_then(|p| p.display_name),
            link: content.canonical_url.and_then(|u| u.url),
            published_at,
            provider: (),
        });
    }

    Ok(ctx.finish(results))
}

fn news_stream_item_diag_key(value: &Value, idx: usize) -> String {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map_or_else(|| format!("stream[{idx}]"), ToString::to_string)
}
