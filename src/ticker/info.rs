use crate::{
    YfClient, YfError, YfResponse, analysis,
    core::client::normalize_symbol,
    core::{CallOptions, ProjectionContext, YfDiagnostics, YfWarning, quotesummary},
    fundamentals,
    profile::Profile,
    ticker::Info,
};

const INFO_QUOTE_SUMMARY_MODULES: &str = "summaryDetail,defaultKeyStatistics,assetProfile,quoteType,fundProfile,financialData,recommendationTrend,calendarEvents";

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
            crate::core::logging::trace_debug!(
                symbol,
                module = name,
                error = %e,
                "optional info module fetch failed"
            );
            #[cfg(not(feature = "tracing"))]
            let _ = symbol;
            ctx.suppressed_error(name, &e)?;
            Ok(None)
        }
    }
}

fn log_optional_response_async<T>(
    ctx: &mut ProjectionContext,
    res: Result<YfResponse<T>, YfError>,
    name: &'static str,
    symbol: &str,
    unavailable_features: &[&'static str],
) -> Result<Option<T>, YfError> {
    let Some(response) = log_err_async(ctx, res, name, symbol)? else {
        return Ok(None);
    };
    let YfResponse { data, diagnostics } = response;
    let unavailable = diagnostics_has_unavailable_feature(&diagnostics, unavailable_features);
    ctx.extend(diagnostics);

    if unavailable {
        Ok(None)
    } else {
        Ok(Some(data))
    }
}

fn diagnostics_has_unavailable_feature(
    diagnostics: &YfDiagnostics,
    unavailable_features: &[&'static str],
) -> bool {
    diagnostics.warnings.iter().any(|warning| {
        let YfWarning::ProviderFeatureUnavailable { feature, .. } = warning else {
            return false;
        };
        unavailable_features.iter().any(|unavailable| {
            *feature == *unavailable
                || feature
                    .strip_prefix(unavailable)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        })
    })
}

pub(super) async fn fetch_info(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<Info, YfError> {
    Ok(fetch_info_with_diagnostics(client, symbol, options)
        .await?
        .into_data())
}

pub(super) async fn fetch_info_with_diagnostics(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<YfResponse<Info>, YfError> {
    let symbol = normalize_symbol(symbol)?;
    let mut ctx = ProjectionContext::new("info", options.data_quality());
    let (quote, quote_summary_parts) = fetch_info_parts(client, &symbol, options, &mut ctx).await?;
    let quote_summary = project_info_quote_summary_parts(&mut ctx, &symbol, quote_summary_parts)?;

    let key_statistics = quote.to_key_statistics_with_context(&mut ctx)?;
    let key_statistics = match quote_summary.key_statistics {
        Some(quote_summary) => {
            crate::core::quotes::merge_key_statistics(key_statistics, &quote_summary)
        }
        None => key_statistics,
    };
    let moving_averages = quote.to_moving_averages_with_context(&mut ctx)?;
    let moving_averages = match quote_summary.moving_averages {
        Some(quote_summary) => {
            crate::core::quotes::merge_moving_averages(moving_averages, &quote_summary)
        }
        None => moving_averages,
    };
    let snapshot = quote.to_snapshot_with_context(&mut ctx)?;
    let calendar = match quote_summary.calendar {
        Some(calendar) => Some(calendar),
        None => quote.calendar_fallback_with_context(&mut ctx)?,
    };

    Ok(ctx.finish(Info {
        snapshot,
        moving_averages,
        key_statistics,
        profile: quote_summary.profile,
        calendar,
        price_target: quote_summary.price_target,
        recommendation_summary: quote_summary.recommendation_summary,
    }))
}

struct InfoQuoteSummaryParts {
    key_statistics: Result<crate::core::quotes::QuoteSummaryKeyStatistics, YfError>,
    profile: Result<YfResponse<Profile>, YfError>,
    analysis: Result<analysis::InfoAnalysisParts, YfError>,
    calendar: Result<YfResponse<paft::fundamentals::statements::Calendar>, YfError>,
}

#[derive(Default)]
struct ProjectedInfoQuoteSummaryParts {
    key_statistics: Option<paft::fundamentals::statistics::KeyStatistics>,
    moving_averages: Option<crate::core::MovingAverages>,
    profile: Option<Profile>,
    price_target: Option<paft::fundamentals::analysis::PriceTarget>,
    recommendation_summary: Option<paft::fundamentals::analysis::RecommendationSummary>,
    calendar: Option<paft::fundamentals::statements::Calendar>,
}

fn project_info_quote_summary_parts(
    ctx: &mut ProjectionContext,
    symbol: &str,
    quote_summary_parts: Result<InfoQuoteSummaryParts, YfError>,
) -> Result<ProjectedInfoQuoteSummaryParts, YfError> {
    let parts = match quote_summary_parts {
        Ok(parts) => parts,
        Err(err) => {
            crate::core::logging::trace_debug!(
                symbol,
                module = "quote_summary",
                error = %err,
                "optional info module fetch failed"
            );
            ctx.suppressed_error("quote_summary", &err)?;
            return Ok(ProjectedInfoQuoteSummaryParts::default());
        }
    };

    let (key_statistics, moving_averages) =
        match log_err_async(ctx, parts.key_statistics, "key_statistics", symbol)? {
            Some(key_statistics) => {
                let (key_statistics, moving_averages) = key_statistics
                    .into_key_statistics_and_moving_averages_with_context(ctx, symbol)?;
                (Some(key_statistics), Some(moving_averages))
            }
            None => (None, None),
        };
    let profile =
        log_optional_response_async(ctx, parts.profile, "profile", symbol, &["assetProfile"])?;
    let (price_target, recommendation_summary) = match parts.analysis {
        Ok(analysis) => {
            let price_target = log_optional_response_async(
                ctx,
                analysis.price_target,
                "price_target",
                symbol,
                &["financialData"],
            )?;
            let recommendation_summary = log_optional_response_async(
                ctx,
                analysis.recommendation_summary,
                "recommendations_summary",
                symbol,
                &["recommendationTrend"],
            )?;
            (price_target, recommendation_summary)
        }
        Err(err) => {
            crate::core::logging::trace_debug!(
                symbol,
                module = "analysis",
                error = %err,
                "optional info module fetch failed"
            );
            ctx.suppressed_error("analysis", &err)?;
            (None, None)
        }
    };
    let calendar =
        log_optional_response_async(ctx, parts.calendar, "calendar", symbol, &["calendarEvents"])?;

    Ok(ProjectedInfoQuoteSummaryParts {
        key_statistics,
        moving_averages,
        profile,
        price_target,
        recommendation_summary,
        calendar,
    })
}

async fn fetch_info_parts(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
    ctx: &mut ProjectionContext,
) -> Result<
    (
        crate::core::quotes::V7QuoteNode,
        Result<InfoQuoteSummaryParts, YfError>,
    ),
    YfError,
> {
    let symbols = [symbol];
    let (quote_res, quote_summary_res) = tokio::join!(
        crate::core::quotes::fetch_v7_quote_values(client, &symbols, options),
        fetch_info_quote_summary_parts(client, symbol, options)
    );

    let quote =
        crate::core::quotes::required_quote_node_from_values_with_context(quote_res?, symbol, ctx)?;
    Ok((quote, quote_summary_res))
}

async fn fetch_info_quote_summary_parts(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<InfoQuoteSummaryParts, YfError> {
    let value = quotesummary::fetch_module_value(
        client,
        symbol,
        INFO_QUOTE_SUMMARY_MODULES,
        "info",
        options,
    )
    .await?;

    Ok(InfoQuoteSummaryParts {
        key_statistics: crate::core::quotes::quote_summary_key_statistics_from_value(value.clone()),
        profile: crate::profile::load_profile_from_quote_summary_value(
            client,
            symbol,
            value.clone(),
            options,
        ),
        analysis: analysis::price_target_and_recommendation_summary_from_quote_summary_value(
            client,
            symbol,
            None,
            value.clone(),
            options,
        )
        .await,
        calendar: fundamentals::calendar_from_quote_summary_value(value, options.data_quality()),
    })
}
