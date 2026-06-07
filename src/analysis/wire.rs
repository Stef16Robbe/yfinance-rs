use serde::Deserialize;

use crate::core::wire::{BufferedWireValue, RawNum, WireValue};

/* ---------------- Serde mapping (only what we need) ---------------- */

#[derive(Deserialize)]
pub struct V10Result {
    #[serde(rename = "recommendationTrend")]
    #[serde(default)]
    pub(crate) recommendation_trend: BufferedWireValue<RecommendationTrendNode>,

    #[serde(rename = "upgradeDowngradeHistory")]
    #[serde(default)]
    pub(crate) upgrade_downgrade_history: BufferedWireValue<UpgradeDowngradeHistoryNode>,

    #[serde(rename = "financialData")]
    #[serde(default)]
    pub(crate) financial_data: BufferedWireValue<FinancialDataNode>,

    #[serde(rename = "earningsTrend")]
    #[serde(default)]
    pub(crate) earnings_trend: BufferedWireValue<EarningsTrendNode>,
}

/* --- recommendation trend --- */

#[derive(Deserialize)]
pub struct RecommendationTrendNode {
    #[serde(default)]
    pub(crate) trend: BufferedWireValue<Vec<RecommendationNode>>,
}

#[derive(Deserialize)]
pub struct RecommendationNode {
    #[serde(default)]
    pub(crate) period: WireValue<String>,

    #[serde(rename = "strongBuy")]
    #[serde(default)]
    pub(crate) strong_buy: WireValue<i64>,
    #[serde(default)]
    pub(crate) buy: WireValue<i64>,
    #[serde(default)]
    pub(crate) hold: WireValue<i64>,
    #[serde(default)]
    pub(crate) sell: WireValue<i64>,

    #[serde(rename = "strongSell")]
    #[serde(default)]
    pub(crate) strong_sell: WireValue<i64>,
}

/* --- upgrades / downgrades --- */

#[derive(Deserialize)]
pub struct UpgradeDowngradeHistoryNode {
    #[serde(default)]
    pub(crate) history: BufferedWireValue<Vec<UpgradeNode>>,
}

#[derive(Deserialize)]
pub struct UpgradeNode {
    #[serde(rename = "epochGradeDate")]
    #[serde(default)]
    pub(crate) epoch_grade_date: WireValue<i64>,

    #[serde(default)]
    pub(crate) firm: WireValue<String>,

    #[serde(rename = "toGrade")]
    #[serde(default)]
    pub(crate) to_grade: WireValue<String>,

    #[serde(rename = "fromGrade")]
    #[serde(default)]
    pub(crate) from_grade: WireValue<String>,

    #[serde(default)]
    pub(crate) action: WireValue<String>,
    #[serde(rename = "gradeChange")]
    #[serde(default)]
    pub(crate) grade_change: WireValue<String>,
}

/* --- financial data (price targets) --- */

#[derive(Deserialize)]
pub struct FinancialDataNode {
    #[serde(rename = "financialCurrency")]
    #[serde(default)]
    pub(crate) financial_currency: WireValue<String>,
    #[serde(rename = "targetMeanPrice")]
    #[serde(default)]
    pub(crate) target_mean_price: WireValue<RawNum<f64>>,
    #[serde(rename = "targetHighPrice")]
    #[serde(default)]
    pub(crate) target_high_price: WireValue<RawNum<f64>>,
    #[serde(rename = "targetLowPrice")]
    #[serde(default)]
    pub(crate) target_low_price: WireValue<RawNum<f64>>,
    #[serde(rename = "numberOfAnalystOpinions")]
    #[serde(default)]
    pub(crate) number_of_analyst_opinions: WireValue<RawNum<f64>>,
    #[serde(rename = "recommendationMean")]
    #[serde(default)]
    pub(crate) recommendation_mean: WireValue<RawNum<f64>>,
    #[serde(rename = "recommendationKey")]
    #[serde(default)]
    pub(crate) recommendation_key: WireValue<String>,
}

#[derive(Deserialize)]
pub struct EarningsTrendNode {
    #[serde(default)]
    pub(crate) trend: BufferedWireValue<Vec<EarningsTrendItemNode>>,
}

