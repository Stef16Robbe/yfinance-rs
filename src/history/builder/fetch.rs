use crate::core::{
    CallOptions,
    client::{CacheEndpoint, SymbolEndpoint, normalize_symbol},
};
use crate::history::wire::{Events, MetaNode, QuoteBlock};

pub struct Fetched {
    pub ts: Vec<i64>,
    pub quote: QuoteBlock,
    pub adjclose: Vec<Option<f64>>,
    pub events: Option<Events>,
    pub meta: Option<MetaNode>,
}

#[derive(Clone, Copy)]
pub struct ChartFetchRequest {
    pub range: Option<crate::core::Range>,
    pub period: Option<(i64, i64)>,
    pub interval: crate::core::Interval,
    pub include_actions: bool,
    pub include_prepost: bool,
}

pub async fn fetch_chart(
    client: &crate::core::YfClient,
    symbol: &str,
    request: ChartFetchRequest,
    options: &CallOptions,
) -> Result<Fetched, crate::core::YfError> {
    let symbol = normalize_symbol(symbol)?;
    let mut url = client.symbol_url(SymbolEndpoint::Chart, &symbol)?;
    {
        let mut qp = url.query_pairs_mut();

        if let Some((p1, p2)) = request.period {
            if p1 >= p2 {
                return Err(crate::core::YfError::InvalidDates);
            }
            qp.append_pair("period1", &p1.to_string());
            qp.append_pair("period2", &p2.to_string());
        } else if let Some(r) = request.range {
            qp.append_pair("range", crate::core::models::range_as_str(r));
        } else {
            return Err(crate::core::YfError::InvalidParams(
                "no range or period set".into(),
            ));
        }

        qp.append_pair(
            "interval",
            crate::core::models::interval_as_str(request.interval),
        );
        if request.include_actions {
            qp.append_pair("events", "div|split|capitalGains");
        }
        qp.append_pair(
            "includePrePost",
            if request.include_prepost {
                "true"
            } else {
                "false"
            },
        );
    }

    let body = crate::core::net::fetch_text_cached(
        client,
        &url,
        crate::core::net::CacheFetchConfig {
            cache_endpoint: CacheEndpoint::Chart,
            options,
            endpoint: "history_chart",
            fixture_key: &symbol,
            ext: "json",
        },
    )
    .await?;

    decode_chart(&body)
}

// NEW helper to keep fetch_chart compact
fn decode_chart(body: &str) -> Result<Fetched, crate::core::YfError> {
    let envelope: crate::history::wire::ChartEnvelope =
        serde_json::from_str(body).map_err(crate::core::YfError::Json)?;

    let chart = envelope
        .chart
        .ok_or_else(|| crate::core::YfError::MissingData("missing chart".into()))?;

    if let Some(error) = chart.error {
        return Err(crate::core::YfError::Api(format!(
            "chart error: {} - {}",
            error.code, error.description
        )));
    }

    let result = chart
        .result
        .ok_or_else(|| crate::core::YfError::MissingData("missing result".into()))?;

    let first = result
        .first()
        .ok_or_else(|| crate::core::YfError::MissingData("empty result".into()))?;

    let quote = first
        .indicators
        .quote
        .first()
        .ok_or_else(|| crate::core::YfError::MissingData("missing quote".into()))?;
    let adjclose = first
        .indicators
        .adjclose
        .first()
        .map(|a| a.adjclose.clone())
        .unwrap_or_default();

    let ts = first
        .timestamp
        .clone()
        .ok_or_else(|| crate::core::YfError::MissingData("missing timestamps".into()))?;

    Ok(Fetched {
        ts,
        quote: quote.clone(),
        adjclose,
        events: first.events.clone(),
        meta: first.meta.clone(),
    })
}
