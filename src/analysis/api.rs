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
            AnalystEstimateCurrencyEvidence, CurrencyHints, CurrencyKind, ResolvedCurrencyUnit,
            TradingCurrencyEvidence,
        },
        diagnostics::{
            optional_decimal_f64, optional_money_i64, optional_price_f64, optional_u32_from_i64,
            optional_u32_from_raw_f64,
        },
        wire::{RawNum, from_raw},
    },
};

use super::fetch::fetch_modules;
use super::model::{PriceTarget, RecommendationRow, RecommendationSummary, UpgradeDowngradeRow};
use super::wire::{
    EarningsEstimateNode, EarningsTrendItemNode, EpsRevisionsNode, EpsTrendNode,
    RecommendationNode, RevenueEstimateNode,
};
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

#[derive(Clone, Copy)]
struct RecommendationCountPaths {
    strong_buy: &'static str,
    buy: &'static str,
    hold: &'static str,
    sell: &'static str,
    strong_sell: &'static str,
}

type RecommendationCounts = (
    Option<u32>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
);

const TREND_COUNT_PATHS: RecommendationCountPaths = RecommendationCountPaths {
    strong_buy: "recommendationTrend.trend[].strongBuy",
    buy: "recommendationTrend.trend[].buy",
    hold: "recommendationTrend.trend[].hold",
    sell: "recommendationTrend.trend[].sell",
    strong_sell: "recommendationTrend.trend[].strongSell",
};

const LATEST_COUNT_PATHS: RecommendationCountPaths = RecommendationCountPaths {
    strong_buy: "recommendationTrend.trend[0].strongBuy",
    buy: "recommendationTrend.trend[0].buy",
    hold: "recommendationTrend.trend[0].hold",
    sell: "recommendationTrend.trend[0].sell",
    strong_sell: "recommendationTrend.trend[0].strongSell",
};

