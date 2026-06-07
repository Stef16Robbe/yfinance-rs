use crate::core::wire::{RawDate, RawDecimal, RawNum, RawNumU64, WireValue};
use serde::Deserialize;

/* ---------------- Serde mapping (only what we need) ---------------- */

#[derive(Deserialize)]
pub struct V10Result {
    /* income */
    #[allow(dead_code)]
    #[serde(rename = "incomeStatementHistory")]
    pub(crate) income_statement_history: Option<IncomeHistoryNode>,
    #[allow(dead_code)]
    #[serde(rename = "incomeStatementHistoryQuarterly")]
    pub(crate) income_statement_history_quarterly: Option<IncomeHistoryNode>,

    /* earnings + calendar */
    #[serde(default)]
    pub(crate) earnings: WireValue<EarningsNode>,
    #[serde(rename = "calendarEvents")]
    #[serde(default)]
    pub(crate) calendar_events: WireValue<CalendarEventsNode>,
}

/* --- income --- */
#[derive(Deserialize)]
pub struct IncomeHistoryNode {
    #[allow(dead_code)]
    #[serde(rename = "incomeStatementHistory")]
    pub(crate) income_statement_history: Option<Vec<IncomeRowNode>>,
}

#[derive(Deserialize)]
pub struct IncomeRowNode {
    #[allow(dead_code)]
    #[serde(rename = "endDate")]
    pub(crate) end_date: Option<RawDate>,
    #[allow(dead_code)]
    #[serde(rename = "totalRevenue")]
    pub(crate) total_revenue: Option<RawDecimal>,
    #[allow(dead_code)]
    #[serde(rename = "grossProfit")]
    pub(crate) gross_profit: Option<RawDecimal>,
    #[allow(dead_code)]
    #[serde(rename = "operatingIncome")]
    pub(crate) operating_income: Option<RawDecimal>,
    #[allow(dead_code)]
    #[serde(rename = "netIncome")]
    pub(crate) net_income: Option<RawDecimal>,
}

/* --- earnings --- */
#[derive(Deserialize)]
pub struct EarningsNode {
    #[serde(rename = "financialCurrency")]
    #[serde(default)]
    pub(crate) financial_currency: WireValue<String>,
    #[serde(rename = "financialsChart")]
    #[serde(default)]
    pub(crate) financials_chart: WireValue<FinancialsChartNode>,
    #[serde(rename = "earningsChart")]
    #[serde(default)]
    pub(crate) earnings_chart: WireValue<EarningsChartNode>,
}

#[derive(Deserialize)]
pub struct FinancialsChartNode {
    #[serde(default)]
    pub(crate) yearly: WireValue<Vec<FinancialYearNode>>,
    #[serde(default)]
    pub(crate) quarterly: WireValue<Vec<FinancialQuarterNode>>,
}

#[derive(Deserialize)]
pub struct FinancialYearNode {
    #[serde(default)]
    pub(crate) date: WireValue<i64>,
    #[serde(default)]
    pub(crate) revenue: WireValue<RawDecimal>,
    #[serde(default)]
    pub(crate) earnings: WireValue<RawDecimal>,
}

#[derive(Deserialize)]
pub struct FinancialQuarterNode {
    #[serde(default)]
    pub(crate) date: WireValue<String>,
    #[serde(default)]
    pub(crate) revenue: WireValue<RawDecimal>,
    #[serde(default)]
    pub(crate) earnings: WireValue<RawDecimal>,
}

#[derive(Deserialize)]
pub struct EarningsChartNode {
    #[serde(default)]
    pub(crate) quarterly: WireValue<Vec<EpsQuarterNode>>,
}

#[derive(Deserialize)]
pub struct EpsQuarterNode {
    #[serde(default)]
    pub(crate) date: WireValue<String>,
    #[serde(default)]
    pub(crate) actual: WireValue<RawNum<f64>>,
    #[serde(default)]
    pub(crate) estimate: WireValue<RawNum<f64>>,
}

/* --- calendar --- */
#[derive(Deserialize)]
pub struct CalendarEventsNode {
    #[serde(default)]
    pub(crate) earnings: WireValue<CalendarEarningsNode>,
    #[serde(rename = "exDividendDate")]
    #[serde(default)]
    pub(crate) ex_dividend_date: WireValue<RawDate>,
    #[serde(rename = "dividendDate")]
    #[serde(default)]
    pub(crate) dividend_date: WireValue<RawDate>,
}

#[derive(Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct CalendarEarningsNode {
    #[serde(rename = "earningsDate")]
    #[serde(default)]
    pub(crate) earnings_date: WireValue<Vec<RawDate>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeseriesEnvelope {
    pub(crate) timeseries: Option<TimeseriesResult>,
    pub(crate) finance: Option<TimeseriesErrorNode>,
}

#[derive(Deserialize)]
pub struct TimeseriesResult {
    pub(crate) result: Option<Vec<TimeseriesData>>,
    pub(crate) error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct TimeseriesErrorNode {
    pub(crate) error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct TimeseriesData {
    pub(crate) timestamp: Option<Vec<i64>>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) meta: serde_json::Value,
    #[serde(flatten)]
    pub(crate) values: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub struct TimeseriesValue {
    #[serde(rename = "reportedValue")]
    pub(crate) reported_value: Option<RawNumU64>,
}
