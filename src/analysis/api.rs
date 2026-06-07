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
            optional_u32_from_raw_f64, optional_wire_value, parse_optional, required_period,
        },
        wire::{RawNum, WireValue},
    },
};

use super::fetch::fetch_modules;
use super::model::{PriceTarget, RecommendationRow, RecommendationSummary, UpgradeDowngradeRow};
use super::wire::{
    EarningsEstimateNode, EarningsTrendItemNode, EpsRevisionsNode, EpsTrendNode, FinancialDataNode,
    RecommendationNode, RecommendationTrendNode, RevenueEstimateNode,
};
use paft::domain::ReportingPeriod;
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

fn module_ref<'a, T>(
    ctx: &mut ProjectionContext,
    feature: &'static str,
    field: &'static str,
    value: &'a WireValue<T>,
) -> Result<Option<&'a T>, YfError> {
    if let Some(details) = value.invalid_details() {
        ctx.provider_feature_unavailable(
            feature,
            ProjectionIssue::InvalidField {
                field,
                details: details.to_string(),
            },
        )?;
        return Ok(None);
    }

    Ok(value.as_ref())
}

fn optional_string_from_wire(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &WireValue<String>,
) -> Result<Option<String>, YfError> {
    Ok(optional_wire_value(ctx, path, key, field, value)?.cloned())
}

fn optional_i64_from_wire(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &WireValue<i64>,
) -> Result<Option<i64>, YfError> {
    Ok(optional_wire_value(ctx, path, key, field, value)?.copied())
}

fn optional_raw_num_from_wire<T: Copy>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &WireValue<RawNum<T>>,
) -> Result<Option<RawNum<T>>, YfError> {
    Ok(optional_wire_value(ctx, path, key, field, value)?.copied())
}

fn optional_raw_value_from_wire<T: Copy>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &WireValue<RawNum<T>>,
) -> Result<Option<T>, YfError> {
    Ok(optional_raw_num_from_wire(ctx, path, key, field, value)?.and_then(|raw| raw.raw))
}

fn key_from_wire(value: &WireValue<String>) -> Option<String> {
    value.as_ref().cloned()
}

fn required_period_from_wire(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &WireValue<String>,
) -> Result<Option<ReportingPeriod>, YfError> {
    if let Some(details) = value.invalid_details() {
        ctx.dropped_item(
            item,
            key,
            ProjectionIssue::InvalidField {
                field,
                details: details.to_string(),
            },
        )?;
        return Ok(None);
    }

    required_period(ctx, item, key, field, value.as_ref().map(String::as_str))
}

fn required_i64_from_wire(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &WireValue<i64>,
) -> Result<Option<i64>, YfError> {
    match value {
        WireValue::Valid(value) => Ok(Some(*value)),
        WireValue::Missing => {
            ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
            Ok(None)
        }
        WireValue::Invalid(details) => {
            ctx.dropped_item(
                item,
                key,
                ProjectionIssue::InvalidField {
                    field,
                    details: details.clone(),
                },
            )?;
            Ok(None)
        }
    }
}

