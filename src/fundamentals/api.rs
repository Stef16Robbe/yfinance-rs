use chrono::{Duration, Utc};
use std::collections::{BTreeMap, btree_map::Entry};

use crate::{
    core::{
        DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError, YfResponse,
        client::{CacheEndpoint, CacheMode, RetryConfig, SymbolEndpoint},
        conversions::{i64_to_datetime, string_to_period},
        currency_resolver::{
            CurrencyHints, CurrencyKind, ReportingCurrencyEvidence, ResolvedCurrencyUnit,
        },
        diagnostics::{optional_money_f64, optional_price_f64},
        wire::{RawNum, RawNumU64},
    },
    fundamentals::wire::{TimeseriesData, TimeseriesEnvelope},
};
use paft::domain::Period;
use paft::fundamentals::profile::ShareCount;
use paft::money::Currency;

use super::fetch::fetch_modules;
use super::{
    BalanceSheetRow, CashflowRow, Earnings, EarningsQuarter, EarningsQuarterEps, EarningsYear,
    IncomeStatementRow,
};

#[derive(serde::Deserialize)]
struct TimeseriesValueF64 {
    #[serde(rename = "reportedValue")]
    reported_value: Option<RawNum<f64>>,
}

#[derive(serde::Deserialize)]
struct TimeseriesValueU64 {
    #[serde(rename = "reportedValue")]
    reported_value: Option<RawNumU64>,
}

/// Generic helper function to fetch and process timeseries data from the fundamentals API.
///
/// This function handles the common pattern of:
/// 1. Constructing the URL for the /ws/fundamentals-timeseries endpoint
/// 2. Making the request with caching logic
/// 3. Parsing the `TimeseriesEnvelope`
/// 4. Processing the data into a `BTreeMap`
///
/// The `process_item` closure is responsible for processing each timeseries item
/// and updating the rows map accordingly.
#[allow(clippy::too_many_arguments)]
async fn fetch_timeseries_data<T, F>(
    client: &YfClient,
    symbol: &str,
    quarterly: bool,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
    keys: &[&str],
    monetary_keys: &[&str],
    endpoint_name: &'static str,
    process_item: F,
) -> Result<YfResponse<Vec<T>>, YfError>
where
    F: Fn(
        &str,
        &serde_json::Value,
        &mut BTreeMap<i64, T>,
        &[i64],
        &str,
        Option<&ResolvedCurrencyUnit>,
        &mut ProjectionContext,
    ) -> Result<(), YfError>,
{
    let mut ctx = ProjectionContext::new(endpoint_name, data_quality);
    let prefix = if quarterly { "quarterly" } else { "annual" };
    let types: Vec<String> = keys.iter().map(|k| format!("{prefix}{k}")).collect();
    let type_str = types.join(",");

    let end_ts = Utc::now().timestamp();
    let start_ts = Utc::now()
        .checked_sub_signed(Duration::days(365 * 5))
        .map_or(0, |dt| dt.timestamp());

    let mut url = client.symbol_url(SymbolEndpoint::Timeseries, symbol)?;
    url.query_pairs_mut()
        .append_pair("symbol", symbol)
        .append_pair("type", &type_str)
        .append_pair("period1", &start_ts.to_string())
        .append_pair("period2", &end_ts.to_string());

    let endpoint = format!("timeseries_{endpoint_name}_{prefix}");
    let (body, _) = crate::core::net::fetch_text_with_auth_retry(
        client,
        url,
        crate::core::net::AuthFetchConfig {
            auth_mode: crate::core::net::AuthMode::RequiredCrumb,
            cache_endpoint: CacheEndpoint::Fundamentals,
            cache_mode,
            cache_body: None,
            retry_override,
            endpoint: &endpoint,
            fixture_key: symbol,
            ext: "json",
            retry_on_invalid_crumb_body: true,
        },
        |url| client.http().get(url),
    )
    .await?;

    let envelope: TimeseriesEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let result_vec = envelope
        .timeseries
        .and_then(|ts| ts.result)
        .unwrap_or_default();

    if result_vec.is_empty() {
        return Ok(ctx.finish(vec![]));
    }

    let (direct_currency, needs_currency) =
        timeseries_currency_evidence(&result_vec, prefix, monetary_keys)?;
    let currency = if needs_currency {
        match client
            .resolve_reporting_currency_unit(
                symbol,
                override_currency,
                ReportingCurrencyEvidence::TimeseriesCurrencyCode(direct_currency.as_deref()),
                cache_mode,
                retry_override,
            )
            .await
        {
            Ok(currency) => {
                ctx.currency_resolution(client, symbol, CurrencyKind::Reporting)
                    .await?;
                Some(currency)
            }
            Err(err @ YfError::InvalidData(_)) => return Err(err),
            Err(err) if data_quality == DataQuality::Strict => return Err(err),
            Err(_) => None,
        }
    } else {
        None
    };

    let mut rows_map = BTreeMap::<i64, T>::new();

    for item in result_vec {
        let Some(timestamps) = item.timestamp else {
            ctx.dropped_item(
                "timeseries_item",
                None,
                ProjectionIssue::MissingRequiredField { field: "timestamp" },
            )?;
            continue;
        };
        let Some((key, values_json)) = item.values.into_iter().next() else {
            ctx.dropped_item(
                "timeseries_item",
                None,
                ProjectionIssue::MissingRequiredField { field: "values" },
            )?;
            continue;
        };

        process_item(
            &key,
            &values_json,
            &mut rows_map,
            &timestamps,
            prefix,
            currency.as_ref(),
            &mut ctx,
        )?;
    }

    Ok(ctx.finish(rows_map.into_values().rev().collect()))
}