fn recommendation_counts(
    ctx: &mut ProjectionContext,
    paths: RecommendationCountPaths,
    key: Option<String>,
    node: &RecommendationNode,
) -> Result<RecommendationCounts, YfError> {
    Ok((
        optional_u32_from_i64(
            ctx,
            paths.strong_buy,
            key.clone(),
            "strongBuy",
            node.strong_buy,
        )?,
        optional_u32_from_i64(ctx, paths.buy, key.clone(), "buy", node.buy)?,
        optional_u32_from_i64(ctx, paths.hold, key.clone(), "hold", node.hold)?,
        optional_u32_from_i64(ctx, paths.sell, key.clone(), "sell", node.sell)?,
        optional_u32_from_i64(ctx, paths.strong_sell, key, "strongSell", node.strong_sell)?,
    ))
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

    let Some(recommendation_trend) = root.recommendation_trend else {
        ctx.unavailable_feature("recommendationTrend")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(trend) = recommendation_trend.trend else {
        ctx.unavailable_feature("recommendationTrend.trend")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let mut rows = Vec::new();
    for n in trend {
        let key = n.period.clone();
        let Some(period) = required_period(
            &mut ctx,
            "recommendation_trend",
            key.clone(),
            "period",
            n.period.as_deref(),
        )?
        else {
            continue;
        };

        let (strong_buy, buy, hold, sell, strong_sell) =
            recommendation_counts(&mut ctx, TREND_COUNT_PATHS, key, &n)?;

        rows.push(RecommendationRow {
            period,
            strong_buy,
            buy,
            hold,
            sell,
            strong_sell,
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

    let trend = if let Some(recommendation_trend) = root.recommendation_trend {
        if let Some(trend) = recommendation_trend.trend {
            trend
        } else {
            ctx.unavailable_feature("recommendationTrend.trend")?;
            Vec::new()
        }
    } else {
        ctx.unavailable_feature("recommendationTrend")?;
        Vec::new()
    };

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
        let (sb, b, h, s, ss) =
            recommendation_counts(&mut ctx, LATEST_COUNT_PATHS, t.period.clone(), t)?;
        (latest_period, sb, b, h, s, ss)
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

    let Some(upgrade_downgrade_history) = root.upgrade_downgrade_history else {
        ctx.unavailable_feature("upgradeDowngradeHistory")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(hist) = upgrade_downgrade_history.history else {
        ctx.unavailable_feature("upgradeDowngradeHistory.history")?;
        return Ok(ctx.finish(Vec::new()));
    };

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
    let Some(fd) = root.financial_data else {
        ctx.unavailable_feature("financialData")?;
        return Ok(ctx.finish(PriceTarget::default()));
    };

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

    let number_of_analysts = optional_u32_from_raw_f64(
        &mut ctx,
        "financialData.numberOfAnalystOpinions",
        Some(symbol.to_string()),
        "numberOfAnalystOpinions",
        fd.number_of_analyst_opinions,
    )?;

    Ok(ctx.finish(PriceTarget {
        mean,
        high,
        low,
        number_of_analysts,
    }))
}

struct AnalystCurrencyResolver<'a> {
    client: &'a YfClient,
    symbol: &'a str,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&'a RetryConfig>,
    data_quality: DataQuality,
}

impl AnalystCurrencyResolver<'_> {
    async fn resolve_if_any<T: Sync>(
        &self,
        values: &[Option<T>],
        evidence: AnalystEstimateCurrencyEvidence<'_>,
    ) -> Result<Option<ResolvedCurrencyUnit>, YfError> {
        if !values.iter().any(Option::is_some) {
            return Ok(None);
        }

        match self
            .client
            .resolve_analyst_estimate_currency_unit(
                self.symbol,
                self.override_currency.clone(),
                evidence,
                self.cache_mode,
                self.retry_override,
            )
            .await
        {
            Ok(unit) => Ok(Some(unit)),
            Err(err @ YfError::InvalidData(_)) => Err(err),
            Err(err) if self.data_quality == DataQuality::Strict => Err(err),
            Err(_) => Ok(None),
        }
    }
}

#[derive(Default)]
struct RawEarningsEstimate {
    currency: Option<String>,
    avg: Option<f64>,
    low: Option<f64>,
    high: Option<f64>,
    year_ago_eps: Option<f64>,
    num_analysts: Option<RawNum<f64>>,
    growth: Option<f64>,
}

impl RawEarningsEstimate {
    const fn price_values(&self) -> [Option<f64>; 4] {
        [self.avg, self.low, self.high, self.year_ago_eps]
    }
}

impl From<Option<EarningsEstimateNode>> for RawEarningsEstimate {
    fn from(node: Option<EarningsEstimateNode>) -> Self {
        let Some(node) = node else {
            return Self::default();
        };

        Self {
            currency: node.earnings_currency,
            avg: from_raw(node.avg),
            low: from_raw(node.low),
            high: from_raw(node.high),
            year_ago_eps: from_raw(node.year_ago_eps),
            num_analysts: node.num_analysts,
            growth: from_raw(node.growth),
        }
    }
}

#[derive(Default)]
struct RawRevenueEstimate {
    currency: Option<String>,
    avg: Option<i64>,
    low: Option<i64>,
    high: Option<i64>,
    year_ago_revenue: Option<i64>,
    num_analysts: Option<RawNum<f64>>,
    growth: Option<f64>,
}

impl RawRevenueEstimate {
    const fn money_values(&self) -> [Option<i64>; 4] {
        [self.avg, self.low, self.high, self.year_ago_revenue]
    }
}

impl From<Option<RevenueEstimateNode>> for RawRevenueEstimate {
    fn from(node: Option<RevenueEstimateNode>) -> Self {
        let Some(node) = node else {
            return Self::default();
        };

        Self {
            currency: node.revenue_currency,
            avg: from_raw(node.avg),
            low: from_raw(node.low),
            high: from_raw(node.high),
            year_ago_revenue: from_raw(node.year_ago_revenue),
            num_analysts: node.num_analysts,
            growth: from_raw(node.growth),
        }
    }
}

#[derive(Default)]
struct RawEpsTrend {
    currency: Option<String>,
    current: Option<f64>,
    seven_days_ago: Option<f64>,
    thirty_days_ago: Option<f64>,
    sixty_days_ago: Option<f64>,
    ninety_days_ago: Option<f64>,
}

impl RawEpsTrend {
    const fn price_values(&self) -> [Option<f64>; 5] {
        [
            self.current,
            self.seven_days_ago,
            self.thirty_days_ago,
            self.sixty_days_ago,
            self.ninety_days_ago,
        ]
    }
}

impl From<Option<EpsTrendNode>> for RawEpsTrend {
    fn from(node: Option<EpsTrendNode>) -> Self {
        let Some(node) = node else {
            return Self::default();
        };

        Self {
            currency: node.eps_trend_currency,
            current: from_raw(node.current),
            seven_days_ago: from_raw(node.seven_days_ago),
            thirty_days_ago: from_raw(node.thirty_days_ago),
            sixty_days_ago: from_raw(node.sixty_days_ago),
            ninety_days_ago: from_raw(node.ninety_days_ago),
        }
    }
}

#[derive(Default)]
struct RawEpsRevisions {
    up_7d: Option<RawNum<f64>>,
    up_30d: Option<RawNum<f64>>,
    down_7d: Option<RawNum<f64>>,
    down_30d: Option<RawNum<f64>>,
}

impl From<Option<EpsRevisionsNode>> for RawEpsRevisions {
    fn from(node: Option<EpsRevisionsNode>) -> Self {
        let Some(node) = node else {
            return Self::default();
        };

        Self {
            up_7d: node.up_last_7_days,
            up_30d: node.up_last_30_days,
            down_7d: node.down_last_7_days,
            down_30d: node.down_last_30_days,
        }
    }
}

fn diagnostic_key(key: Option<&str>) -> Option<String> {
    key.map(str::to_owned)
}

struct RawEarningsTrendItem {
    period_key: Option<String>,
    growth: Option<f64>,
    earnings: RawEarningsEstimate,
    revenue: RawRevenueEstimate,
    eps_trend: RawEpsTrend,
    eps_revisions: RawEpsRevisions,
}

impl From<EarningsTrendItemNode> for RawEarningsTrendItem {
    fn from(node: EarningsTrendItemNode) -> Self {
        Self {
            period_key: node.period,
            growth: from_raw(node.growth),
            earnings: node.earnings_estimate.into(),
            revenue: node.revenue_estimate.into(),
            eps_trend: node.eps_trend.into(),
            eps_revisions: node.eps_revisions.into(),
        }
    }
}

#[derive(Clone, Copy)]
struct EarningsTrendUnits<'a> {
    earnings: Option<&'a ResolvedCurrencyUnit>,
    revenue: Option<&'a ResolvedCurrencyUnit>,
    eps: Option<&'a ResolvedCurrencyUnit>,
}

fn project_earnings_estimate(
    ctx: &mut ProjectionContext,
    period_key: Option<&str>,
    raw: &RawEarningsEstimate,
    unit: Option<&ResolvedCurrencyUnit>,
) -> Result<EarningsEstimate, YfError> {
    Ok(EarningsEstimate {
        avg: optional_price_f64(
            ctx,
            "earningsTrend[].earningsEstimate.avg",
            diagnostic_key(period_key),
            unit,
            raw.avg,
            "analyst price value",
        )?,
        low: optional_price_f64(
            ctx,
            "earningsTrend[].earningsEstimate.low",
            diagnostic_key(period_key),
            unit,
            raw.low,
            "analyst price value",
        )?,
        high: optional_price_f64(
            ctx,
            "earningsTrend[].earningsEstimate.high",
            diagnostic_key(period_key),
            unit,
            raw.high,
            "analyst price value",
        )?,
        year_ago_eps: optional_price_f64(
            ctx,
            "earningsTrend[].earningsEstimate.yearAgoEps",
            diagnostic_key(period_key),
            unit,
            raw.year_ago_eps,
            "analyst price value",
        )?,
        num_analysts: optional_u32_from_raw_f64(
            ctx,
            "earningsTrend[].earningsEstimate.numberOfAnalysts",
            diagnostic_key(period_key),
            "numberOfAnalysts",
            raw.num_analysts,
        )?,
        growth: optional_decimal_f64(
            ctx,
            "earningsTrend[].earningsEstimate.growth",
            diagnostic_key(period_key),
            raw.growth,
            "earnings estimate growth",
        )?,
    })
}

fn project_revenue_estimate(
    ctx: &mut ProjectionContext,
    period_key: Option<&str>,
    raw: &RawRevenueEstimate,
    unit: Option<&ResolvedCurrencyUnit>,
) -> Result<RevenueEstimate, YfError> {
    Ok(RevenueEstimate {
        avg: optional_money_i64(
            ctx,
            "earningsTrend[].revenueEstimate.avg",
            diagnostic_key(period_key),
            unit,
            raw.avg,
            "analyst monetary value",
        )?,
        low: optional_money_i64(
            ctx,
            "earningsTrend[].revenueEstimate.low",
            diagnostic_key(period_key),
            unit,
            raw.low,
            "analyst monetary value",
        )?,
        high: optional_money_i64(
            ctx,
            "earningsTrend[].revenueEstimate.high",
            diagnostic_key(period_key),
            unit,
            raw.high,
            "analyst monetary value",
        )?,
        year_ago_revenue: optional_money_i64(
            ctx,
            "earningsTrend[].revenueEstimate.yearAgoRevenue",
            diagnostic_key(period_key),
            unit,
            raw.year_ago_revenue,
            "analyst monetary value",
        )?,
        num_analysts: optional_u32_from_raw_f64(
            ctx,
            "earningsTrend[].revenueEstimate.numberOfAnalysts",
            diagnostic_key(period_key),
            "numberOfAnalysts",
            raw.num_analysts,
        )?,
        growth: optional_decimal_f64(
            ctx,
            "earningsTrend[].revenueEstimate.growth",
            diagnostic_key(period_key),
            raw.growth,
            "revenue estimate growth",
        )?,
    })
}

fn push_eps_trend_point(
    ctx: &mut ProjectionContext,
    historical: &mut Vec<TrendPoint>,
    period_key: Option<&str>,
    unit: Option<&ResolvedCurrencyUnit>,
    period: &str,
    path: &'static str,
    value: Option<f64>,
) -> Result<(), YfError> {
    if let Some(value) = optional_price_f64(
        ctx,
        path,
        diagnostic_key(period_key),
        unit,
        value,
        "analyst price value",
    )? && let Ok(point) = TrendPoint::try_new_str(period, value)
    {
        historical.push(point);
    }

    Ok(())
}

fn project_eps_trend(
    ctx: &mut ProjectionContext,
    period_key: Option<&str>,
    raw: &RawEpsTrend,
    unit: Option<&ResolvedCurrencyUnit>,
) -> Result<EpsTrend, YfError> {
    let current = optional_price_f64(
        ctx,
        "earningsTrend[].epsTrend.current",
        diagnostic_key(period_key),
        unit,
        raw.current,
        "analyst price value",
    )?;
    let mut historical = Vec::new();

    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        unit,
        "7d",
        "earningsTrend[].epsTrend.7daysAgo",
        raw.seven_days_ago,
    )?;
    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        unit,
        "30d",
        "earningsTrend[].epsTrend.30daysAgo",
        raw.thirty_days_ago,
    )?;
    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        unit,
        "60d",
        "earningsTrend[].epsTrend.60daysAgo",
        raw.sixty_days_ago,
    )?;
    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        unit,
        "90d",
        "earningsTrend[].epsTrend.90daysAgo",
        raw.ninety_days_ago,
    )?;

    Ok(EpsTrend {
        current,
        historical,
    })
}