#[derive(Deserialize)]
pub struct EarningsTrendItemNode {
    #[serde(default)]
    pub(crate) period: WireValue<String>,
    #[serde(default)]
    pub(crate) growth: WireValue<RawNum<f64>>,
    #[serde(rename = "earningsEstimate")]
    #[serde(default)]
    pub(crate) earnings_estimate: BufferedWireValue<EarningsEstimateNode>,
    #[serde(rename = "revenueEstimate")]
    #[serde(default)]
    pub(crate) revenue_estimate: BufferedWireValue<RevenueEstimateNode>,
    #[serde(rename = "epsTrend")]
    #[serde(default)]
    pub(crate) eps_trend: BufferedWireValue<EpsTrendNode>,
    #[serde(rename = "epsRevisions")]
    #[serde(default)]
    pub(crate) eps_revisions: BufferedWireValue<EpsRevisionsNode>,
}

#[derive(Deserialize)]
pub struct EarningsEstimateNode {
    #[serde(rename = "earningsCurrency")]
    #[serde(default)]
    pub(crate) earnings_currency: WireValue<String>,
    #[serde(default)]
    pub(crate) avg: WireValue<RawNum<f64>>,
    #[serde(default)]
    pub(crate) low: WireValue<RawNum<f64>>,
    #[serde(default)]
    pub(crate) high: WireValue<RawNum<f64>>,
    #[serde(rename = "yearAgoEps")]
    #[serde(default)]
    pub(crate) year_ago_eps: WireValue<RawNum<f64>>,
    #[serde(rename = "numberOfAnalysts")]
    #[serde(default)]
    pub(crate) num_analysts: WireValue<RawNum<f64>>,
    #[serde(default)]
    pub(crate) growth: WireValue<RawNum<f64>>,
}

#[derive(Deserialize)]
pub struct RevenueEstimateNode {
    #[serde(rename = "revenueCurrency")]
    #[serde(default)]
    pub(crate) revenue_currency: WireValue<String>,
    #[serde(default)]
    pub(crate) avg: WireValue<RawNum<i64>>,
    #[serde(default)]
    pub(crate) low: WireValue<RawNum<i64>>,
    #[serde(default)]
    pub(crate) high: WireValue<RawNum<i64>>,
    #[serde(rename = "yearAgoRevenue")]
    #[serde(default)]
    pub(crate) year_ago_revenue: WireValue<RawNum<i64>>,
    #[serde(rename = "numberOfAnalysts")]
    #[serde(default)]
    pub(crate) num_analysts: WireValue<RawNum<f64>>,
    #[serde(default)]
    pub(crate) growth: WireValue<RawNum<f64>>,
}

#[derive(Deserialize)]
pub struct EpsTrendNode {
    #[serde(rename = "epsTrendCurrency")]
    #[serde(default)]
    pub(crate) eps_trend_currency: WireValue<String>,
    #[serde(default)]
    pub(crate) current: WireValue<RawNum<f64>>,
    #[serde(rename = "7daysAgo")]
    #[serde(default)]
    pub(crate) seven_days_ago: WireValue<RawNum<f64>>,
    #[serde(rename = "30daysAgo")]
    #[serde(default)]
    pub(crate) thirty_days_ago: WireValue<RawNum<f64>>,
    #[serde(rename = "60daysAgo")]
    #[serde(default)]
    pub(crate) sixty_days_ago: WireValue<RawNum<f64>>,
    #[serde(rename = "90daysAgo")]
    #[serde(default)]
    pub(crate) ninety_days_ago: WireValue<RawNum<f64>>,
}

#[derive(Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct EpsRevisionsNode {
    #[serde(rename = "upLast7days")]
    #[serde(default)]
    pub(crate) up_last_7_days: WireValue<RawNum<f64>>,
    #[serde(rename = "upLast30days")]
    #[serde(default)]
    pub(crate) up_last_30_days: WireValue<RawNum<f64>>,
    #[serde(rename = "downLast7days", alias = "downLast7Days")]
    #[serde(default)]
    pub(crate) down_last_7_days: WireValue<RawNum<f64>>,
    #[serde(rename = "downLast30days", alias = "downLast30Days")]
    #[serde(default)]
    pub(crate) down_last_30_days: WireValue<RawNum<f64>>,
}
