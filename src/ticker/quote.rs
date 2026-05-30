use crate::core::{
    CallOptions, ProjectionContext, YfClient, YfError, YfResponse,
    models::{FastInfo, Quote},
    quotes,
};
use paft::fundamentals::statistics::KeyStatistics;

fn log_err<T>(
    ctx: &mut ProjectionContext,
    res: Result<T, YfError>,
    name: &'static str,
    symbol: &str,
) -> Result<Option<T>, YfError> {
    match res {
        Ok(data) => Ok(Some(data)),
        Err(e) => {
            crate::core::logging::trace_debug!(
                symbol,
                module = name,
                error = %e,
                "optional key statistics module fetch failed"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = (name, symbol, &e);
            ctx.suppressed_error(name, &e)?;
            Ok(None)
        }
    }
}

pub async fn fetch_quote(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<Quote, YfError> {
    Ok(fetch_quote_with_diagnostics(client, symbol, options)
        .await?
        .into_data())
}

pub async fn fetch_quote_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<YfResponse<Quote>, YfError> {
    let mut ctx = ProjectionContext::new("quote", options.data_quality());
    let symbols = [symbol];
    let results = quotes::fetch_v7_quote_values(client, &symbols, options).await?;
    let result = quotes::required_quote_node_from_values_with_context(results, symbol, &mut ctx)?;

    let quote = result.to_quote_with_context(&mut ctx)?;
    Ok(ctx.finish(quote))
}

pub async fn fetch_fast_info(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<FastInfo, YfError> {
    Ok(fetch_fast_info_with_diagnostics(client, symbol, options)
        .await?
        .into_data())
}

pub async fn fetch_fast_info_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<YfResponse<FastInfo>, YfError> {
    let mut ctx = ProjectionContext::new("fast_info", options.data_quality());
    let symbols = [symbol];
    let results = quotes::fetch_v7_quote_values(client, &symbols, options).await?;
    let result = quotes::required_quote_node_from_values_with_context(results, symbol, &mut ctx)?;

    let snapshot = result.to_snapshot_with_context(&mut ctx)?;
    Ok(ctx.finish(snapshot))
}

pub async fn fetch_key_statistics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<KeyStatistics, YfError> {
    Ok(
        fetch_key_statistics_with_diagnostics(client, symbol, options)
            .await?
            .into_data(),
    )
}

pub async fn fetch_key_statistics_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<YfResponse<KeyStatistics>, YfError> {
    let mut ctx = ProjectionContext::new("key_statistics", options.data_quality());
    let symbols = [symbol];
    let (quote_res, quote_summary_res) = tokio::join!(
        quotes::fetch_v7_quote_values(client, &symbols, options),
        quotes::fetch_quote_summary_key_statistics(client, symbol, options)
    );

    let results = quote_res?;
    if results.is_empty() {
        return Err(YfError::MissingData(format!(
            "no quote result found for symbol {symbol}"
        )));
    }
    let stats = match quotes::first_quote_node_from_values_with_context(results, &mut ctx)? {
        Some(result) => result.to_key_statistics_with_context(&mut ctx)?,
        None => KeyStatistics::default(),
    };
    let stats = match log_err(
        &mut ctx,
        quote_summary_res,
        "quote_summary_key_statistics",
        symbol,
    )? {
        Some(quote_summary) => quotes::merge_key_statistics(stats, &quote_summary),
        None => stats,
    };

    Ok(ctx.finish(stats))
}