fn push_revision_point(
    historical: &mut Vec<RevisionPoint>,
    period: &str,
    up: Option<u32>,
    down: Option<u32>,
) {
    if let (Some(up), Some(down)) = (up, down)
        && let Ok(point) = RevisionPoint::try_new_str(period, up, down)
    {
        historical.push(point);
    }
}

fn project_eps_revisions(
    ctx: &mut ProjectionContext,
    period_key: Option<&str>,
    raw: &RawEpsRevisions,
) -> Result<EpsRevisions, YfError> {
    let up_last_7_days = optional_u32_from_raw_f64(
        ctx,
        "earningsTrend[].epsRevisions.upLast7days",
        diagnostic_key(period_key),
        "upLast7days",
        raw.up_7d,
    )?;
    let up_last_30_days = optional_u32_from_raw_f64(
        ctx,
        "earningsTrend[].epsRevisions.upLast30days",
        diagnostic_key(period_key),
        "upLast30days",
        raw.up_30d,
    )?;
    let down_last_7_days = optional_u32_from_raw_f64(
        ctx,
        "earningsTrend[].epsRevisions.downLast7days",
        diagnostic_key(period_key),
        "downLast7days",
        raw.down_7d,
    )?;
    let down_last_30_days = optional_u32_from_raw_f64(
        ctx,
        "earningsTrend[].epsRevisions.downLast30days",
        diagnostic_key(period_key),
        "downLast30days",
        raw.down_30d,
    )?;

    let mut historical = Vec::new();
    push_revision_point(&mut historical, "7d", up_last_7_days, down_last_7_days);
    push_revision_point(&mut historical, "30d", up_last_30_days, down_last_30_days);

    Ok(EpsRevisions { historical })
}

