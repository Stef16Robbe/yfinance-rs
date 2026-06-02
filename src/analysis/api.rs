use crate::{
    analysis::model::EarningsTrendRow,
    core::{
        CallOptions, DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError,
        YfResponse,
        conversions::{
            i64_to_datetime, string_to_period, string_to_recommendation_action,
            string_to_recommendation_grade,
        },
        currency_resolver::{
            AnalystEstimateCurrencyEvidence, CurrencyHints, CurrencyPurpose, ResolvedCurrencyUnit,
            project_currency_resolution,
        },
        diagnostics::{
            diagnostic_key, nonempty, optional_decimal_f64, optional_money_i64_with_currency_issue,
            optional_parsed, optional_price_f64_with_currency_issue, optional_u32_from_i64,
            optional_u32_from_raw_f64, parse_optional, required_period,
        },
        wire::{RawNum, from_raw},
    },
};

use super::fetch::fetch_modules;
use super::model::{PriceTarget, RecommendationRow, RecommendationSummary, UpgradeDowngradeRow};
use super::wire::{
    EarningsEstimateNode, EarningsTrendItemNode, EpsRevisionsNode, EpsTrendNode, FinancialDataNode,
    RecommendationNode, RecommendationTrendNode, RevenueEstimateNode,
};
use paft::domain::Period;
use paft::fundamentals::analysis::{
    EarningsEstimate, EpsRevisions, EpsTrend, RecommendationAction, RecommendationGrade,
    RevenueEstimate, RevisionPoint, TrendPoint,
};
use paft::money::Currency;
use std::collections::BTreeMap;

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
    options: &CallOptions,
) -> Result<YfResponse<Vec<RecommendationRow>>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", options.data_quality());
    let root = fetch_modules(client, symbol, "recommendationTrend", options).await?;

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
    options: &CallOptions,
) -> Result<YfResponse<RecommendationSummary>, YfError> {
    let root = fetch_modules(client, symbol, "recommendationTrend,financialData", options).await?;

    map_recommendation_summary(
        symbol,
        root.recommendation_trend.as_ref(),
        root.financial_data.as_ref(),
        options.data_quality(),
    )
}

fn map_recommendation_summary(
    symbol: &str,
    recommendation_trend: Option<&RecommendationTrendNode>,
    financial_data: Option<&FinancialDataNode>,
    data_quality: DataQuality,
) -> Result<YfResponse<RecommendationSummary>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let trend = if let Some(recommendation_trend) = recommendation_trend {
        if let Some(trend) = recommendation_trend.trend.as_ref() {
            trend.as_slice()
        } else {
            ctx.unavailable_feature("recommendationTrend.trend")?;
            &[]
        }
    } else {
        ctx.unavailable_feature("recommendationTrend")?;
        &[]
    };

    let latest = trend.first();

    let (latest_period, sb, b, h, s, ss) = if let Some(t) = latest {
        let latest_period = match parse_optional(t.period.as_deref(), string_to_period) {
            Ok(Some(period)) => Some(period),
            Ok(None) => {
                ctx.omitted_present_field(
                    "recommendationTrend.trend[0].period",
                    None,
                    ProjectionIssue::MissingRequiredField { field: "period" },
                )?;
                None
            }
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
        };
        let (sb, b, h, s, ss) =
            recommendation_counts(&mut ctx, LATEST_COUNT_PATHS, t.period.clone(), t)?;
        (latest_period, sb, b, h, s, ss)
    } else {
        (None, None, None, None, None, None)
    };

    let (mean, mean_rating_text) = financial_data.map_or((None, None), |fd| {
        (
            from_raw(fd.recommendation_mean),
            fd.recommendation_key.clone(),
        )
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
        mean_rating_text,
    }))
}

