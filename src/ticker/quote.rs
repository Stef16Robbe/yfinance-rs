use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
    models::{FastInfo, Quote},
    quotes,
};
use paft::fundamentals::statistics::KeyStatistics;

fn log_err<T>(res: Result<T, YfError>, name: &str, symbol: &str) -> Option<T> {
    match res {
        Ok(data) => Some(data),
        Err(e) => {
            if std::env::var("YF_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("YF_DEBUG(key_statistics): failed to fetch '{name}' for {symbol}: {e}");
            }
            None
        }
    }
}

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
    result.try_into()
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

    result.to_snapshot()
}

pub async fn fetch_key_statistics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<KeyStatistics, YfError> {
    let symbols = [symbol];
    let (quote_res, quote_summary_res) = tokio::join!(
        quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override),
        quotes::fetch_quote_summary_key_statistics(client, symbol, cache_mode, retry_override)
    );

    let mut results = quote_res?;

    let result = results.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;

    let stats = result.to_key_statistics();
    let stats = match log_err(quote_summary_res, "quote_summary_key_statistics", symbol) {
        Some(quote_summary) => quotes::merge_key_statistics(stats, &quote_summary),
        None => stats,
    };

    Ok(stats)
}