fn recommendation_counts(
    ctx: &mut ProjectionContext,
    paths: RecommendationCountPaths,
    key: Option<&str>,
    node: &RecommendationNode,
) -> Result<RecommendationCounts, YfError> {
    let strong_buy_raw =
        optional_i64_from_wire(ctx, paths.strong_buy, key, "strongBuy", &node.strong_buy)?;
    let buy_raw = optional_i64_from_wire(ctx, paths.buy, key, "buy", &node.buy)?;
    let hold_raw = optional_i64_from_wire(ctx, paths.hold, key, "hold", &node.hold)?;
    let sell_raw = optional_i64_from_wire(ctx, paths.sell, key, "sell", &node.sell)?;
    let strong_sell_raw =
        optional_i64_from_wire(ctx, paths.strong_sell, key, "strongSell", &node.strong_sell)?;

    Ok((
        optional_u32_from_i64(ctx, paths.strong_buy, key, "strongBuy", strong_buy_raw)?,
        optional_u32_from_i64(ctx, paths.buy, key, "buy", buy_raw)?,
        optional_u32_from_i64(ctx, paths.hold, key, "hold", hold_raw)?,
        optional_u32_from_i64(ctx, paths.sell, key, "sell", sell_raw)?,
        optional_u32_from_i64(ctx, paths.strong_sell, key, "strongSell", strong_sell_raw)?,
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

    let Some(recommendation_trend) = module_ref(
        &mut ctx,
        "recommendationTrend",
        "recommendationTrend",
        &root.recommendation_trend,
    )?
    else {
        ctx.unavailable_feature("recommendationTrend")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(trend) = module_ref(
        &mut ctx,
        "recommendationTrend.trend",
        "trend",
        &recommendation_trend.trend,
    )?
    else {
        ctx.unavailable_feature("recommendationTrend.trend")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let mut rows = Vec::new();
    for n in trend {
        let key = key_from_wire(&n.period);
        let Some(period) = required_period_from_wire(
            &mut ctx,
            "recommendation_trend",
            key.as_deref(),
            "period",
            &n.period,
        )?
        else {
            continue;
        };

        let (strong_buy, buy, hold, sell, strong_sell) =
            recommendation_counts(&mut ctx, TREND_COUNT_PATHS, key.as_deref(), n)?;

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
        &root.recommendation_trend,
        &root.financial_data,
        options.data_quality(),
    )
}

#[allow(clippy::too_many_lines)]
fn map_recommendation_summary(
    symbol: &str,
    recommendation_trend: &WireValue<RecommendationTrendNode>,
    financial_data: &WireValue<FinancialDataNode>,
    data_quality: DataQuality,
) -> Result<YfResponse<RecommendationSummary>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", data_quality);
    let trend = if let Some(recommendation_trend) = module_ref(
        &mut ctx,
        "recommendationTrend",
        "recommendationTrend",
        recommendation_trend,
    )? {
        if let Some(trend) = module_ref(
            &mut ctx,
            "recommendationTrend.trend",
            "trend",
            &recommendation_trend.trend,
        )? {
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
        let latest_period = if let Some(details) = t.period.invalid_details() {
            ctx.omitted_present_field(
                "recommendationTrend.trend[0].period",
                None,
                ProjectionIssue::InvalidField {
                    field: "period",
                    details: details.to_string(),
                },
            )?;
            None
        } else {
            match parse_optional(t.period.as_ref().map(String::as_str), string_to_period) {
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
                        key_from_wire(&t.period).as_deref(),
                        ProjectionIssue::InvalidField {
                            field: "period",
                            details: err.to_string(),
                        },
                    )?;
                    None
                }
            }
        };
        let period_key = key_from_wire(&t.period);
        let (sb, b, h, s, ss) =
            recommendation_counts(&mut ctx, LATEST_COUNT_PATHS, period_key.as_deref(), t)?;
        (latest_period, sb, b, h, s, ss)
    } else {
        (None, None, None, None, None, None)
    };

    let fd = module_ref(&mut ctx, "financialData", "financialData", financial_data)?;
    let (mean, mean_rating_text) = if let Some(fd) = fd {
        (
            optional_raw_value_from_wire(
                &mut ctx,
                "financialData.recommendationMean",
                Some(symbol),
                "recommendationMean",
                &fd.recommendation_mean,
            )?,
            optional_string_from_wire(
                &mut ctx,
                "financialData.recommendationKey",
                Some(symbol),
                "recommendationKey",
                &fd.recommendation_key,
            )?,
        )
    } else {
        (None, None)
    };
    let mean = optional_decimal_f64(
        &mut ctx,
        "financialData.recommendationMean",
        Some(symbol),
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

    let Some(upgrade_downgrade_history) = module_ref(
        &mut ctx,
        "upgradeDowngradeHistory",
        "upgradeDowngradeHistory",
        &root.upgrade_downgrade_history,
    )?
    else {
        ctx.unavailable_feature("upgradeDowngradeHistory")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(hist) = module_ref(
        &mut ctx,
        "upgradeDowngradeHistory.history",
        "history",
        &upgrade_downgrade_history.history,
    )?
    else {
        ctx.unavailable_feature("upgradeDowngradeHistory.history")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let mut rows = Vec::new();
    for h in hist {
        let key = key_from_wire(&h.firm);
        let Some(ts_raw) = required_i64_from_wire(
            &mut ctx,
            "upgrade_downgrade",
            key.as_deref(),
            "epochGradeDate",
            &h.epoch_grade_date,
        )?
        else {
            continue;
        };
        let ts = match i64_to_datetime(ts_raw) {
            Ok(ts) => ts,
            Err(err) => {
                ctx.dropped_item(
                    "upgrade_downgrade",
                    key.as_deref(),
                    ProjectionIssue::InvalidField {
                        field: "epochGradeDate",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let from_grade_value = optional_wire_value(
            &mut ctx,
            "upgradeDowngradeHistory.history[].fromGrade",
            key.as_deref(),
            "fromGrade",
            &h.from_grade,
        )?
        .map(String::as_str);
        let from_grade = optional_parsed::<RecommendationGrade>(
            &mut ctx,
            "upgradeDowngradeHistory.history[].fromGrade",
            key.as_deref(),
            "fromGrade",
            from_grade_value,
            string_to_recommendation_grade,
        )?;
        let to_grade_value = optional_wire_value(
            &mut ctx,
            "upgradeDowngradeHistory.history[].toGrade",
            key.as_deref(),
            "toGrade",
            &h.to_grade,
        )?
        .map(String::as_str);
        let to_grade = optional_parsed::<RecommendationGrade>(
            &mut ctx,
            "upgradeDowngradeHistory.history[].toGrade",
            key.as_deref(),
            "toGrade",
            to_grade_value,
            string_to_recommendation_grade,
        )?;
        let action = optional_wire_value(
            &mut ctx,
            "upgradeDowngradeHistory.history[].action",
            key.as_deref(),
            "action",
            &h.action,
        )?
        .map(String::as_str);
        let grade_change = optional_wire_value(
            &mut ctx,
            "upgradeDowngradeHistory.history[].gradeChange",
            key.as_deref(),
            "gradeChange",
            &h.grade_change,
        )?
        .map(String::as_str);
        let (action_value, action_path, action_field) = nonempty(action).map_or_else(
            || {
                (
                    nonempty(grade_change),
                    "upgradeDowngradeHistory.history[].gradeChange",
                    "gradeChange",
                )
            },
            |action| {
                (
                    Some(action),
                    "upgradeDowngradeHistory.history[].action",
                    "action",
                )
            },
        );
        let action = optional_parsed::<RecommendationAction>(
            &mut ctx,
            action_path,
            key.as_deref(),
            action_field,
            action_value,
            string_to_recommendation_action,
        )?;

        rows.push(UpgradeDowngradeRow {
            ts,
            firm: key,
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
        &root.financial_data,
        options,
    )
    .await
}

#[allow(clippy::too_many_lines)]
async fn map_analyst_price_target(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    financial_data: &WireValue<FinancialDataNode>,
    options: &CallOptions,
) -> Result<YfResponse<PriceTarget>, YfError> {
    let mut ctx = ProjectionContext::new("analysis", options.data_quality());
    let Some(fd) = module_ref(&mut ctx, "financialData", "financialData", financial_data)? else {
        ctx.unavailable_feature("financialData")?;
        return Ok(ctx.finish(PriceTarget::default()));
    };

    let financial_currency = optional_string_from_wire(
        &mut ctx,
        "financialData.financialCurrency",
        Some(symbol),
        "financialCurrency",
        &fd.financial_currency,
    )?;
    client.store_currency_hints(
        symbol,
        CurrencyHints::from_quote_summary_financial(financial_currency.as_deref()),
    );

    let mean = optional_raw_value_from_wire(
        &mut ctx,
        "financialData.targetMeanPrice",
        Some(symbol),
        "targetMeanPrice",
        &fd.target_mean_price,
    )?;
    let high = optional_raw_value_from_wire(
        &mut ctx,
        "financialData.targetHighPrice",
        Some(symbol),
        "targetHighPrice",
        &fd.target_high_price,
    )?;
    let low = optional_raw_value_from_wire(
        &mut ctx,
        "financialData.targetLowPrice",
        Some(symbol),
        "targetLowPrice",
        &fd.target_low_price,
    )?;
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
        Some(symbol),
        unit.as_ref(),
        currency_issue.as_ref(),
        mean,
        "analyst price value",
    )?;
    let high = optional_price_f64_with_currency_issue(
        &mut ctx,
        "financialData.targetHighPrice",
        Some(symbol),
        unit.as_ref(),
        currency_issue.as_ref(),
        high,
        "analyst price value",
    )?;
    let low = optional_price_f64_with_currency_issue(
        &mut ctx,
        "financialData.targetLowPrice",
        Some(symbol),
        unit.as_ref(),
        currency_issue.as_ref(),
        low,
        "analyst price value",
    )?;

    let number_of_analysts_raw = optional_raw_num_from_wire(
        &mut ctx,
        "financialData.numberOfAnalystOpinions",
        Some(symbol),
        "numberOfAnalystOpinions",
        &fd.number_of_analyst_opinions,
    )?;
    let number_of_analysts = optional_u32_from_raw_f64(
        &mut ctx,
        "financialData.numberOfAnalystOpinions",
        Some(symbol),
        "numberOfAnalystOpinions",
        number_of_analysts_raw,
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
        &root.financial_data,
        options,
    )
    .await;
    let recommendation_summary = map_recommendation_summary(
        symbol,
        &root.recommendation_trend,
        &root.financial_data,
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
    period: ReportingPeriod,
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

    fn from_wire(
        ctx: &mut ProjectionContext,
        key: Option<&str>,
        node: &WireValue<EarningsEstimateNode>,
    ) -> Result<Self, YfError> {
        let Some(node) = optional_wire_value(
            ctx,
            "earningsTrend[].earningsEstimate",
            key,
            "earningsEstimate",
            node,
        )?
        else {
            return Ok(Self::default());
        };

        Ok(Self {
            currency: optional_string_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.earningsCurrency",
                key,
                "earningsCurrency",
                &node.earnings_currency,
            )?,
            avg: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.avg",
                key,
                "avg",
                &node.avg,
            )?,
            low: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.low",
                key,
                "low",
                &node.low,
            )?,
            high: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.high",
                key,
                "high",
                &node.high,
            )?,
            year_ago_eps: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.yearAgoEps",
                key,
                "yearAgoEps",
                &node.year_ago_eps,
            )?,
            num_analysts: optional_raw_num_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.numberOfAnalysts",
                key,
                "numberOfAnalysts",
                &node.num_analysts,
            )?,
            growth: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].earningsEstimate.growth",
                key,
                "growth",
                &node.growth,
            )?,
        })
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

    fn from_wire(
        ctx: &mut ProjectionContext,
        key: Option<&str>,
        node: &WireValue<RevenueEstimateNode>,
    ) -> Result<Self, YfError> {
        let Some(node) = optional_wire_value(
            ctx,
            "earningsTrend[].revenueEstimate",
            key,
            "revenueEstimate",
            node,
        )?
        else {
            return Ok(Self::default());
        };

        Ok(Self {
            currency: optional_string_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.revenueCurrency",
                key,
                "revenueCurrency",
                &node.revenue_currency,
            )?,
            avg: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.avg",
                key,
                "avg",
                &node.avg,
            )?,
            low: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.low",
                key,
                "low",
                &node.low,
            )?,
            high: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.high",
                key,
                "high",
                &node.high,
            )?,
            year_ago_revenue: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.yearAgoRevenue",
                key,
                "yearAgoRevenue",
                &node.year_ago_revenue,
            )?,
            num_analysts: optional_raw_num_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.numberOfAnalysts",
                key,
                "numberOfAnalysts",
                &node.num_analysts,
            )?,
            growth: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].revenueEstimate.growth",
                key,
                "growth",
                &node.growth,
            )?,
        })
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

    fn from_wire(
        ctx: &mut ProjectionContext,
        key: Option<&str>,
        node: &WireValue<EpsTrendNode>,
    ) -> Result<Self, YfError> {
        let Some(node) =
            optional_wire_value(ctx, "earningsTrend[].epsTrend", key, "epsTrend", node)?
        else {
            return Ok(Self::default());
        };

        Ok(Self {
            currency: optional_string_from_wire(
                ctx,
                "earningsTrend[].epsTrend.epsTrendCurrency",
                key,
                "epsTrendCurrency",
                &node.eps_trend_currency,
            )?,
            current: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].epsTrend.current",
                key,
                "current",
                &node.current,
            )?,
            seven_days_ago: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].epsTrend.7daysAgo",
                key,
                "7daysAgo",
                &node.seven_days_ago,
            )?,
            thirty_days_ago: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].epsTrend.30daysAgo",
                key,
                "30daysAgo",
                &node.thirty_days_ago,
            )?,
            sixty_days_ago: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].epsTrend.60daysAgo",
                key,
                "60daysAgo",
                &node.sixty_days_ago,
            )?,
            ninety_days_ago: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].epsTrend.90daysAgo",
                key,
                "90daysAgo",
                &node.ninety_days_ago,
            )?,
        })
    }
}