#[allow(clippy::too_many_lines)]
pub(super) async fn upgrades_downgrades(
    client: &YfClient,
    symbol: &str,
    options: &CallOptions,
) -> Result<YfResponse<Vec<UpgradeDowngradeRow>>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", options.data_quality());
    let root = fetch_modules(client, symbol, "upgradeDowngradeHistory", options).await?;

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
        let from_grade = optional_parsed::<RecommendationGrade>(
            &mut ctx,
            "upgradeDowngradeHistory.history[].fromGrade",
            key.clone(),
            "fromGrade",
            h.from_grade.as_deref(),
            string_to_recommendation_grade,
        )?;
        let to_grade = optional_parsed::<RecommendationGrade>(
            &mut ctx,
            "upgradeDowngradeHistory.history[].toGrade",
            key.clone(),
            "toGrade",
            h.to_grade.as_deref(),
            string_to_recommendation_grade,
        )?;
        let (action_value, action_path, action_field) = match nonempty(h.action.as_deref()) {
            Some(action) => (
                Some(action),
                "upgradeDowngradeHistory.history[].action",
                "action",
            ),
            None => (
                nonempty(h.grade_change.as_deref()),
                "upgradeDowngradeHistory.history[].gradeChange",
                "gradeChange",
            ),
        };
        let action = optional_parsed::<RecommendationAction>(
            &mut ctx,
            action_path,
            key.clone(),
            action_field,
            action_value,
            string_to_recommendation_action,
        )?;

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
    options: &CallOptions,
) -> Result<YfResponse<PriceTarget>, YfError> {
    let root = fetch_modules(client, symbol, "financialData", options).await?;
    map_analyst_price_target(
        client,
        symbol,
        override_currency,
        root.financial_data.as_ref(),
        options,
    )
    .await
}

async fn map_analyst_price_target(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    financial_data: Option<&FinancialDataNode>,
    options: &CallOptions,
) -> Result<YfResponse<PriceTarget>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", options.data_quality());
    let Some(fd) = financial_data else {
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
    let (unit, currency_issue) = if [mean, high, low].iter().any(Option::is_some) {
        let projected_currency = project_currency_resolution(
            &mut ctx,
            symbol,
            CurrencyPurpose::AnalystEstimate,
            None,
            client
                .resolve_analyst_price_target_currency(symbol, override_currency, options)
                .await,
        )?;
        let currency_issue = projected_currency.issue().cloned();
        (projected_currency.into_unit(), currency_issue)
    } else {
        (None, None)
    };

    let mean = optional_price_f64_with_currency_issue(
        &mut ctx,
        "financialData.targetMeanPrice",
        Some(symbol.to_string()),
        unit.as_ref(),
        currency_issue.as_ref(),
        mean,
        "analyst price value",
    )?;
    let high = optional_price_f64_with_currency_issue(
        &mut ctx,
        "financialData.targetHighPrice",
        Some(symbol.to_string()),
        unit.as_ref(),
        currency_issue.as_ref(),
        high,
        "analyst price value",
    )?;
    let low = optional_price_f64_with_currency_issue(
        &mut ctx,
        "financialData.targetLowPrice",
        Some(symbol.to_string()),
        unit.as_ref(),
        currency_issue.as_ref(),
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

pub(super) async fn price_target_and_recommendation_summary_from_quote_summary_value(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    value: serde_json::Value,
    options: &CallOptions,
) -> Result<super::InfoAnalysisParts, YfError> {
    let root: super::wire::V10Result = serde_json::from_value(value).map_err(YfError::Json)?;
    Ok(map_price_target_and_recommendation_summary(
        client,
        symbol,
        override_currency,
        &root,
        options,
    )
    .await)
}

async fn map_price_target_and_recommendation_summary(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    root: &super::wire::V10Result,
    options: &CallOptions,
) -> super::InfoAnalysisParts {
    let price_target = map_analyst_price_target(
        client,
        symbol,
        override_currency,
        root.financial_data.as_ref(),
        options,
    )
    .await;
    let recommendation_summary = map_recommendation_summary(
        symbol,
        root.recommendation_trend.as_ref(),
        root.financial_data.as_ref(),
        options.data_quality(),
    );

    super::InfoAnalysisParts {
        price_target,
        recommendation_summary,
    }
}

struct AnalystCurrencyResolver<'a> {
    client: &'a YfClient,
    symbol: &'a str,
    override_currency: Option<Currency>,
    options: &'a CallOptions,
}

impl AnalystCurrencyResolver<'_> {
    async fn resolve(
        &self,
        ctx: &mut ProjectionContext,
        evidence: AnalystEstimateCurrencyEvidence<'_>,
    ) -> Result<ProjectedAnalystCurrency, YfError> {
        let direct_code = match evidence {
            AnalystEstimateCurrencyEvidence::Earnings(code)
            | AnalystEstimateCurrencyEvidence::Revenue(code)
            | AnalystEstimateCurrencyEvidence::EpsTrend(code) => code,
        };

        let projected = project_currency_resolution(
            ctx,
            self.symbol,
            CurrencyPurpose::AnalystEstimate,
            direct_code,
            self.client
                .resolve_analyst_estimate_currency(
                    self.symbol,
                    self.override_currency.clone(),
                    evidence,
                    self.options,
                )
                .await,
        )?;
        let issue = projected.issue().cloned();
        Ok(ProjectedAnalystCurrency {
            unit: projected.into_unit(),
            issue,
        })
    }
}

