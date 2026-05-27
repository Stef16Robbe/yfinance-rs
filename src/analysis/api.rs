use crate::{
    analysis::model::EarningsTrendRow,
    core::{
        YfClient, YfError,
        client::{CacheMode, RetryConfig},
        conversions::{
            decimal_from_f64, i64_to_datetime, i64_to_money_with_currency, price_from_f64,
            string_to_period, string_to_recommendation_action, string_to_recommendation_grade,
        },
        wire::{from_raw, from_raw_u32_round},
    },
};

use super::fetch::fetch_modules;
use super::model::{PriceTarget, RecommendationRow, RecommendationSummary, UpgradeDowngradeRow};
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

/* ---------- Public entry points (mapping wire → public models) ---------- */

pub(super) async fn recommendation_trend(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<RecommendationRow>, YfError> {
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

    let rows = trend
        .into_iter()
        .map(|n| {
            Ok(RecommendationRow {
                period: string_to_period(n.period.as_deref().unwrap_or(""))?,
                strong_buy: n.strong_buy.and_then(|v| u32::try_from(v).ok()),
                buy: n.buy.and_then(|v| u32::try_from(v).ok()),
                hold: n.hold.and_then(|v| u32::try_from(v).ok()),
                sell: n.sell.and_then(|v| u32::try_from(v).ok()),
                strong_sell: n.strong_sell.and_then(|v| u32::try_from(v).ok()),
            })
        })
        .collect::<Result<Vec<_>, YfError>>()?;

    Ok(rows)
}

pub(super) async fn recommendation_summary(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<RecommendationSummary, YfError> {
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
        (
            Some(string_to_period(t.period.as_deref().unwrap_or(""))?),
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

    Ok(RecommendationSummary {
        latest_period,
        strong_buy: sb,
        buy: b,
        hold: h,
        sell: s,
        strong_sell: ss,
        mean: mean.and_then(decimal_from_f64),
        mean_rating_text: None,
    })
}

pub(super) async fn upgrades_downgrades(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<UpgradeDowngradeRow>, YfError> {
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

    let mut rows: Vec<UpgradeDowngradeRow> = hist
        .into_iter()
        .filter_map(|h| {
            let ts = h.epoch_grade_date.and_then(|ts| i64_to_datetime(ts).ok())?;
            let from_grade = match parse_optional::<RecommendationGrade>(
                h.from_grade.as_deref(),
                string_to_recommendation_grade,
            ) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let to_grade = match parse_optional::<RecommendationGrade>(
                h.to_grade.as_deref(),
                string_to_recommendation_grade,
            ) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let action = match parse_optional::<RecommendationAction>(
                h.action.or(h.grade_change).as_deref(),
                string_to_recommendation_action,
            ) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };

            Some(Ok(UpgradeDowngradeRow {
                ts,
                firm: h.firm,
                from_grade,
                to_grade,
                action,
            }))
        })
        .collect::<Result<Vec<_>, YfError>>()?;

    rows.sort_by_key(|r| r.ts);
    Ok(rows)
}

pub(super) async fn analyst_price_target(
    client: &YfClient,
    symbol: &str,
    currency: Currency,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<PriceTarget, YfError> {
    let root = fetch_modules(client, symbol, "financialData", cache_mode, retry_override).await?;
    let fd = root
        .financial_data
        .ok_or_else(|| YfError::MissingData("financialData missing".into()))?;

    Ok(PriceTarget {
        mean: from_raw(fd.target_mean_price).and_then(|v| price_from_f64(v, currency.clone())),
        high: from_raw(fd.target_high_price).and_then(|v| price_from_f64(v, currency.clone())),
        low: from_raw(fd.target_low_price).and_then(|v| price_from_f64(v, currency.clone())),
        number_of_analysts: from_raw_u32_round(fd.number_of_analyst_opinions),
    })
}

#[allow(clippy::too_many_lines)]
pub(super) async fn earnings_trend(
    client: &YfClient,
    symbol: &str,
    currency: Currency,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<EarningsTrendRow>, YfError> {
    let root = fetch_modules(client, symbol, "earningsTrend", cache_mode, retry_override).await?;

    let trend = root
        .earnings_trend
        .ok_or_else(|| YfError::MissingData("earningsTrend module missing".into()))?
        .trend
        .ok_or_else(|| YfError::MissingData("earningsTrend.trend missing".into()))?;

    let rows = trend
        .into_iter()
        .map(|n| -> Result<EarningsTrendRow, YfError> {
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

            Ok(EarningsTrendRow {
                period: string_to_period(n.period.as_deref().unwrap_or(""))?,
                growth: from_raw(n.growth).and_then(decimal_from_f64),
                earnings_estimate: EarningsEstimate {
                    avg: earnings_estimate_avg.and_then(|v| price_from_f64(v, currency.clone())),
                    low: earnings_estimate_low.and_then(|v| price_from_f64(v, currency.clone())),
                    high: earnings_estimate_high.and_then(|v| price_from_f64(v, currency.clone())),
                    year_ago_eps: earnings_estimate_year_ago_eps
                        .and_then(|v| price_from_f64(v, currency.clone())),
                    num_analysts: earnings_estimate_num_analysts,
                    growth: earnings_estimate_growth.and_then(decimal_from_f64),
                },
                revenue_estimate: RevenueEstimate {
                    avg: revenue_estimate_avg
                        .map(|v| i64_to_money_with_currency(v, currency.clone()))
                        .transpose()?,
                    low: revenue_estimate_low
                        .map(|v| i64_to_money_with_currency(v, currency.clone()))
                        .transpose()?,
                    high: revenue_estimate_high
                        .map(|v| i64_to_money_with_currency(v, currency.clone()))
                        .transpose()?,
                    year_ago_revenue: revenue_estimate_year_ago_revenue
                        .map(|v| i64_to_money_with_currency(v, currency.clone()))
                        .transpose()?,
                    num_analysts: revenue_estimate_num_analysts,
                    growth: revenue_estimate_growth.and_then(decimal_from_f64),
                },
                eps_trend: EpsTrend {
                    current: eps_trend_current.and_then(|v| price_from_f64(v, currency.clone())),
                    historical: {
                        let mut hist = Vec::new();
                        if let Some(v) =
                            eps_trend_7_days_ago.and_then(|v| price_from_f64(v, currency.clone()))
                            && let Ok(tp) = TrendPoint::try_new_str("7d", v)
                        {
                            hist.push(tp);
                        }
                        if let Some(v) =
                            eps_trend_30_days_ago.and_then(|v| price_from_f64(v, currency.clone()))
                            && let Ok(tp) = TrendPoint::try_new_str("30d", v)
                        {
                            hist.push(tp);
                        }
                        if let Some(v) =
                            eps_trend_60_days_ago.and_then(|v| price_from_f64(v, currency.clone()))
                            && let Ok(tp) = TrendPoint::try_new_str("60d", v)
                        {
                            hist.push(tp);
                        }
                        if let Some(v) =
                            eps_trend_90_days_ago.and_then(|v| price_from_f64(v, currency.clone()))
                            && let Ok(tp) = TrendPoint::try_new_str("90d", v)
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
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}
