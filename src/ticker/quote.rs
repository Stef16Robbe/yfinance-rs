use crate::core::{
    DataQuality, ProjectionContext, YfClient, YfError, YfResponse,
    client::{CacheMode, RetryConfig},
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
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Quote, YfError> {
    Ok(fetch_quote_with_diagnostics(
        client,
        symbol,
        cache_mode,
        retry_override,
        DataQuality::BestEffort,
    )
    .await?
    .into_data())
}

pub async fn fetch_quote_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Quote>, YfError> {
    let mut ctx = ProjectionContext::new("quote", data_quality);
    let symbols = [symbol];
    let mut results = quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override).await?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    let quote = result.to_quote_with_context(&mut ctx)?;
    Ok(ctx.finish(quote))
}

pub async fn fetch_fast_info(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<FastInfo, YfError> {
    Ok(fetch_fast_info_with_diagnostics(
        client,
        symbol,
        cache_mode,
        retry_override,
        DataQuality::BestEffort,
    )
    .await?
    .into_data())
}

pub async fn fetch_fast_info_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<FastInfo>, YfError> {
    let mut ctx = ProjectionContext::new("fast_info", data_quality);
    let symbols = [symbol];
    let mut results = quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override).await?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    let snapshot = result.to_snapshot_with_context(&mut ctx)?;
    Ok(ctx.finish(snapshot))
}

pub async fn fetch_key_statistics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<KeyStatistics, YfError> {
    Ok(fetch_key_statistics_with_diagnostics(
        client,
        symbol,
        cache_mode,
        retry_override,
        DataQuality::BestEffort,
    )
    .await?
    .into_data())
}

pub async fn fetch_key_statistics_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<KeyStatistics>, YfError> {
    let mut ctx = ProjectionContext::new("key_statistics", data_quality);
    let symbols = [symbol];
    let (quote_res, quote_summary_res) = tokio::join!(
        quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override),
        quotes::fetch_quote_summary_key_statistics(client, symbol, cache_mode, retry_override)
    );

    let mut results = quote_res?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    let stats = result.to_key_statistics_with_context(&mut ctx)?;
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