#[derive(Default)]
struct ProjectedAnalystCurrency {
    unit: Option<ResolvedCurrencyUnit>,
    issue: Option<ProjectionIssue>,
}

impl ProjectedAnalystCurrency {
    const fn as_ref(&self) -> AnalystCurrencyRef<'_> {
        AnalystCurrencyRef {
            unit: self.unit.as_ref(),
            issue: self.issue.as_ref(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct AnalystCurrencyRef<'a> {
    unit: Option<&'a ResolvedCurrencyUnit>,
    issue: Option<&'a ProjectionIssue>,
}

#[derive(Clone, Copy)]
enum AnalystCurrencyField {
    Earnings,
    Revenue,
    EpsTrend,
}

impl AnalystCurrencyField {
    const fn evidence(self, code: Option<&str>) -> AnalystEstimateCurrencyEvidence<'_> {
        match self {
            Self::Earnings => AnalystEstimateCurrencyEvidence::Earnings(code),
            Self::Revenue => AnalystEstimateCurrencyEvidence::Revenue(code),
            Self::EpsTrend => AnalystEstimateCurrencyEvidence::EpsTrend(code),
        }
    }

    fn direct_code(self, raw: &RawEarningsTrendItem) -> Option<&str> {
        match self {
            Self::Earnings => raw.earnings.currency.as_deref(),
            Self::Revenue => raw.revenue.currency.as_deref(),
            Self::EpsTrend => raw.eps_trend.currency.as_deref(),
        }
    }

    fn has_values(self, raw: &RawEarningsTrendItem) -> bool {
        match self {
            Self::Earnings => raw.earnings.price_values().iter().any(Option::is_some),
            Self::Revenue => raw.revenue.money_values().iter().any(Option::is_some),
            Self::EpsTrend => raw.eps_trend.price_values().iter().any(Option::is_some),
        }
    }
}

#[derive(Default)]
struct AnalystCurrencyGroups {
    currencies: BTreeMap<Option<String>, ProjectedAnalystCurrency>,
}

impl AnalystCurrencyGroups {
    fn currency_for(&self, code: Option<&str>) -> AnalystCurrencyRef<'_> {
        self.currencies.get(&currency_group_key(code)).map_or_else(
            AnalystCurrencyRef::default,
            ProjectedAnalystCurrency::as_ref,
        )
    }
}

struct ValidEarningsTrendItem {
    period: Period,
    raw: RawEarningsTrendItem,
}