#[derive(Default)]
struct RawEpsRevisions {
    up_7d: Option<RawNum<f64>>,
    up_30d: Option<RawNum<f64>>,
    down_7d: Option<RawNum<f64>>,
    down_30d: Option<RawNum<f64>>,
}

impl RawEpsRevisions {
    fn from_wire(
        ctx: &mut ProjectionContext,
        key: Option<&str>,
        node: &WireValue<EpsRevisionsNode>,
    ) -> Result<Self, YfError> {
        let Some(node) = optional_wire_value(
            ctx,
            "earningsTrend[].epsRevisions",
            key,
            "epsRevisions",
            node,
        )?
        else {
            return Ok(Self::default());
        };

        Ok(Self {
            up_7d: optional_raw_num_from_wire(
                ctx,
                "earningsTrend[].epsRevisions.upLast7days",
                key,
                "upLast7days",
                &node.up_last_7_days,
            )?,
            up_30d: optional_raw_num_from_wire(
                ctx,
                "earningsTrend[].epsRevisions.upLast30days",
                key,
                "upLast30days",
                &node.up_last_30_days,
            )?,
            down_7d: optional_raw_num_from_wire(
                ctx,
                "earningsTrend[].epsRevisions.downLast7days",
                key,
                "downLast7days",
                &node.down_last_7_days,
            )?,
            down_30d: optional_raw_num_from_wire(
                ctx,
                "earningsTrend[].epsRevisions.downLast30days",
                key,
                "downLast30days",
                &node.down_last_30_days,
            )?,
        })
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

impl RawEarningsTrendItem {
    fn from_node(
        ctx: &mut ProjectionContext,
        node: &EarningsTrendItemNode,
    ) -> Result<Self, YfError> {
        let key = key_from_wire(&node.period);

        Ok(Self {
            period_key: key.clone(),
            growth: optional_raw_value_from_wire(
                ctx,
                "earningsTrend[].growth",
                key.as_deref(),
                "growth",
                &node.growth,
            )?,
            earnings: RawEarningsEstimate::from_wire(ctx, key.as_deref(), &node.earnings_estimate)?,
            revenue: RawRevenueEstimate::from_wire(ctx, key.as_deref(), &node.revenue_estimate)?,
            eps_trend: RawEpsTrend::from_wire(ctx, key.as_deref(), &node.eps_trend)?,
            eps_revisions: RawEpsRevisions::from_wire(ctx, key.as_deref(), &node.eps_revisions)?,
        })
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
    period: ReportingPeriod,
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

    let Some(earnings_trend) = module_ref(
        &mut ctx,
        "earningsTrend",
        "earningsTrend",
        &root.earnings_trend,
    )?
    else {
        ctx.unavailable_feature("earningsTrend")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(trend) = module_ref(
        &mut ctx,
        "earningsTrend.trend",
        "trend",
        &earnings_trend.trend,
    )?
    else {
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
        let Some(period) = required_period_from_wire(
            &mut ctx,
            "earnings_trend",
            key_from_wire(&item.period).as_deref(),
            "period",
            &item.period,
        )?
        else {
            continue;
        };
        let raw = RawEarningsTrendItem::from_node(&mut ctx, item)?;

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
