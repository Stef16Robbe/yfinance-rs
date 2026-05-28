use crate::{
    DataQuality, YfClient, YfError, YfResponse, analysis,
    core::ProjectionContext,
    core::client::{CacheMode, RetryConfig},
    esg, fundamentals,
    profile::Profile,
    ticker::Info,
};
use paft::fundamentals::statistics::KeyStatistics;

/// Private helper to handle optional async results, logging errors in debug mode.
fn log_err_async<T>(
    ctx: &mut ProjectionContext,
    res: Result<T, YfError>,
    name: &'static str,
    symbol: &str,
) -> Result<Option<T>, YfError> {
    match res {
        Ok(data) => Ok(Some(data)),
        Err(e) => {
            if std::env::var("YF_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("YF_DEBUG(info): failed to fetch '{name}' for {symbol}: {e}");
            }
            ctx.suppressed_error(name, &e)?;
            Ok(None)
        }
    }
}

fn log_response_async<T>(
    ctx: &mut ProjectionContext,
    res: Result<YfResponse<T>, YfError>,
    name: &'static str,
    symbol: &str,
) -> Result<Option<T>, YfError> {
    let Some(response) = log_err_async(ctx, res, name, symbol)? else {
        return Ok(None);
    };
    ctx.extend(response.diagnostics);
    Ok(Some(response.data))
}

pub(super) async fn fetch_info(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Info, YfError> {
    Ok(fetch_info_with_diagnostics(
        client,
        symbol,
        cache_mode,
        retry_override,
        DataQuality::BestEffort,
    )
    .await?
    .into_data())
}

pub(super) async fn fetch_info_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Info>, YfError> {
    let mut ctx = ProjectionContext::new("info", data_quality);
    let (
        quote,
        quote_summary_key_statistics,
        profile,
        price_target,
        rec_summary,
        esg_summary,
        calendar,
    ) = Box::pin(fetch_info_parts(
        client,
        symbol,
        cache_mode,
        retry_override,
        data_quality,
    ))
    .await?;
    let quote_summary_key_statistics = log_err_async(
        &mut ctx,
        quote_summary_key_statistics,
        "key_statistics",
        symbol,
    )?;
    let profile = log_err_async(&mut ctx, profile, "profile", symbol)?;
    let price_target = log_response_async(&mut ctx, price_target, "price_target", symbol)?;
    let rec_summary = log_response_async(&mut ctx, rec_summary, "recommendations_summary", symbol)?;
    let esg_summary = log_response_async(&mut ctx, esg_summary, "esg_scores", symbol)?;
    let calendar = log_response_async(&mut ctx, calendar, "calendar", symbol)?;

    let key_statistics = quote_summary_key_statistics.map_or_else(
        || quote.to_key_statistics(),
        |quote_summary| {
            crate::core::quotes::merge_key_statistics(quote.to_key_statistics(), &quote_summary)
        },
    );
    let snapshot = quote.to_snapshot()?;

    Ok(ctx.finish(Info {
        snapshot,
        key_statistics,
        profile,
        calendar: calendar.or_else(|| quote.calendar_fallback()),
        price_target,
        recommendation_summary: rec_summary,
        esg_scores: esg_summary.and_then(|s| s.scores),
    }))
}

async fn fetch_info_parts(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<
    (
        crate::core::quotes::V7QuoteNode,
        Result<KeyStatistics, YfError>,
        Result<Profile, YfError>,
        Result<YfResponse<paft::fundamentals::analysis::PriceTarget>, YfError>,
        Result<YfResponse<paft::fundamentals::analysis::RecommendationSummary>, YfError>,
        Result<YfResponse<paft::fundamentals::esg::EsgSummary>, YfError>,
        Result<YfResponse<paft::fundamentals::statements::Calendar>, YfError>,
    ),
    YfError,
> {
    let symbols = [symbol];
    let calendar_builder = fundamentals::FundamentalsBuilder::new(client, symbol)
        .cache_mode(cache_mode)
        .retry_policy(retry_override.cloned())
        .data_quality(data_quality);
    let price_target_builder = analysis::AnalysisBuilder::new(client, symbol)
        .cache_mode(cache_mode)
        .retry_policy(retry_override.cloned())
        .data_quality(data_quality);
    let rec_summary_builder = analysis::AnalysisBuilder::new(client, symbol)
        .cache_mode(cache_mode)
        .retry_policy(retry_override.cloned())
        .data_quality(data_quality);
    let esg_builder = esg::EsgBuilder::new(client, symbol)
        .cache_mode(cache_mode)
        .retry_policy(retry_override.cloned())
        .data_quality(data_quality);
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
        price_target_builder.analyst_price_target_with_diagnostics(None),
        rec_summary_builder.recommendations_summary_with_diagnostics(),
        esg_builder.fetch_with_diagnostics(),
        calendar_builder.calendar_with_diagnostics()
    );

    let mut quotes = quote_res?;
    let quote = quotes.pop().ok_or_else(|| {
        YfError::MissingData(format!("no quote result found for symbol {symbol}"))
    })?;
    Ok((
        quote,
        key_statistics_res,
        profile_res,
        price_target_res,
        rec_summary_res,
        esg_res,
        calendar_res,
    ))
}
