use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, btree_map::Entry};

use crate::{
    core::{
        DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError, YfResponse,
        client::{CacheEndpoint, CacheMode, RetryConfig, SymbolEndpoint, normalize_symbol},
        conversions::{i64_to_datetime, string_to_period},
        currency_resolver::{
            CurrencyHints, CurrencyKind, ReportingCurrencyEvidence, ResolvedCurrencyUnit,
        },
        diagnostics::{optional_money_decimal, optional_price_f64},
        wire::{RawDate, RawDecimal, RawNumU64},
    },
    fundamentals::wire::{TimeseriesData, TimeseriesEnvelope},
};
use paft::domain::Period;
use paft::fundamentals::profile::ShareCount;
use paft::money::{Currency, Money};
use url::Url;

use super::fetch::fetch_modules;
use super::{
    BalanceSheetRow, CashflowRow, Earnings, EarningsQuarter, EarningsQuarterEps, EarningsYear,
    IncomeStatementRow,
};

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
const STATEMENT_LOOKBACK_DAYS: i64 = 365 * 5;
const SHARE_COUNT_LOOKBACK_DAYS: i64 = 548;

#[derive(serde::Deserialize)]
struct TimeseriesValueDecimal {
    #[serde(rename = "reportedValue")]
    reported_value: Option<RawDecimal>,
}

#[derive(serde::Deserialize)]
struct TimeseriesValueU64 {
    #[serde(rename = "reportedValue")]
    reported_value: Option<RawNumU64>,
}

struct TimeseriesRequest<'a> {
    client: &'a YfClient,
    symbol: &'a str,
    quarterly: bool,
    override_currency: Option<Currency>,
    cache_mode: CacheMode,
    retry_override: Option<&'a RetryConfig>,
    data_quality: DataQuality,
    keys: &'a [&'static str],
    monetary_keys: &'a [&'static str],
    endpoint_name: &'static str,
}

struct TimeseriesItem<'a, T> {
    key: &'a str,
    values_json: &'a serde_json::Value,
    rows_map: &'a mut BTreeMap<i64, T>,
    timestamps: &'a [i64],
    prefix: &'a str,
    currency: Option<&'a ResolvedCurrencyUnit>,
    ctx: &'a mut ProjectionContext,
}

