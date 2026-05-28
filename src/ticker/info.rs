use crate::{
    YfClient, YfError, analysis,
    core::client::{CacheMode, RetryConfig},
    esg, fundamentals,
    profile::Profile,
    ticker::Info,
};
use paft::fundamentals::statistics::KeyStatistics;

/// Private helper to handle optional async results, logging errors in debug mode.
fn log_err_async<T>(res: Result<T, YfError>, name: &str, symbol: &str) -> Option<T> {
    match res {
        Ok(data) => Some(data),
        Err(e) => {
            if std::env::var("YF_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("YF_DEBUG(info): failed to fetch '{name}' for {symbol}: {e}");
            }
            None
        }
    }
}

pub(super) async fn fetch_info(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Info, YfError> {
    let (
        quote,
        quote_summary_key_statistics,
        profile,
        price_target,
        rec_summary,
        esg_summary,
        calendar,
    ) = Box::pin(fetch_info_parts(client, symbol, cache_mode, retry_override)).await?;
    let key_statistics = quote_summary_key_statistics.map_or_else(
        || quote.to_key_statistics(),
        |quote_summary| {
            crate::core::quotes::merge_key_statistics(quote.to_key_statistics(), &quote_summary)
        },
    );
    let snapshot = quote.to_snapshot()?;

    Ok(Info {
        snapshot,
        key_statistics,
        profile,
        calendar: calendar.or_else(|| quote.calendar_fallback()),
        price_target,
        recommendation_summary: rec_summary,
        esg_scores: esg_summary.and_then(|s| s.scores),
    })
}

async fn fetch_info_parts(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<
    (
        crate::core::quotes::V7QuoteNode,
        Option<KeyStatistics>,
        Option<Profile>,
        Option<paft::fundamentals::analysis::PriceTarget>,
        Option<paft::fundamentals::analysis::RecommendationSummary>,
        Option<paft::fundamentals::esg::EsgSummary>,
        Option<paft::fundamentals::statements::Calendar>,
    ),
    YfError,
> {
    let symbols = [symbol];
    let calendar_builder = fundamentals::FundamentalsBuilder::new(client, symbol)
        .cache_mode(cache_mode)
        .retry_policy(retry_override.cloned());
    let (
        quote_res,
        key_statistics_res,
        profile_res,
        price_target_res,
        rec_summary_res,
        esg_res,
        calendar_res,
    ) = tokio::join!(
        crate::core::quotes::fetch_v7_quotes(client, &symbols, cache_mode, retry_override),
        crate::core::quotes::fetch_quote_summary_key_statistics(
            client,
            symbol,
            cache_mode,
            retry_override
        ),
        crate::profile::load_profile_with_options(client, symbol, cache_mode, retry_override),
        analysis::AnalysisBuilder::new(client, symbol)
            .cache_mode(cache_mode)
            .retry_policy(retry_override.cloned())
            .analyst_price_target(None),
        analysis::AnalysisBuilder::new(client, symbol)
            .cache_mode(cache_mode)
            .retry_policy(retry_override.cloned())
            .recommendations_summary(),
        esg::EsgBuilder::new(client, symbol)
            .cache_mode(cache_mode)
            .retry_policy(retry_override.cloned())
            .fetch(),
        calendar_builder.calendar()
    );

    let mut quotes = quote_res?;
    let quote = quotes.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;
    let key_statistics = log_err_async(key_statistics_res, "key_statistics", symbol);
    let profile = log_err_async(profile_res, "profile", symbol);
    let price_target = log_err_async(price_target_res, "price_target", symbol);
    let rec_summary = log_err_async(rec_summary_res, "recommendations_summary", symbol);
    let esg_summary = log_err_async(esg_res, "esg_scores", symbol);
    let calendar = log_err_async(calendar_res, "calendar", symbol);
    Ok((
        quote,
        key_statistics,
        profile,
        price_target,
        rec_summary,
        esg_summary,
        calendar,
    ))
}
