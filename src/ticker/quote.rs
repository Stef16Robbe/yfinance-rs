use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
    models::{FastInfo, Quote},
    quotes,
};
use paft::fundamentals::statistics::KeyStatistics;

pub async fn fetch_quote(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Quote, YfError> {
    let symbols = [symbol];
    let mut results = quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override).await?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    // Use the same currency-aware conversion as the batch quotes API
    Ok(result.into())
}

pub async fn fetch_fast_info(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<FastInfo, YfError> {
    let symbols = [symbol];
    let mut results = quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override).await?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    Ok(result.to_snapshot())
}

pub async fn fetch_key_statistics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<KeyStatistics, YfError> {
    let symbols = [symbol];
    let mut results = quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override).await?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    Ok(result.to_key_statistics())
}