fn timeseries_currency_evidence(
    result: &[TimeseriesData],
    prefix: &str,
    monetary_keys: &[&str],
) -> Result<(Option<String>, bool), YfError> {
    let monetary_types = monetary_keys
        .iter()
        .map(|key| format!("{prefix}{key}"))
        .collect::<Vec<_>>();
    let mut currency_code: Option<String> = None;
    let mut needs_currency = false;

    for item in result {
        for (key, values_json) in &item.values {
            if !monetary_types
                .iter()
                .any(|monetary_key| monetary_key == key)
            {
                continue;
            }

            let Some(values) = values_json.as_array() else {
                continue;
            };

            for value in values {
                if value
                    .pointer("/reportedValue/raw")
                    .is_none_or(serde_json::Value::is_null)
                {
                    continue;
                }

                needs_currency = true;
                let Some(code) = value
                    .get("currencyCode")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|code| !code.is_empty())
                else {
                    continue;
                };

                if let Some(existing) = currency_code.as_deref()
                    && existing != code
                {
                    return Err(YfError::InvalidData(format!(
                        "conflicting timeseries currencyCode values for {key}: {existing} and {code}"
                    )));
                }

                currency_code.get_or_insert_with(|| code.to_string());
            }
        }
    }

    Ok((currency_code, needs_currency))
}

fn period_from_timestamp(timestamp: i64) -> Result<Period, YfError> {
    let date = i64_to_datetime(timestamp)?.format("%Y-%m-%d").to_string();
    string_to_period(&date)
}