async fn fetch_timeseries_data<T, F>(
    request: TimeseriesRequest<'_>,
    mut process_item: F,
) -> Result<YfResponse<Vec<T>>, YfError>
where
    F: for<'a> FnMut(TimeseriesItem<'a, T>) -> Result<(), YfError>,
{
    let TimeseriesRequest {
        client,
        symbol,
        quarterly,
        override_currency,
        cache_mode,
        retry_override,
        data_quality,
        keys,
        monetary_keys,
        endpoint_name,
    } = request;
    let symbol = normalize_symbol(symbol)?;

    let mut ctx = ProjectionContext::new(endpoint_name, data_quality);
    let prefix = if quarterly { "quarterly" } else { "annual" };
    let url = timeseries_url(client, &symbol, prefix, keys)?;
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
            fixture_key: &symbol,
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
            .resolve_reporting_currency(
                &symbol,
                override_currency,
                ReportingCurrencyEvidence::TimeseriesCurrencyCode(direct_currency.as_deref()),
                cache_mode,
                retry_override,
            )
            .await
        {
            Ok(currency) => {
                ctx.currency_resolution(&symbol, CurrencyKind::Reporting, &currency)?;
                Some(currency.into_unit())
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

        process_item(TimeseriesItem {
            key: &key,
            values_json: &values_json,
            rows_map: &mut rows_map,
            timestamps: &timestamps,
            prefix,
            currency: currency.as_ref(),
            ctx: &mut ctx,
        })?;
    }

    Ok(ctx.finish(rows_map.into_values().rev().collect()))
}

fn timeseries_url(
    client: &YfClient,
    symbol: &str,
    prefix: &str,
    keys: &[&'static str],
) -> Result<Url, YfError> {
    let type_str = keys
        .iter()
        .map(|key| format!("{prefix}{key}"))
        .collect::<Vec<_>>()
        .join(",");
    let (start_ts, end_ts) = timeseries_window();

    let symbol = normalize_symbol(symbol)?;
    let mut url = client.symbol_url(SymbolEndpoint::Timeseries, &symbol)?;
    url.query_pairs_mut()
        .append_pair("symbol", &symbol)
        .append_pair("type", &type_str)
        .append_pair("period1", &start_ts.to_string())
        .append_pair("period2", &end_ts.to_string());

    Ok(url)
}

fn timeseries_window() -> (i64, i64) {
    window_ending_at_next_utc_midnight(Utc::now(), STATEMENT_LOOKBACK_DAYS)
}

fn shares_window(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> (i64, i64) {
    let end_ts = end.map_or_else(
        || next_utc_midnight_timestamp(Utc::now()),
        |dt| dt.timestamp(),
    );
    let start_ts = start.map_or_else(
        || timestamp_days_before(end_ts, SHARE_COUNT_LOOKBACK_DAYS),
        |dt| dt.timestamp(),
    );

    (start_ts, end_ts)
}

fn window_ending_at_next_utc_midnight(now: DateTime<Utc>, lookback_days: i64) -> (i64, i64) {
    let end_ts = next_utc_midnight_timestamp(now);
    let start_ts = timestamp_days_before(end_ts, lookback_days);

    (start_ts, end_ts)
}

const fn next_utc_midnight_timestamp(now: DateTime<Utc>) -> i64 {
    (now.timestamp().div_euclid(SECONDS_PER_DAY) + 1) * SECONDS_PER_DAY
}

fn timestamp_days_before(end_ts: i64, lookback_days: i64) -> i64 {
    DateTime::from_timestamp(end_ts, 0)
        .and_then(|end| end.checked_sub_signed(Duration::days(lookback_days)))
        .map_or(0, |start| start.timestamp())
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

fn row_for_timestamp<'a, T>(
    ctx: &mut ProjectionContext,
    rows_map: &'a mut BTreeMap<i64, T>,
    timestamp: i64,
    key: String,
    create: impl FnOnce(Period) -> T,
) -> Result<Option<&'a mut T>, YfError> {
    let period = match period_from_timestamp(timestamp) {
        Ok(period) => period,
        Err(err) => {
            ctx.dropped_item(
                "timeseries_row",
                Some(key),
                ProjectionIssue::InvalidField {
                    field: "timestamp",
                    details: err.to_string(),
                },
            )?;
            return Ok(None);
        }
    };

    match rows_map.entry(timestamp) {
        Entry::Occupied(entry) => Ok(Some(entry.into_mut())),
        Entry::Vacant(entry) => Ok(Some(entry.insert(create(period)))),
    }
}

fn process_statement_money_values<T>(
    item: TimeseriesItem<'_, T>,
    create_row: fn(Period) -> T,
    assign_money: fn(&mut T, &str, Option<Money>),
) -> Result<(), YfError> {
    let TimeseriesItem {
        key,
        values_json,
        rows_map,
        timestamps,
        prefix,
        currency,
        ctx,
    } = item;

    let Some(field) = key.strip_prefix(prefix) else {
        return Ok(());
    };

    let Some(values) = parse_timeseries_values::<TimeseriesValueDecimal>(ctx, key, values_json)?
    else {
        return Ok(());
    };

    for (idx, timestamp) in timestamps.iter().enumerate() {
        let row_key = format!("{field}@{timestamp}");
        let Some(row) = row_for_timestamp(ctx, rows_map, *timestamp, row_key.clone(), create_row)?
        else {
            continue;
        };

        let value = values
            .get(idx)
            .and_then(|value| value.reported_value.and_then(|reported| reported.raw));
        let money = optional_money_decimal(
            ctx,
            "timeseries.reportedValue",
            Some(row_key),
            currency,
            value,
            "statement monetary value",
        )?;

        assign_money(row, field, money);
    }

    Ok(())
}

fn process_statement_u64_values<T>(
    item: TimeseriesItem<'_, T>,
    create_row: fn(Period) -> T,
    assign_value: fn(&mut T, &str, Option<u64>),
) -> Result<(), YfError> {
    let TimeseriesItem {
        key,
        values_json,
        rows_map,
        timestamps,
        prefix,
        ctx,
        ..
    } = item;

    let Some(field) = key.strip_prefix(prefix) else {
        return Ok(());
    };

    let Some(values) = parse_timeseries_values::<TimeseriesValueU64>(ctx, key, values_json)? else {
        return Ok(());
    };

    for (idx, timestamp) in timestamps.iter().enumerate() {
        let Some(row) = row_for_timestamp(
            ctx,
            rows_map,
            *timestamp,
            format!("{field}@{timestamp}"),
            create_row,
        )?
        else {
            continue;
        };

        let value = values
            .get(idx)
            .and_then(|value| value.reported_value.and_then(|reported| reported.raw));
        assign_value(row, field, value);
    }

    Ok(())
}

const fn empty_income_statement_row(period: Period) -> IncomeStatementRow {
    IncomeStatementRow {
        period,
        total_revenue: None,
        gross_profit: None,
        operating_income: None,
        net_income: None,
        interest_expense: None,
        income_tax_expense: None,
        depreciation_and_amortization: None,
    }
}

fn assign_income_statement_money(row: &mut IncomeStatementRow, field: &str, money: Option<Money>) {
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

    let result = fetch_timeseries_data(
        TimeseriesRequest {
            client,
            symbol,
            quarterly,
            override_currency,
            cache_mode,
            retry_override,
            data_quality,
            keys: &keys,
            monetary_keys: &keys,
            endpoint_name,
        },
        |item| {
            process_statement_money_values(
                item,
                empty_income_statement_row,
                assign_income_statement_money,
            )
        },
    )
    .await?;

    Ok(result)
}

const fn empty_balance_sheet_row(period: Period) -> BalanceSheetRow {
    BalanceSheetRow {
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
    }
}

fn assign_balance_sheet_money(row: &mut BalanceSheetRow, field: &str, money: Option<Money>) {
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

fn assign_balance_sheet_shares(row: &mut BalanceSheetRow, field: &str, shares: Option<u64>) {
    if field == "OrdinarySharesNumber" {
        row.shares_outstanding = shares;
    }
}

fn process_balance_sheet_item(item: TimeseriesItem<'_, BalanceSheetRow>) -> Result<(), YfError> {
    let Some(field) = item.key.strip_prefix(item.prefix) else {
        return Ok(());
    };

    if field == "OrdinarySharesNumber" {
        process_statement_u64_values(item, empty_balance_sheet_row, assign_balance_sheet_shares)
    } else {
        process_statement_money_values(item, empty_balance_sheet_row, assign_balance_sheet_money)
    }
}

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

    fetch_timeseries_data(
        TimeseriesRequest {
            client,
            symbol,
            quarterly,
            override_currency,
            cache_mode,
            retry_override,
            data_quality,
            keys: &keys,
            monetary_keys: &monetary_keys,
            endpoint_name,
        },
        process_balance_sheet_item,
    )
    .await
}

const fn empty_cashflow_row(period: Period) -> CashflowRow {
    CashflowRow {
        period,
        operating_cashflow: None,
        capital_expenditures: None,
        free_cash_flow: None,
        net_income: None,
        depreciation_and_amortization: None,
    }
}

fn assign_cashflow_money(row: &mut CashflowRow, field: &str, money: Option<Money>) {
    match field {
        "OperatingCashFlow" => row.operating_cashflow = money,
        "CapitalExpenditure" => row.capital_expenditures = money,
        "FreeCashFlow" => row.free_cash_flow = money,
        "NetIncome" => row.net_income = money,
        "DepreciationAndAmortization" => row.depreciation_and_amortization = money,
        _ => {}
    }
}

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

    let mut result = fetch_timeseries_data(
        TimeseriesRequest {
            client,
            symbol,
            quarterly,
            override_currency,
            cache_mode,
            retry_override,
            data_quality,
            keys: &keys,
            monetary_keys: &keys,
            endpoint_name,
        },
        |item| process_statement_money_values(item, empty_cashflow_row, assign_cashflow_money),
    )
    .await?;

    let mut ctx = ProjectionContext::new(endpoint_name, data_quality);
    ctx.extend(result.diagnostics);

    // After filling values, calculate FCF if it's missing.
    for row in &mut result.data {
        if row.free_cash_flow.is_none()
            && let (Some(ocf), Some(capex)) = (
                row.operating_cashflow.clone(),
                row.capital_expenditures.clone(),
            )
        {
            // In timeseries API, capex is negative for cash outflow.
            if let Ok(free_cash_flow) = ocf.try_add(&capex) {
                ctx.repaired_data(
                    "cash_flow",
                    Some(row.period.to_string()),
                    "inferred missing free cash flow from operating cash flow and capital expenditure",
                )?;
                row.free_cash_flow = Some(free_cash_flow);
            }
        }
    }

    Ok(ctx.finish(result.data))
}

fn earnings_has_monetary_values(earnings: &crate::fundamentals::wire::EarningsNode) -> bool {
    let decimal_present = |value: Option<&crate::core::wire::RawDecimal>| {
        value.and_then(|v| v.raw.as_ref()).is_some()
    };
    let f64_present = |value: Option<&crate::core::wire::RawNum<f64>>| {
        value.and_then(|v| v.raw.as_ref()).is_some()
    };

    earnings.financials_chart.as_ref().is_some_and(|chart| {
        chart.yearly.as_ref().is_some_and(|rows| {
            rows.iter().any(|row| {
                decimal_present(row.revenue.as_ref()) || decimal_present(row.earnings.as_ref())
            })
        }) || chart.quarterly.as_ref().is_some_and(|rows| {
            rows.iter().any(|row| {
                decimal_present(row.revenue.as_ref()) || decimal_present(row.earnings.as_ref())
            })
        })
    }) || earnings.earnings_chart.as_ref().is_some_and(|chart| {
        chart.quarterly.as_ref().is_some_and(|rows| {
            rows.iter()
                .any(|row| f64_present(row.actual.as_ref()) || f64_present(row.estimate.as_ref()))
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
    let Some(e) = root.earnings else {
        ctx.unavailable_feature("earnings")?;
        return Ok(ctx.finish(Earnings::default()));
    };
    client
        .store_currency_hints(
            symbol,
            CurrencyHints::from_quote_summary_financial(e.financial_currency.as_deref()),
        )
        .await;
    let currency = if earnings_has_monetary_values(&e) {
        match client
            .resolve_reporting_currency(
                symbol,
                override_currency,
                ReportingCurrencyEvidence::FinancialCurrency(e.financial_currency.as_deref()),
                cache_mode,
                retry_override,
            )
            .await
        {
            Ok(currency) => {
                ctx.currency_resolution(symbol, CurrencyKind::Reporting, &currency)?;
                Some(currency.into_unit())
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
                revenue: optional_money_decimal(
                    &mut ctx,
                    "financialsChart.yearly[].revenue",
                    Some(year.to_string()),
                    currency.as_ref(),
                    y.revenue.as_ref().and_then(|x| x.raw),
                    "earnings monetary value",
                )?,
                earnings: optional_money_decimal(
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
                revenue: optional_money_decimal(
                    &mut ctx,
                    "financialsChart.quarterly[].revenue",
                    Some(period_raw.to_string()),
                    currency.as_ref(),
                    q.revenue.as_ref().and_then(|x| x.raw),
                    "earnings monetary value",
                )?,
                earnings: optional_money_decimal(
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
    let root = fetch_modules(client, symbol, "calendarEvents", cache_mode, retry_override).await?;
    map_calendar(root, data_quality)
}

pub(super) fn calendar_from_quote_summary_value(
    value: serde_json::Value,
    data_quality: DataQuality,
) -> Result<YfResponse<super::Calendar>, YfError> {
    let root: super::wire::V10Result = serde_json::from_value(value).map_err(YfError::Json)?;
    map_calendar(root, data_quality)
}

fn map_calendar(
    root: super::wire::V10Result,
    data_quality: DataQuality,
) -> Result<YfResponse<super::Calendar>, YfError> {
    let mut ctx = ProjectionContext::new("calendar", data_quality);
    let Some(calendar_events) = root.calendar_events else {
        ctx.unavailable_feature("calendarEvents")?;
        return Ok(ctx.finish(super::Calendar {
            earnings_dates: Vec::new(),
            ex_dividend_date: None,
            dividend_payment_date: None,
        }));
    };

    let earnings_dates = calendar_earnings_dates(
        &mut ctx,
        calendar_events.earnings.and_then(|e| e.earnings_date),
    )?;
    let ex_dividend_date = optional_calendar_date(
        &mut ctx,
        "calendarEvents.exDividendDate",
        "exDividendDate",
        calendar_events.ex_dividend_date,
    )?;
    let dividend_payment_date = optional_calendar_date(
        &mut ctx,
        "calendarEvents.dividendDate",
        "dividendDate",
        calendar_events.dividend_date,
    )?;

    Ok(ctx.finish(super::Calendar {
        earnings_dates,
        ex_dividend_date,
        dividend_payment_date,
    }))
}

fn calendar_earnings_dates(
    ctx: &mut ProjectionContext,
    dates: Option<Vec<RawDate>>,
) -> Result<Vec<DateTime<Utc>>, YfError> {
    let mut out = Vec::new();
    for (idx, date) in dates.unwrap_or_default().into_iter().enumerate() {
        let key = Some(idx.to_string());
        let Some(raw) = date.raw else {
            ctx.dropped_item(
                "calendar_earnings_date",
                key,
                ProjectionIssue::MissingRequiredField {
                    field: "earningsDate",
                },
            )?;
            continue;
        };
        match i64_to_datetime(raw) {
            Ok(date) => out.push(date),
            Err(err) => {
                ctx.dropped_item(
                    "calendar_earnings_date",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "earningsDate",
                        details: err.to_string(),
                    },
                )?;
            }
        }
    }
    Ok(out)
}

fn optional_calendar_date(
    ctx: &mut ProjectionContext,
    path: &'static str,
    field: &'static str,
    value: Option<RawDate>,
) -> Result<Option<DateTime<Utc>>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(raw) = value.raw else {
        ctx.omitted_present_field(path, None, ProjectionIssue::MissingRequiredField { field })?;
        return Ok(None);
    };
    match i64_to_datetime(raw) {
        Ok(date) => Ok(Some(date)),
        Err(err) => {
            ctx.omitted_present_field(
                path,
                None,
                ProjectionIssue::InvalidField {
                    field,
                    details: err.to_string(),
                },
            )?;
            Ok(None)
        }
    }
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
    let symbol = normalize_symbol(symbol)?;
    let mut ctx = ProjectionContext::new("shares", data_quality);
    let (start_ts, end_ts) = shares_window(start, end);

    let type_key = if quarterly {
        "quarterlyBasicAverageShares"
    } else {
        "annualBasicAverageShares"
    };

    let mut url = client.symbol_url(SymbolEndpoint::Timeseries, &symbol)?;
    url.query_pairs_mut()
        .append_pair("symbol", &symbol)
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
            fixture_key: &symbol,
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

    let values: Vec<super::wire::TimeseriesValue> = match serde_json::from_value(values_json) {
        Ok(values) => values,
        Err(err) => {
            ctx.dropped_item(
                "share_count",
                Some(type_key.to_string()),
                ProjectionIssue::InvalidField {
                    field: "values",
                    details: err.to_string(),
                },
            )?;
            return Ok(ctx.finish(Vec::new()));
        }
    };

    let mut counts = Vec::new();
    for (ts, val) in timestamps.into_iter().zip(values) {
        let Some(shares) = val.reported_value.and_then(|rv| rv.raw) else {
            continue;
        };
        let date = match i64_to_datetime(ts) {
            Ok(date) => date,
            Err(err) => {
                ctx.dropped_item(
                    "share_count",
                    Some(format!("{type_key}@{ts}")),
                    ProjectionIssue::InvalidField {
                        field: "timestamp",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        counts.push(ShareCount { date, shares });
    }

    Ok(ctx.finish(counts))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::{
        SECONDS_PER_DAY, SHARE_COUNT_LOOKBACK_DAYS, STATEMENT_LOOKBACK_DAYS,
        next_utc_midnight_timestamp, shares_window, window_ending_at_next_utc_midnight,
    };

    #[test]
    fn statement_window_is_stable_within_utc_day() {
        let morning = Utc.with_ymd_and_hms(2026, 5, 28, 1, 2, 3).unwrap();
        let evening = Utc.with_ymd_and_hms(2026, 5, 28, 23, 59, 59).unwrap();
        let expected_end = Utc.with_ymd_and_hms(2026, 5, 29, 0, 0, 0).unwrap();

        let morning_window = window_ending_at_next_utc_midnight(morning, STATEMENT_LOOKBACK_DAYS);
        let evening_window = window_ending_at_next_utc_midnight(evening, STATEMENT_LOOKBACK_DAYS);

        assert_eq!(morning_window, evening_window);
        assert_eq!(morning_window.1, expected_end.timestamp());
        assert_eq!(
            morning_window.1 - morning_window.0,
            STATEMENT_LOOKBACK_DAYS * SECONDS_PER_DAY
        );
    }

    #[test]
    fn next_utc_midnight_advances_at_utc_boundary() {
        let before_midnight = Utc.with_ymd_and_hms(2026, 5, 28, 23, 59, 59).unwrap();
        let at_midnight = Utc.with_ymd_and_hms(2026, 5, 29, 0, 0, 0).unwrap();

        assert_eq!(
            next_utc_midnight_timestamp(before_midnight),
            at_midnight.timestamp()
        );
        assert_eq!(
            next_utc_midnight_timestamp(at_midnight),
            Utc.with_ymd_and_hms(2026, 5, 30, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn shares_window_defaults_start_from_effective_end() {
        let end = Utc.with_ymd_and_hms(2026, 5, 28, 12, 34, 56).unwrap();
        let expected_start = end - Duration::days(SHARE_COUNT_LOOKBACK_DAYS);

        let (start_ts, end_ts) = shares_window(None, Some(end));

        assert_eq!(end_ts, end.timestamp());
        assert_eq!(start_ts, expected_start.timestamp());
    }

    #[test]
    fn shares_window_respects_explicit_start() {
        let start = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 5, 28, 12, 34, 56).unwrap();

        let (start_ts, end_ts) = shares_window(Some(start), Some(end));

        assert_eq!(start_ts, start.timestamp());
        assert_eq!(end_ts, end.timestamp());
    }
}
