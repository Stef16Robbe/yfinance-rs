use crate::{
    analysis::model::EarningsTrendRow,
    core::{
        DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError, YfResponse,
        client::{CacheMode, RetryConfig},
        conversions::{
            i64_to_datetime, string_to_period, string_to_recommendation_action,
            string_to_recommendation_grade,
        },
        currency_resolver::{
            AnalystEstimateCurrencyEvidence, CurrencyHints, CurrencyKind, TradingCurrencyEvidence,
        },
        diagnostics::{optional_decimal_f64, optional_money_i64, optional_price_f64},
        wire::{from_raw, from_raw_u32_round},
    },
};

use super::fetch::fetch_modules;
use super::model::{PriceTarget, RecommendationRow, RecommendationSummary, UpgradeDowngradeRow};
use paft::domain::Period;
use paft::fundamentals::analysis::{
    EarningsEstimate, EpsRevisions, EpsTrend, RecommendationAction, RecommendationGrade,
    RevenueEstimate, RevisionPoint, TrendPoint,
};
use paft::money::Currency;
// Period is available via prelude or directly; we use string_to_period for parsing, so import not needed

fn parse_optional<T>(
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, YfError>,
) -> Result<Option<T>, YfError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse)
        .transpose()
}

fn required_period(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<String>,
    field: &'static str,
    value: Option<&str>,
) -> Result<Option<Period>, YfError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
        return Ok(None);
    };
    match string_to_period(value) {
        Ok(period) => Ok(Some(period)),
        Err(err) => {
            ctx.dropped_item(
                item,
                key,
                ProjectionIssue::InvalidField {
                    field,
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
}

/* ---------- Public entry points (mapping wire → public models) ---------- */

pub(super) async fn recommendation_trend(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<RecommendationRow>>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let root = fetch_modules(
        client,
        symbol,
        "recommendationTrend",
        cache_mode,
        retry_override,
    )
    .await?;

    let trend = root
        .recommendation_trend
        .ok_or_else(|| YfError::MissingData("recommendationTrend module missing".into()))?
        .trend
        .ok_or_else(|| YfError::MissingData("recommendationTrend.trend missing".into()))?;

    let mut rows = Vec::new();
    for n in trend {
        let key = n.period.clone();
        let Some(period) = required_period(
            &mut ctx,
            "recommendation_trend",
            key,
            "period",
            n.period.as_deref(),
        )?
        else {
            continue;
        };

        rows.push(RecommendationRow {
            period,
            strong_buy: n.strong_buy.and_then(|v| u32::try_from(v).ok()),
            buy: n.buy.and_then(|v| u32::try_from(v).ok()),
            hold: n.hold.and_then(|v| u32::try_from(v).ok()),
            sell: n.sell.and_then(|v| u32::try_from(v).ok()),
            strong_sell: n.strong_sell.and_then(|v| u32::try_from(v).ok()),
        });
    }

    Ok(ctx.finish(rows))
}

pub(super) async fn recommendation_summary(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<RecommendationSummary>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let root = fetch_modules(
        client,
        symbol,
        "recommendationTrend,financialData",
        cache_mode,
        retry_override,
    )
    .await?;

    let trend = root
        .recommendation_trend
        .ok_or_else(|| YfError::MissingData("recommendationTrend module missing".into()))?
        .trend
        .ok_or_else(|| YfError::MissingData("recommendationTrend.trend missing".into()))?;

    let latest = trend.first();

    let (latest_period, sb, b, h, s, ss) = if let Some(t) = latest {
        let latest_period =
            if let Some(period) = t.period.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                match string_to_period(period) {
                    Ok(period) => Some(period),
                    Err(err) => {
                        ctx.omitted_present_field(
                            "recommendationTrend.trend[0].period",
                            t.period.clone(),
                            ProjectionIssue::InvalidField {
                                field: "period",
                                details: err.to_string(),
                            },
                        )?;
                        None
                    }
                }
            } else {
                ctx.omitted_present_field(
                    "recommendationTrend.trend[0].period",
                    None,
                    ProjectionIssue::MissingRequiredField { field: "period" },
                )?;
                None
            };
        (
            latest_period,
            t.strong_buy.and_then(|v| u32::try_from(v).ok()),
            t.buy.and_then(|v| u32::try_from(v).ok()),
            t.hold.and_then(|v| u32::try_from(v).ok()),
            t.sell.and_then(|v| u32::try_from(v).ok()),
            t.strong_sell.and_then(|v| u32::try_from(v).ok()),
        )
    } else {
        (None, None, None, None, None, None)
    };

    let (mean, _mean_key) = root.financial_data.map_or((None, None), |fd| {
        (from_raw(fd.recommendation_mean), fd.recommendation_key)
    });
    let mean = optional_decimal_f64(
        &mut ctx,
        "financialData.recommendationMean",
        Some(symbol.to_string()),
        mean,
        "recommendation mean",
    )?;

    Ok(ctx.finish(RecommendationSummary {
        latest_period,
        strong_buy: sb,
        buy: b,
        hold: h,
        sell: s,
        strong_sell: ss,
        mean,
        mean_rating_text: None,
    }))
}

#[allow(clippy::too_many_lines)]
pub(super) async fn upgrades_downgrades(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<UpgradeDowngradeRow>>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let root = fetch_modules(
        client,
        symbol,
        "upgradeDowngradeHistory",
        cache_mode,
        retry_override,
    )
    .await?;

    let hist = root
        .upgrade_downgrade_history
        .ok_or_else(|| YfError::MissingData("upgradeDowngradeHistory module missing".into()))?
        .history
        .ok_or_else(|| YfError::MissingData("upgradeDowngradeHistory.history missing".into()))?;

    let mut rows = Vec::new();
    for h in hist {
        let key = h.firm.clone();
        let Some(ts_raw) = h.epoch_grade_date else {
            ctx.dropped_item(
                "upgrade_downgrade",
                key,
                ProjectionIssue::MissingRequiredField {
                    field: "epochGradeDate",
                },
            )?;
            continue;
        };
        let ts = match i64_to_datetime(ts_raw) {
            Ok(ts) => ts,
            Err(err) => {
                ctx.dropped_item(
                    "upgrade_downgrade",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "epochGradeDate",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let from_grade = match parse_optional::<RecommendationGrade>(
            h.from_grade.as_deref(),
            string_to_recommendation_grade,
        ) {
            Ok(value) => value,
            Err(err) => {
                ctx.dropped_item(
                    "upgrade_downgrade",
                    key.clone(),
                    ProjectionIssue::InvalidField {
                        field: "fromGrade",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let to_grade = match parse_optional::<RecommendationGrade>(
            h.to_grade.as_deref(),
            string_to_recommendation_grade,
        ) {
            Ok(value) => value,
            Err(err) => {
                ctx.dropped_item(
                    "upgrade_downgrade",
                    key.clone(),
                    ProjectionIssue::InvalidField {
                        field: "toGrade",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let action = match parse_optional::<RecommendationAction>(
            h.action.or(h.grade_change).as_deref(),
            string_to_recommendation_action,
        ) {
            Ok(value) => value,
            Err(err) => {
                ctx.dropped_item(
                    "upgrade_downgrade",
                    key.clone(),
                    ProjectionIssue::InvalidField {
                        field: "action",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };

        rows.push(UpgradeDowngradeRow {
            ts,
            firm: h.firm,
            from_grade,
            to_grade,
            action,
        });
    }

    rows.sort_by_key(|r| r.ts);
    Ok(ctx.finish(rows))
}

pub(super) async fn analyst_price_target(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<PriceTarget>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let root = fetch_modules(client, symbol, "financialData", cache_mode, retry_override).await?;
    let fd = root
        .financial_data
        .ok_or_else(|| YfError::MissingData("financialData missing".into()))?;

    client
        .store_currency_hints(
            symbol,
            CurrencyHints::from_quote_summary_financial(fd.financial_currency.as_deref()),
        )
        .await;

    let mean = from_raw(fd.target_mean_price);
    let high = from_raw(fd.target_high_price);
    let low = from_raw(fd.target_low_price);
    let unit = if [mean, high, low].iter().any(Option::is_some) {
        match client
            .resolve_trading_currency_unit(
                symbol,
                override_currency,
                TradingCurrencyEvidence::None,
                cache_mode,
                retry_override,
            )
            .await
        {
            Ok(unit) => {
                ctx.currency_resolution(client, symbol, CurrencyKind::Trading)
                    .await?;
                Some(unit)
            }
            Err(err @ YfError::InvalidData(_)) => return Err(err),
            Err(err) if data_quality == DataQuality::Strict => return Err(err),
            Err(_) => None,
        }
    } else {
        None
    };

    let mean = optional_price_f64(
        &mut ctx,
        "financialData.targetMeanPrice",
        Some(symbol.to_string()),
        unit.as_ref(),
        mean,
        "analyst price value",
    )?;
    let high = optional_price_f64(
        &mut ctx,
        "financialData.targetHighPrice",
        Some(symbol.to_string()),
        unit.as_ref(),
        high,
        "analyst price value",
    )?;
    let low = optional_price_f64(
        &mut ctx,
        "financialData.targetLowPrice",
        Some(symbol.to_string()),
        unit.as_ref(),
        low,
        "analyst price value",
    )?;

    Ok(ctx.finish(PriceTarget {
        mean,
        high,
        low,
        number_of_analysts: from_raw_u32_round(fd.number_of_analyst_opinions),
    }))
}

#[allow(clippy::too_many_lines)]
pub(super) async fn earnings_trend(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<EarningsTrendRow>>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let root = fetch_modules(client, symbol, "earningsTrend", cache_mode, retry_override).await?;

    let trend = root
        .earnings_trend
        .ok_or_else(|| YfError::MissingData("earningsTrend module missing".into()))?
        .trend
        .ok_or_else(|| YfError::MissingData("earningsTrend.trend missing".into()))?;

    let mut rows = Vec::with_capacity(trend.len());
    for n in trend {
        let earnings_currency = n
            .earnings_estimate
            .as_ref()
            .and_then(|estimate| estimate.earnings_currency.clone());
        let revenue_currency = n
            .revenue_estimate
            .as_ref()
            .and_then(|estimate| estimate.revenue_currency.clone());
        let eps_currency = n
            .eps_trend
            .as_ref()
            .and_then(|trend| trend.eps_trend_currency.clone());

        let (
            earnings_estimate_avg,
            earnings_estimate_low,
            earnings_estimate_high,
            earnings_estimate_year_ago_eps,
            earnings_estimate_num_analysts,
            earnings_estimate_growth,
        ) = n
            .earnings_estimate
            .map(|e| {
                (
                    from_raw(e.avg),
                    from_raw(e.low),
                    from_raw(e.high),
                    from_raw(e.year_ago_eps),
                    from_raw_u32_round(e.num_analysts),
                    from_raw(e.growth),
                )
            })
            .unwrap_or_default();

        let (
            revenue_estimate_avg,
            revenue_estimate_low,
            revenue_estimate_high,
            revenue_estimate_year_ago_revenue,
            revenue_estimate_num_analysts,
            revenue_estimate_growth,
        ) = n
            .revenue_estimate
            .map(|e| {
                (
                    from_raw(e.avg),
                    from_raw(e.low),
                    from_raw(e.high),
                    from_raw(e.year_ago_revenue),
                    from_raw_u32_round(e.num_analysts),
                    from_raw(e.growth),
                )
            })
            .unwrap_or_default();

        let (
            eps_trend_current,
            eps_trend_7_days_ago,
            eps_trend_30_days_ago,
            eps_trend_60_days_ago,
            eps_trend_90_days_ago,
        ) = n
            .eps_trend
            .map(|e| {
                (
                    from_raw(e.current),
                    from_raw(e.seven_days_ago),
                    from_raw(e.thirty_days_ago),
                    from_raw(e.sixty_days_ago),
                    from_raw(e.ninety_days_ago),
                )
            })
            .unwrap_or_default();

        let (
            eps_revisions_up_last_7_days,
            eps_revisions_up_last_30_days,
            eps_revisions_down_last_7_days,
            eps_revisions_down_last_30_days,
        ) = n
            .eps_revisions
            .map(|e| {
                (
                    from_raw_u32_round(e.up_last_7_days),
                    from_raw_u32_round(e.up_last_30_days),
                    from_raw_u32_round(e.down_last_7_days),
                    from_raw_u32_round(e.down_last_30_days),
                )
            })
            .unwrap_or_default();

        let earnings_unit = if [
            earnings_estimate_avg,
            earnings_estimate_low,
            earnings_estimate_high,
            earnings_estimate_year_ago_eps,
        ]
        .iter()
        .any(Option::is_some)
        {
            match client
                .resolve_analyst_estimate_currency_unit(
                    symbol,
                    override_currency.clone(),
                    AnalystEstimateCurrencyEvidence::Earnings(earnings_currency.as_deref()),
                    cache_mode,
                    retry_override,
                )
                .await
            {
                Ok(unit) => Some(unit),
                Err(err @ YfError::InvalidData(_)) => return Err(err),
                Err(err) if data_quality == DataQuality::Strict => return Err(err),
                Err(_) => None,
            }
        } else {
            None
        };

        let revenue_unit = if [
            revenue_estimate_avg,
            revenue_estimate_low,
            revenue_estimate_high,
            revenue_estimate_year_ago_revenue,
        ]
        .iter()
        .any(Option::is_some)
        {
            match client
                .resolve_analyst_estimate_currency_unit(
                    symbol,
                    override_currency.clone(),
                    AnalystEstimateCurrencyEvidence::Revenue(revenue_currency.as_deref()),
                    cache_mode,
                    retry_override,
                )
                .await
            {
                Ok(unit) => Some(unit),
                Err(err @ YfError::InvalidData(_)) => return Err(err),
                Err(err) if data_quality == DataQuality::Strict => return Err(err),
                Err(_) => None,
            }
        } else {
            None
        };

        let eps_unit = if [
            eps_trend_current,
            eps_trend_7_days_ago,
            eps_trend_30_days_ago,
            eps_trend_60_days_ago,
            eps_trend_90_days_ago,
        ]
        .iter()
        .any(Option::is_some)
        {
            match client
                .resolve_analyst_estimate_currency_unit(
                    symbol,
                    override_currency.clone(),
                    AnalystEstimateCurrencyEvidence::EpsTrend(eps_currency.as_deref()),
                    cache_mode,
                    retry_override,
                )
                .await
            {
                Ok(unit) => Some(unit),
                Err(err @ YfError::InvalidData(_)) => return Err(err),
                Err(err) if data_quality == DataQuality::Strict => return Err(err),
                Err(_) => None,
            }
        } else {
            None
        };

        let Some(period) = required_period(
            &mut ctx,
            "earnings_trend",
            n.period.clone(),
            "period",
            n.period.as_deref(),
        )?
        else {
            continue;
        };

        rows.push(EarningsTrendRow {
            period,
            growth: optional_decimal_f64(
                &mut ctx,
                "earningsTrend[].growth",
                n.period.clone(),
                from_raw(n.growth),
                "earnings trend growth",
            )?,
            earnings_estimate: EarningsEstimate {
                avg: optional_price_f64(
                    &mut ctx,
                    "earningsTrend[].earningsEstimate.avg",
                    n.period.clone(),
                    earnings_unit.as_ref(),
                    earnings_estimate_avg,
                    "analyst price value",
                )?,
                low: optional_price_f64(
                    &mut ctx,
                    "earningsTrend[].earningsEstimate.low",
                    n.period.clone(),
                    earnings_unit.as_ref(),
                    earnings_estimate_low,
                    "analyst price value",
                )?,
                high: optional_price_f64(
                    &mut ctx,
                    "earningsTrend[].earningsEstimate.high",
                    n.period.clone(),
                    earnings_unit.as_ref(),
                    earnings_estimate_high,
                    "analyst price value",
                )?,
                year_ago_eps: optional_price_f64(
                    &mut ctx,
                    "earningsTrend[].earningsEstimate.yearAgoEps",
                    n.period.clone(),
                    earnings_unit.as_ref(),
                    earnings_estimate_year_ago_eps,
                    "analyst price value",
                )?,
                num_analysts: earnings_estimate_num_analysts,
                growth: optional_decimal_f64(
                    &mut ctx,
                    "earningsTrend[].earningsEstimate.growth",
                    n.period.clone(),
                    earnings_estimate_growth,
                    "earnings estimate growth",
                )?,
            },
            revenue_estimate: RevenueEstimate {
                avg: optional_money_i64(
                    &mut ctx,
                    "earningsTrend[].revenueEstimate.avg",
                    n.period.clone(),
                    revenue_unit.as_ref(),
                    revenue_estimate_avg,
                    "analyst monetary value",
                )?,
                low: optional_money_i64(
                    &mut ctx,
                    "earningsTrend[].revenueEstimate.low",
                    n.period.clone(),
                    revenue_unit.as_ref(),
                    revenue_estimate_low,
                    "analyst monetary value",
                )?,
                high: optional_money_i64(
                    &mut ctx,
                    "earningsTrend[].revenueEstimate.high",
                    n.period.clone(),
                    revenue_unit.as_ref(),
                    revenue_estimate_high,
                    "analyst monetary value",
                )?,
                year_ago_revenue: optional_money_i64(
                    &mut ctx,
                    "earningsTrend[].revenueEstimate.yearAgoRevenue",
                    n.period.clone(),
                    revenue_unit.as_ref(),
                    revenue_estimate_year_ago_revenue,
                    "analyst monetary value",
                )?,
                num_analysts: revenue_estimate_num_analysts,
                growth: optional_decimal_f64(
                    &mut ctx,
                    "earningsTrend[].revenueEstimate.growth",
                    n.period.clone(),
                    revenue_estimate_growth,
                    "revenue estimate growth",
                )?,
            },
            eps_trend: EpsTrend {
                current: optional_price_f64(
                    &mut ctx,
                    "earningsTrend[].epsTrend.current",
                    n.period.clone(),
                    eps_unit.as_ref(),
                    eps_trend_current,
                    "analyst price value",
                )?,
                historical: {
                    let mut hist = Vec::new();
                    if let Some(v) = optional_price_f64(
                        &mut ctx,
                        "earningsTrend[].epsTrend.7daysAgo",
                        n.period.clone(),
                        eps_unit.as_ref(),
                        eps_trend_7_days_ago,
                        "analyst price value",
                    )? && let Ok(tp) = TrendPoint::try_new_str("7d", v)
                    {
                        hist.push(tp);
                    }
                    if let Some(v) = optional_price_f64(
                        &mut ctx,
                        "earningsTrend[].epsTrend.30daysAgo",
                        n.period.clone(),
                        eps_unit.as_ref(),
                        eps_trend_30_days_ago,
                        "analyst price value",
                    )? && let Ok(tp) = TrendPoint::try_new_str("30d", v)
                    {
                        hist.push(tp);
                    }
                    if let Some(v) = optional_price_f64(
                        &mut ctx,
                        "earningsTrend[].epsTrend.60daysAgo",
                        n.period.clone(),
                        eps_unit.as_ref(),
                        eps_trend_60_days_ago,
                        "analyst price value",
                    )? && let Ok(tp) = TrendPoint::try_new_str("60d", v)
                    {
                        hist.push(tp);
                    }
                    if let Some(v) = optional_price_f64(
                        &mut ctx,
                        "earningsTrend[].epsTrend.90daysAgo",
                        n.period.clone(),
                        eps_unit.as_ref(),
                        eps_trend_90_days_ago,
                        "analyst price value",
                    )? && let Ok(tp) = TrendPoint::try_new_str("90d", v)
                    {
                        hist.push(tp);
                    }
                    hist
                },
            },
            eps_revisions: EpsRevisions {
                historical: {
                    let mut hist = Vec::new();
                    if let (Some(up), Some(down)) =
                        (eps_revisions_up_last_7_days, eps_revisions_down_last_7_days)
                        && let Ok(rp) = RevisionPoint::try_new_str("7d", up, down)
                    {
                        hist.push(rp);
                    }
                    if let (Some(up), Some(down)) = (
                        eps_revisions_up_last_30_days,
                        eps_revisions_down_last_30_days,
                    ) && let Ok(rp) = RevisionPoint::try_new_str("30d", up, down)
                    {
                        hist.push(rp);
                    }
                    hist
                },
            },
        });
    }

    Ok(ctx.finish(rows))
}