fn project_earnings_trend_row(
    ctx: &mut ProjectionContext,
    period: Period,
    raw: &RawEarningsTrendItem,
    units: EarningsTrendUnits<'_>,
) -> Result<EarningsTrendRow, YfError> {
    let period_key = raw.period_key.as_deref();

    Ok(EarningsTrendRow {
        period,
        growth: optional_decimal_f64(
            ctx,
            "earningsTrend[].growth",
            diagnostic_key(period_key),
            raw.growth,
            "earnings trend growth",
        )?,
        earnings_estimate: project_earnings_estimate(
            ctx,
            period_key,
            &raw.earnings,
            units.earnings,
        )?,
        revenue_estimate: project_revenue_estimate(ctx, period_key, &raw.revenue, units.revenue)?,
        eps_trend: project_eps_trend(ctx, period_key, &raw.eps_trend, units.eps)?,
        eps_revisions: project_eps_revisions(ctx, period_key, &raw.eps_revisions)?,
    })
}

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

    let Some(earnings_trend) = root.earnings_trend else {
        ctx.unavailable_feature("earningsTrend")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(trend) = earnings_trend.trend else {
        ctx.unavailable_feature("earningsTrend.trend")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let currency_resolver = AnalystCurrencyResolver {
        client,
        symbol,
        override_currency,
        cache_mode,
        retry_override,
        data_quality,
    };

    let mut rows = Vec::with_capacity(trend.len());
    for item in trend {
        let raw = RawEarningsTrendItem::from(item);

        let earnings_values = raw.earnings.price_values();
        let earnings_unit = currency_resolver
            .resolve_if_any(
                &earnings_values,
                AnalystEstimateCurrencyEvidence::Earnings(raw.earnings.currency.as_deref()),
            )
            .await?;

        let revenue_values = raw.revenue.money_values();
        let revenue_unit = currency_resolver
            .resolve_if_any(
                &revenue_values,
                AnalystEstimateCurrencyEvidence::Revenue(raw.revenue.currency.as_deref()),
            )
            .await?;

        let eps_values = raw.eps_trend.price_values();
        let eps_unit = currency_resolver
            .resolve_if_any(
                &eps_values,
                AnalystEstimateCurrencyEvidence::EpsTrend(raw.eps_trend.currency.as_deref()),
            )
            .await?;

        let Some(period) = required_period(
            &mut ctx,
            "earnings_trend",
            raw.period_key.clone(),
            "period",
            raw.period_key.as_deref(),
        )?
        else {
            continue;
        };

        rows.push(project_earnings_trend_row(
            &mut ctx,
            period,
            &raw,
            EarningsTrendUnits {
                earnings: earnings_unit.as_ref(),
                revenue: revenue_unit.as_ref(),
                eps: eps_unit.as_ref(),
            },
        )?);
    }

    Ok(ctx.finish(rows))
}