fn parse_timeseries_values<T>(
    ctx: &mut ProjectionContext,
    key: &str,
    values_json: &serde_json::Value,
) -> Result<Option<Vec<T>>, YfError>
where
    T: serde::de::DeserializeOwned,
{
    match serde_json::from_value(values_json.clone()) {
        Ok(values) => Ok(Some(values)),
        Err(err) => {
            ctx.dropped_item(
                "timeseries_item",
                Some(key.to_string()),
                ProjectionIssue::InvalidField {
                    field: "values",
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
}

fn row_for_timestamp<T>(
    rows_map: &mut BTreeMap<i64, T>,
    timestamp: i64,
    create: impl FnOnce(Period) -> T,
) -> Result<&mut T, YfError> {
    match rows_map.entry(timestamp) {
        Entry::Occupied(entry) => Ok(entry.into_mut()),
        Entry::Vacant(entry) => {
            let period = period_from_timestamp(timestamp)?;
            Ok(entry.insert(create(period)))
        }
    }
}

pub(super) async fn income_statement(
    client: &YfClient,
    symbol: &str,
    quarterly: bool,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<IncomeStatementRow>>, YfError> {
    let keys = [
        "TotalRevenue",
        "GrossProfit",
        "OperatingIncome",
        "NetIncome",
        "InterestExpense",
        "TaxProvision",
        "DepreciationAndAmortization",
    ];
    let endpoint_name = "income_statement";

    let create_default_row = |period: Period| IncomeStatementRow {
        period,
        total_revenue: None,
        gross_profit: None,
        operating_income: None,
        net_income: None,
        interest_expense: None,
        income_tax_expense: None,
        depreciation_and_amortization: None,
    };

    let process_item = |key: &str,
                        values_json: &serde_json::Value,
                        rows_map: &mut BTreeMap<i64, IncomeStatementRow>,
                        timestamps: &[i64],
                        prefix: &str,
                        currency: Option<&ResolvedCurrencyUnit>,
                        ctx: &mut ProjectionContext|
     -> Result<(), YfError> {
        let Some(field) = key.strip_prefix(prefix) else {
            return Ok(());
        };

        let Some(values) = parse_timeseries_values::<TimeseriesValueF64>(ctx, key, values_json)?
        else {
            return Ok(());
        };
        for (i, ts) in timestamps.iter().enumerate() {
            let row = row_for_timestamp(rows_map, *ts, create_default_row)?;

            let value = values
                .get(i)
                .and_then(|v| v.reported_value.and_then(|rv| rv.raw));
            let money = optional_money_f64(
                ctx,
                "timeseries.reportedValue",
                Some(format!("{field}@{ts}")),
                currency,
                value,
                "statement monetary value",
            )?;

            match field {
                "TotalRevenue" => row.total_revenue = money,
                "GrossProfit" => row.gross_profit = money,
                "OperatingIncome" => row.operating_income = money,
                "NetIncome" => row.net_income = money,
                "InterestExpense" => row.interest_expense = money,
                "TaxProvision" => row.income_tax_expense = money,
                "DepreciationAndAmortization" => row.depreciation_and_amortization = money,
                _ => {}
            }
        }
        Ok(())
    };

    let result = fetch_timeseries_data(
        client,
        symbol,
        quarterly,
        override_currency,
        cache_mode,
        retry_override,
        data_quality,
        &keys,
        &keys,
        endpoint_name,
        process_item,
    )
    .await?;

    Ok(result)
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::cognitive_complexity)]
pub(super) async fn balance_sheet(
    client: &YfClient,
    symbol: &str,
    quarterly: bool,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<BalanceSheetRow>>, YfError> {
    let keys = [
        "TotalAssets",
        "TotalLiabilitiesNetMinorityInterest",
        "StockholdersEquity",
        "CashAndCashEquivalents",
        "LongTermDebt",
        "OrdinarySharesNumber",
        "CurrentAssets",
        "CurrentLiabilities",
        "AccountsReceivable",
        "Inventory",
        "AccountsPayable",
        "NetPPE",
        "Goodwill",
        "OtherIntangibleAssets",
    ];
    let monetary_keys = [
        "TotalAssets",
        "TotalLiabilitiesNetMinorityInterest",
        "StockholdersEquity",
        "CashAndCashEquivalents",
        "LongTermDebt",
        "CurrentAssets",
        "CurrentLiabilities",
        "AccountsReceivable",
        "Inventory",
        "AccountsPayable",
        "NetPPE",
        "Goodwill",
        "OtherIntangibleAssets",
    ];
    let endpoint_name = "balance_sheet";

    let create_default_row = |period: Period| BalanceSheetRow {
        period,
        total_assets: None,
        total_liabilities: None,
        total_equity: None,
        cash: None,
        long_term_debt: None,
        shares_outstanding: None,
        current_assets: None,
        current_liabilities: None,
        accounts_receivable: None,
        inventory: None,
        accounts_payable: None,
        net_property_plant_equipment: None,
        goodwill: None,
        intangible_assets: None,
    };

    let process_item = |key: &str,
                        values_json: &serde_json::Value,
                        rows_map: &mut BTreeMap<i64, BalanceSheetRow>,
                        timestamps: &[i64],
                        prefix: &str,
                        currency: Option<&ResolvedCurrencyUnit>,
                        ctx: &mut ProjectionContext|
     -> Result<(), YfError> {
        let Some(field) = key.strip_prefix(prefix) else {
            return Ok(());
        };

        if field == "OrdinarySharesNumber" {
            let Some(values) =
                parse_timeseries_values::<TimeseriesValueU64>(ctx, key, values_json)?
            else {
                return Ok(());
            };
            for (i, ts) in timestamps.iter().enumerate() {
                let row = row_for_timestamp(rows_map, *ts, create_default_row)?;
                row.shares_outstanding = values
                    .get(i)
                    .and_then(|v| v.reported_value.and_then(|rv| rv.raw));
            }
        } else {
            let Some(values) =
                parse_timeseries_values::<TimeseriesValueF64>(ctx, key, values_json)?
            else {
                return Ok(());
            };
            for (i, ts) in timestamps.iter().enumerate() {
                let row = row_for_timestamp(rows_map, *ts, create_default_row)?;

                let value = values
                    .get(i)
                    .and_then(|v| v.reported_value.and_then(|rv| rv.raw));
                let money = optional_money_f64(
                    ctx,
                    "timeseries.reportedValue",
                    Some(format!("{field}@{ts}")),
                    currency,
                    value,
                    "statement monetary value",
                )?;

                match field {
                    "TotalAssets" => row.total_assets = money,
                    "TotalLiabilitiesNetMinorityInterest" => row.total_liabilities = money,
                    "StockholdersEquity" => row.total_equity = money,
                    "CashAndCashEquivalents" => row.cash = money,
                    "LongTermDebt" => row.long_term_debt = money,
                    "CurrentAssets" => row.current_assets = money,
                    "CurrentLiabilities" => row.current_liabilities = money,
                    "AccountsReceivable" => row.accounts_receivable = money,
                    "Inventory" => row.inventory = money,
                    "AccountsPayable" => row.accounts_payable = money,
                    "NetPPE" => row.net_property_plant_equipment = money,
                    "Goodwill" => row.goodwill = money,
                    "OtherIntangibleAssets" => row.intangible_assets = money,
                    _ => {}
                }
            }
        }
        Ok(())
    };

    fetch_timeseries_data(
        client,
        symbol,
        quarterly,
        override_currency,
        cache_mode,
        retry_override,
        data_quality,
        &keys,
        &monetary_keys,
        endpoint_name,
        process_item,
    )
    .await
}

#[allow(clippy::too_many_lines)]
pub(super) async fn cashflow(
    client: &YfClient,
    symbol: &str,
    quarterly: bool,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<CashflowRow>>, YfError> {
    let keys = [
        "OperatingCashFlow",
        "CapitalExpenditure",
        "FreeCashFlow",
        "NetIncome",
        "DepreciationAndAmortization",
    ];
    let endpoint_name = "cash_flow";

    let create_default_row = |period: Period| CashflowRow {
        period,
        operating_cashflow: None,
        capital_expenditures: None,
        free_cash_flow: None,
        net_income: None,
        depreciation_and_amortization: None,
    };

    let process_item = |key: &str,
                        values_json: &serde_json::Value,
                        rows_map: &mut BTreeMap<i64, CashflowRow>,
                        timestamps: &[i64],
                        prefix: &str,
                        currency: Option<&ResolvedCurrencyUnit>,
                        ctx: &mut ProjectionContext|
     -> Result<(), YfError> {
        let Some(field) = key.strip_prefix(prefix) else {
            return Ok(());
        };

        let Some(values) = parse_timeseries_values::<TimeseriesValueF64>(ctx, key, values_json)?
        else {
            return Ok(());
        };
        for (i, ts) in timestamps.iter().enumerate() {
            let row = row_for_timestamp(rows_map, *ts, create_default_row)?;

            let value = values
                .get(i)
                .and_then(|v| v.reported_value.and_then(|rv| rv.raw));
            let money = optional_money_f64(
                ctx,
                "timeseries.reportedValue",
                Some(format!("{field}@{ts}")),
                currency,
                value,
                "statement monetary value",
            )?;

            match field {
                "OperatingCashFlow" => row.operating_cashflow = money,
                "CapitalExpenditure" => row.capital_expenditures = money,
                "FreeCashFlow" => row.free_cash_flow = money,
                "NetIncome" => row.net_income = money,
                "DepreciationAndAmortization" => row.depreciation_and_amortization = money,
                _ => {}
            }
        }
        Ok(())
    };

    let mut result = fetch_timeseries_data(
        client,
        symbol,
        quarterly,
        override_currency,
        cache_mode,
        retry_override,
        data_quality,
        &keys,
        &keys,
        endpoint_name,
        process_item,
    )
    .await?;

    // After filling values, calculate FCF if it's missing.
    for row in &mut result.data {
        if row.free_cash_flow.is_none()
            && let (Some(ocf), Some(capex)) = (
                row.operating_cashflow.clone(),
                row.capital_expenditures.clone(),
            )
        {
            // In timeseries API, capex is negative for cash outflow.
            row.free_cash_flow = ocf.try_add(&capex).ok();
        }
    }

    Ok(result)
}

fn earnings_has_monetary_values(earnings: &crate::fundamentals::wire::EarningsNode) -> bool {
    let raw_present = |value: Option<&crate::core::wire::RawNum<f64>>| {
        value.and_then(|v| v.raw.as_ref()).is_some()
    };

    earnings.financials_chart.as_ref().is_some_and(|chart| {
        chart.yearly.as_ref().is_some_and(|rows| {
            rows.iter()
                .any(|row| raw_present(row.revenue.as_ref()) || raw_present(row.earnings.as_ref()))
        }) || chart.quarterly.as_ref().is_some_and(|rows| {
            rows.iter()
                .any(|row| raw_present(row.revenue.as_ref()) || raw_present(row.earnings.as_ref()))
        })
    }) || earnings.earnings_chart.as_ref().is_some_and(|chart| {
        chart.quarterly.as_ref().is_some_and(|rows| {
            rows.iter()
                .any(|row| raw_present(row.actual.as_ref()) || raw_present(row.estimate.as_ref()))
        })
    })
}

#[allow(clippy::too_many_lines)]
pub(super) async fn earnings(
    client: &YfClient,
    symbol: &str,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Earnings>, YfError> {
    let mut ctx = ProjectionContext::new("earnings", data_quality);
    let root = fetch_modules(client, symbol, "earnings", cache_mode, retry_override).await?;
    let e = root
        .earnings
        .ok_or_else(|| YfError::MissingData("earnings missing".into()))?;
    client
        .store_currency_hints(
            symbol,
            CurrencyHints::from_quote_summary_financial(e.financial_currency.as_deref()),
        )
        .await;
    let currency = if earnings_has_monetary_values(&e) {
        match client
            .resolve_reporting_currency_unit(
                symbol,
                override_currency,
                ReportingCurrencyEvidence::FinancialCurrency(e.financial_currency.as_deref()),
                cache_mode,
                retry_override,
            )
            .await
        {
            Ok(currency) => {
                ctx.currency_resolution(client, symbol, CurrencyKind::Reporting)
                    .await?;
                Some(currency)
            }
            Err(err @ YfError::InvalidData(_)) => return Err(err),
            Err(err) if data_quality == DataQuality::Strict => return Err(err),
            Err(_) => None,
        }
    } else {
        None
    };

    let mut yearly = Vec::new();
    if let Some(rows) = e
        .financials_chart
        .as_ref()
        .and_then(|fc| fc.yearly.as_ref())
    {
        for y in rows {
            let Some(date) = y.date else {
                ctx.dropped_item(
                    "earnings_year",
                    None,
                    ProjectionIssue::MissingRequiredField { field: "date" },
                )?;
                continue;
            };
            let Ok(year) = i32::try_from(date) else {
                ctx.dropped_item(
                    "earnings_year",
                    Some(date.to_string()),
                    ProjectionIssue::InvalidField {
                        field: "date",
                        details: "year outside i32 range".into(),
                    },
                )?;
                continue;
            };
            yearly.push(EarningsYear {
                year,
                revenue: optional_money_f64(
                    &mut ctx,
                    "financialsChart.yearly[].revenue",
                    Some(year.to_string()),
                    currency.as_ref(),
                    y.revenue.as_ref().and_then(|x| x.raw),
                    "earnings monetary value",
                )?,
                earnings: optional_money_f64(
                    &mut ctx,
                    "financialsChart.yearly[].earnings",
                    Some(year.to_string()),
                    currency.as_ref(),
                    y.earnings.as_ref().and_then(|x| x.raw),
                    "earnings monetary value",
                )?,
            });
        }
    }

    let mut quarterly = Vec::new();
    if let Some(rows) = e
        .financials_chart
        .as_ref()
        .and_then(|fc| fc.quarterly.as_ref())
    {
        for q in rows {
            let Some(period_raw) = q.date.as_deref() else {
                ctx.dropped_item(
                    "earnings_quarter",
                    None,
                    ProjectionIssue::MissingRequiredField { field: "date" },
                )?;
                continue;
            };
            let period = match string_to_period(period_raw) {
                Ok(period) => period,
                Err(err) => {
                    ctx.dropped_item(
                        "earnings_quarter",
                        Some(period_raw.to_string()),
                        ProjectionIssue::InvalidField {
                            field: "date",
                            details: err.to_string(),
                        },
                    )?;
                    continue;
                }
            };
            quarterly.push(EarningsQuarter {
                period,
                revenue: optional_money_f64(
                    &mut ctx,
                    "financialsChart.quarterly[].revenue",
                    Some(period_raw.to_string()),
                    currency.as_ref(),
                    q.revenue.as_ref().and_then(|x| x.raw),
                    "earnings monetary value",
                )?,
                earnings: optional_money_f64(
                    &mut ctx,
                    "financialsChart.quarterly[].earnings",
                    Some(period_raw.to_string()),
                    currency.as_ref(),
                    q.earnings.as_ref().and_then(|x| x.raw),
                    "earnings monetary value",
                )?,
            });
        }
    }

    let mut quarterly_eps = Vec::new();
    if let Some(rows) = e
        .earnings_chart
        .as_ref()
        .and_then(|ec| ec.quarterly.as_ref())
    {
        for q in rows {
            let Some(period_raw) = q.date.as_deref() else {
                ctx.dropped_item(
                    "earnings_quarter_eps",
                    None,
                    ProjectionIssue::MissingRequiredField { field: "date" },
                )?;
                continue;
            };
            let period = match string_to_period(period_raw) {
                Ok(period) => period,
                Err(err) => {
                    ctx.dropped_item(
                        "earnings_quarter_eps",
                        Some(period_raw.to_string()),
                        ProjectionIssue::InvalidField {
                            field: "date",
                            details: err.to_string(),
                        },
                    )?;
                    continue;
                }
            };
            quarterly_eps.push(EarningsQuarterEps {
                period,
                actual: optional_price_f64(
                    &mut ctx,
                    "earningsChart.quarterly[].actual",
                    Some(period_raw.to_string()),
                    currency.as_ref(),
                    q.actual.as_ref().and_then(|x| x.raw),
                    "earnings price value",
                )?,
                estimate: optional_price_f64(
                    &mut ctx,
                    "earningsChart.quarterly[].estimate",
                    Some(period_raw.to_string()),
                    currency.as_ref(),
                    q.estimate.as_ref().and_then(|x| x.raw),
                    "earnings price value",
                )?,
            });
        }
    }

    Ok(ctx.finish(Earnings {
        yearly,
        quarterly,
        quarterly_eps,
    }))
}

pub(super) async fn calendar(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<super::Calendar>, YfError> {
    let ctx = ProjectionContext::new("calendar", data_quality);
    let root = fetch_modules(client, symbol, "calendarEvents", cache_mode, retry_override).await?;
    let calendar_events = root
        .calendar_events
        .ok_or_else(|| YfError::MissingData("calendarEvents missing".into()))?;

    let earnings_dates = calendar_events
        .earnings
        .and_then(|e| e.earnings_date)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|d| d.raw)
        .map(i64_to_datetime)
        .collect::<Result<Vec<_>, YfError>>()?;

    Ok(ctx.finish(super::Calendar {
        earnings_dates,
        ex_dividend_date: calendar_events
            .ex_dividend_date
            .and_then(|x| x.raw)
            .map(i64_to_datetime)
            .transpose()?,
        dividend_payment_date: calendar_events
            .dividend_date
            .and_then(|x| x.raw)
            .map(i64_to_datetime)
            .transpose()?,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn shares(
    client: &YfClient,
    symbol: &str,
    start: Option<chrono::DateTime<Utc>>,
    end: Option<chrono::DateTime<Utc>>,
    quarterly: bool,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<ShareCount>>, YfError> {
    let ctx = ProjectionContext::new("shares", data_quality);
    let end_ts = end.unwrap_or_else(Utc::now).timestamp();
    let start_ts = start
        .unwrap_or_else(|| Utc::now() - Duration::days(548))
        .timestamp();

    let type_key = if quarterly {
        "quarterlyBasicAverageShares"
    } else {
        "annualBasicAverageShares"
    };

    let mut url = client.symbol_url(SymbolEndpoint::Timeseries, symbol)?;
    url.query_pairs_mut()
        .append_pair("symbol", symbol)
        .append_pair("type", type_key)
        .append_pair("period1", &start_ts.to_string())
        .append_pair("period2", &end_ts.to_string());

    let endpoint = format!("timeseries_{type_key}");
    let (body, _) = crate::core::net::fetch_text_with_auth_retry(
        client,
        url,
        crate::core::net::AuthFetchConfig {
            auth_mode: crate::core::net::AuthMode::RequiredCrumb,
            cache_endpoint: CacheEndpoint::Fundamentals,
            cache_mode,
            cache_body: None,
            retry_override,
            endpoint: &endpoint,
            fixture_key: symbol,
            ext: "json",
            retry_on_invalid_crumb_body: true,
        },
        |url| client.http().get(url),
    )
    .await?;

    let envelope: TimeseriesEnvelope = serde_json::from_str(&body).map_err(YfError::Json)?;

    let result_data: Option<TimeseriesData> = envelope
        .timeseries
        .and_then(|ts| ts.result)
        .and_then(|mut v| v.pop());

    let Some(TimeseriesData {
        timestamp: Some(timestamps),
        values: mut values_map,
        ..
    }) = result_data
    else {
        return Ok(ctx.finish(vec![]));
    };

    let Some(values_json) = values_map.remove(type_key) else {
        return Ok(ctx.finish(vec![]));
    };

    let values: Vec<super::wire::TimeseriesValue> =
        serde_json::from_value(values_json).map_err(YfError::Json)?;

    let counts = timestamps
        .into_iter()
        .zip(values)
        .filter_map(|(ts, val)| {
            val.reported_value
                .and_then(|rv| rv.raw)
                .map(|shares| i64_to_datetime(ts).map(|date| ShareCount { date, shares }))
        })
        .collect::<Result<Vec<_>, YfError>>()?;

    Ok(ctx.finish(counts))
}