async fn resolve_analyst_currency_groups(
    resolver: &AnalystCurrencyResolver<'_>,
    ctx: &mut ProjectionContext,
    rows: &[ValidEarningsTrendItem],
    field: AnalystCurrencyField,
) -> Result<AnalystCurrencyGroups, YfError> {
    let mut groups = AnalystCurrencyGroups::default();

    for row in rows {
        if !field.has_values(&row.raw) {
            continue;
        }

        let key = currency_group_key(field.direct_code(&row.raw));
        if groups.currencies.contains_key(&key) {
            continue;
        }

        let currency = resolver
            .resolve(ctx, field.evidence(key.as_deref()))
            .await?;
        groups.currencies.insert(key, currency);
    }

    Ok(groups)
}

fn currency_group_key(code: Option<&str>) -> Option<String> {
    nonempty(code).map(str::to_owned)
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
    earnings: AnalystCurrencyRef<'a>,
    revenue: AnalystCurrencyRef<'a>,
    eps: AnalystCurrencyRef<'a>,
}

fn project_earnings_estimate(
    ctx: &mut ProjectionContext,
    period_key: Option<&str>,
    raw: &RawEarningsEstimate,
    currency: AnalystCurrencyRef<'_>,
) -> Result<EarningsEstimate, YfError> {
    Ok(EarningsEstimate {
        avg: optional_price_f64_with_currency_issue(
            ctx,
            "earningsTrend[].earningsEstimate.avg",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
            raw.avg,
            "analyst price value",
        )?,
        low: optional_price_f64_with_currency_issue(
            ctx,
            "earningsTrend[].earningsEstimate.low",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
            raw.low,
            "analyst price value",
        )?,
        high: optional_price_f64_with_currency_issue(
            ctx,
            "earningsTrend[].earningsEstimate.high",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
            raw.high,
            "analyst price value",
        )?,
        year_ago_eps: optional_price_f64_with_currency_issue(
            ctx,
            "earningsTrend[].earningsEstimate.yearAgoEps",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
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
    currency: AnalystCurrencyRef<'_>,
) -> Result<RevenueEstimate, YfError> {
    Ok(RevenueEstimate {
        avg: optional_money_i64_with_currency_issue(
            ctx,
            "earningsTrend[].revenueEstimate.avg",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
            raw.avg,
            "analyst monetary value",
        )?,
        low: optional_money_i64_with_currency_issue(
            ctx,
            "earningsTrend[].revenueEstimate.low",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
            raw.low,
            "analyst monetary value",
        )?,
        high: optional_money_i64_with_currency_issue(
            ctx,
            "earningsTrend[].revenueEstimate.high",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
            raw.high,
            "analyst monetary value",
        )?,
        year_ago_revenue: optional_money_i64_with_currency_issue(
            ctx,
            "earningsTrend[].revenueEstimate.yearAgoRevenue",
            diagnostic_key(period_key),
            currency.unit,
            currency.issue,
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
    currency: AnalystCurrencyRef<'_>,
    period: &str,
    path: &'static str,
    value: Option<f64>,
) -> Result<(), YfError> {
    if let Some(value) = optional_price_f64_with_currency_issue(
        ctx,
        path,
        diagnostic_key(period_key),
        currency.unit,
        currency.issue,
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
    currency: AnalystCurrencyRef<'_>,
) -> Result<EpsTrend, YfError> {
    let current = optional_price_f64_with_currency_issue(
        ctx,
        "earningsTrend[].epsTrend.current",
        diagnostic_key(period_key),
        currency.unit,
        currency.issue,
        raw.current,
        "analyst price value",
    )?;
    let mut historical = Vec::new();

    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        currency,
        "7d",
        "earningsTrend[].epsTrend.7daysAgo",
        raw.seven_days_ago,
    )?;
    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        currency,
        "30d",
        "earningsTrend[].epsTrend.30daysAgo",
        raw.thirty_days_ago,
    )?;
    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        currency,
        "60d",
        "earningsTrend[].epsTrend.60daysAgo",
        raw.sixty_days_ago,
    )?;
    push_eps_trend_point(
        ctx,
        &mut historical,
        period_key,
        currency,
        "90d",
        "earningsTrend[].epsTrend.90daysAgo",
        raw.ninety_days_ago,
    )?;

    Ok(EpsTrend {
        current,
        historical,
    })
}

#[derive(Clone, Copy)]
struct RevisionField {
    path: &'static str,
    name: &'static str,
    value: Option<u32>,
}

fn push_revision_point(
    ctx: &mut ProjectionContext,
    historical: &mut Vec<RevisionPoint>,
    period_key: Option<&str>,
    period: &str,
    up: RevisionField,
    down: RevisionField,
) -> Result<(), YfError> {
    match (up.value, down.value) {
        (Some(up), Some(down)) => {
            if let Ok(point) = RevisionPoint::try_new_str(period, up, down) {
                historical.push(point);
            }
        }
        (Some(_), None) => ctx.omitted_present_field(
            up.path,
            diagnostic_key(period_key),
            ProjectionIssue::MissingRequiredField { field: down.name },
        )?,
        (None, Some(_)) => ctx.omitted_present_field(
            down.path,
            diagnostic_key(period_key),
            ProjectionIssue::MissingRequiredField { field: up.name },
        )?,
        (None, None) => {}
    }

    Ok(())
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
    push_revision_point(
        ctx,
        &mut historical,
        period_key,
        "7d",
        RevisionField {
            path: "earningsTrend[].epsRevisions.upLast7days",
            name: "upLast7days",
            value: up_last_7_days,
        },
        RevisionField {
            path: "earningsTrend[].epsRevisions.downLast7days",
            name: "downLast7days",
            value: down_last_7_days,
        },
    )?;
    push_revision_point(
        ctx,
        &mut historical,
        period_key,
        "30d",
        RevisionField {
            path: "earningsTrend[].epsRevisions.upLast30days",
            name: "upLast30days",
            value: up_last_30_days,
        },
        RevisionField {
            path: "earningsTrend[].epsRevisions.downLast30days",
            name: "downLast30days",
            value: down_last_30_days,
        },
    )?;

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
    options: &CallOptions,
) -> Result<YfResponse<Vec<EarningsTrendRow>>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", options.data_quality());
    let root = fetch_modules(client, symbol, "earningsTrend", options).await?;

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
        options,
    };

    let mut valid_rows = Vec::with_capacity(trend.len());
    for item in trend {
        let raw = RawEarningsTrendItem::from(item);

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

        valid_rows.push(ValidEarningsTrendItem { period, raw });
    }

    let earnings_currencies = resolve_analyst_currency_groups(
        &currency_resolver,
        &mut ctx,
        &valid_rows,
        AnalystCurrencyField::Earnings,
    )
    .await?;
    let revenue_currencies = resolve_analyst_currency_groups(
        &currency_resolver,
        &mut ctx,
        &valid_rows,
        AnalystCurrencyField::Revenue,
    )
    .await?;
    let eps_currencies = resolve_analyst_currency_groups(
        &currency_resolver,
        &mut ctx,
        &valid_rows,
        AnalystCurrencyField::EpsTrend,
    )
    .await?;

    let mut rows = Vec::with_capacity(valid_rows.len());
    for item in valid_rows {
        let raw = item.raw;

        rows.push(project_earnings_trend_row(
            &mut ctx,
            item.period,
            &raw,
            EarningsTrendUnits {
                earnings: earnings_currencies.currency_for(raw.earnings.currency.as_deref()),
                revenue: revenue_currencies.currency_for(raw.revenue.currency.as_deref()),
                eps: eps_currencies.currency_for(raw.eps_trend.currency.as_deref()),
            },
        )?);
    }

    Ok(ctx.finish(rows))
}
