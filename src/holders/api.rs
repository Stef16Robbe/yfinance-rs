use super::model::{
    InsiderRosterHolder, InsiderTransaction, InstitutionalHolder, MajorHolder,
    NetSharePurchaseActivity,
};
use super::wire::V10Result;
use crate::core::conversions::decimal_from_f64;
use crate::core::wire::{from_raw, from_raw_date};
use crate::core::{
    DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError, YfResponse,
    client::{CacheMode, RetryConfig},
    conversions::{i64_to_datetime, string_to_insider_position, string_to_transaction_type},
    currency_resolver::{ReportingCurrencyEvidence, ResolvedCurrencyUnit},
    diagnostics::optional_money_u64,
    quotesummary,
};
use paft::Decimal;
use paft::fundamentals::holders::{InsiderPosition, TransactionType};

const MODULES: &str = "institutionOwnership,fundOwnership,majorHoldersBreakdown,insiderTransactions,insiderHolders,netSharePurchaseActivity";

async fn fetch_holders_modules(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<V10Result, YfError> {
    quotesummary::fetch_module_result(
        client,
        symbol,
        MODULES,
        "holders",
        cache_mode,
        retry_override,
    )
    .await
}

pub(super) async fn major_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<MajorHolder>>, YfError> {
    let ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let breakdown = root
        .major_holders_breakdown
        .ok_or_else(|| YfError::MissingData("majorHoldersBreakdown missing".into()))?;

    let mut result = Vec::new();

    if let Some(v) = from_raw(breakdown.insiders_percent_held)
        && let Some(value) = decimal_from_f64(v)
    {
        result.push(MajorHolder {
            category: "% of Shares Held by All Insiders".into(),
            value,
        });
    }
    if let Some(v) = from_raw(breakdown.institutions_percent_held)
        && let Some(value) = decimal_from_f64(v)
    {
        result.push(MajorHolder {
            category: "% of Shares Held by Institutions".into(),
            value,
        });
    }
    if let Some(v) = from_raw(breakdown.institutions_float_percent_held)
        && let Some(value) = decimal_from_f64(v)
    {
        result.push(MajorHolder {
            category: "% of Float Held by Institutions".into(),
            value,
        });
    }
    if let Some(v) = from_raw(breakdown.institutions_count) {
        result.push(MajorHolder {
            category: "Number of Institutions Holding Shares".into(),
            value: Decimal::from(v),
        });
    }

    Ok(ctx.finish(result))
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

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

fn required_row_value<T>(
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, YfError>,
) -> Option<Result<T, YfError>> {
    match parse_optional(value, parse) {
        Ok(Some(parsed)) => Some(Ok(parsed)),
        Ok(None) => None,
        Err(err) => Some(Err(err)),
    }
}

fn required_parsed_row_value<T>(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<String>,
    field: &'static str,
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, YfError>,
) -> Result<Option<T>, YfError> {
    match required_row_value(value, parse) {
        Some(Ok(value)) => Ok(Some(value)),
        Some(Err(err)) => {
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
        None => {
            ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
            Ok(None)
        }
    }
}

fn required_date(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<String>,
    field: &'static str,
    value: Option<crate::core::wire::RawDate>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, YfError> {
    let Some(raw) = from_raw_date(value) else {
        ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
        return Ok(None);
    };
    match i64_to_datetime(raw) {
        Ok(value) => Ok(Some(value)),
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

async fn map_ownership_list(
    client: &YfClient,
    symbol: &str,
    node: Option<super::wire::OwnershipNode>,
    module_name: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    ctx: &mut ProjectionContext,
) -> Result<Vec<InstitutionalHolder>, YfError> {
    let holders = node
        .ok_or_else(|| YfError::MissingData(format!("{module_name} missing")))?
        .ownership_list
        .ok_or_else(|| YfError::MissingData(format!("{module_name}.ownershipList missing")))?;

    let currency = optional_reporting_currency(
        client,
        symbol,
        holders.iter().any(|h| from_raw(h.value).is_some()),
        cache_mode,
        retry_override,
        ctx,
    )
    .await?;

    let mut rows = Vec::new();
    for h in holders {
        let key = h.organization.as_deref().map(str::to_string);
        let Some(holder) = nonempty(h.organization) else {
            ctx.dropped_item(
                "institutional_holder",
                key,
                ProjectionIssue::MissingRequiredField {
                    field: "organization",
                },
            )?;
            continue;
        };
        let Some(date_reported) = required_date(
            ctx,
            "institutional_holder",
            Some(holder.clone()),
            "reportDate",
            h.date_reported,
        )?
        else {
            continue;
        };
        let value = optional_money_u64(
            ctx,
            "ownershipList[].value",
            Some(holder.clone()),
            currency.as_ref(),
            from_raw(h.value),
            "holder monetary value",
        )?;

        rows.push(InstitutionalHolder {
            holder,
            shares: from_raw(h.shares),
            date_reported,
            pct_held: from_raw(h.pct_held).and_then(decimal_from_f64),
            value,
        });
    }

    Ok(rows)
}

async fn resolve_reporting_currency(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<ResolvedCurrencyUnit, YfError> {
    client
        .resolve_reporting_currency_unit(
            symbol,
            None,
            ReportingCurrencyEvidence::None,
            cache_mode,
            retry_override,
        )
        .await
}

async fn optional_reporting_currency(
    client: &YfClient,
    symbol: &str,
    needed: bool,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    ctx: &ProjectionContext,
) -> Result<Option<ResolvedCurrencyUnit>, YfError> {
    if !needed {
        return Ok(None);
    }

    match resolve_reporting_currency(client, symbol, cache_mode, retry_override).await {
        Ok(currency) => Ok(Some(currency)),
        Err(err) if ctx.policy() == DataQuality::Strict => Err(err),
        Err(_) => Ok(None),
    }
}

pub(super) async fn institutional_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<InstitutionalHolder>>, YfError> {
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let rows = map_ownership_list(
        client,
        symbol,
        root.institution_ownership,
        "institutionOwnership",
        cache_mode,
        retry_override,
        &mut ctx,
    )
    .await?;
    Ok(ctx.finish(rows))
}

pub(super) async fn mutual_fund_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<InstitutionalHolder>>, YfError> {
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let rows = map_ownership_list(
        client,
        symbol,
        root.fund_ownership,
        "fundOwnership",
        cache_mode,
        retry_override,
        &mut ctx,
    )
    .await?;
    Ok(ctx.finish(rows))
}

pub(super) async fn insider_transactions(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<InsiderTransaction>>, YfError> {
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let transactions = root
        .insider_transactions
        .ok_or_else(|| YfError::MissingData("insiderTransactions missing".into()))?
        .transactions
        .ok_or_else(|| YfError::MissingData("insiderTransactions.transactions missing".into()))?;

    let currency = optional_reporting_currency(
        client,
        symbol,
        transactions.iter().any(|t| from_raw(t.value).is_some()),
        cache_mode,
        retry_override,
        &ctx,
    )
    .await?;

    let mut rows = Vec::new();
    for t in transactions {
        let key = t.insider.as_deref().map(str::to_string);
        let Some(insider) = nonempty(t.insider) else {
            ctx.dropped_item(
                "insider_transaction",
                key,
                ProjectionIssue::MissingRequiredField { field: "insider" },
            )?;
            continue;
        };
        let Some(position) = required_parsed_row_value::<InsiderPosition>(
            &mut ctx,
            "insider_transaction",
            Some(insider.clone()),
            "position",
            t.position.as_deref(),
            string_to_insider_position,
        )?
        else {
            continue;
        };
        let Some(transaction_type) = required_parsed_row_value::<TransactionType>(
            &mut ctx,
            "insider_transaction",
            Some(insider.clone()),
            "transaction",
            t.transaction.as_deref(),
            string_to_transaction_type,
        )?
        else {
            continue;
        };
        let Some(transaction_date) = required_date(
            &mut ctx,
            "insider_transaction",
            Some(insider.clone()),
            "startDate",
            t.start_date,
        )?
        else {
            continue;
        };
        let value = optional_money_u64(
            &mut ctx,
            "insiderTransactions.transactions[].value",
            Some(insider.clone()),
            currency.as_ref(),
            from_raw(t.value),
            "holder monetary value",
        )?;

        rows.push(InsiderTransaction {
            insider,
            position,
            transaction_type,
            shares: from_raw(t.shares),
            value,
            transaction_date,
            url: t.url.unwrap_or_default(),
        });
    }

    Ok(ctx.finish(rows))
}

pub(super) async fn insider_roster_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<InsiderRosterHolder>>, YfError> {
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let holders = root
        .insider_holders
        .ok_or_else(|| YfError::MissingData("insiderHolders missing".into()))?
        .holders
        .ok_or_else(|| YfError::MissingData("insiderHolders.holders missing".into()))?;

    let mut rows = Vec::new();
    for h in holders {
        let key = h.name.as_deref().map(str::to_string);
        let Some(name) = nonempty(h.name) else {
            ctx.dropped_item(
                "insider_roster_holder",
                key,
                ProjectionIssue::MissingRequiredField { field: "name" },
            )?;
            continue;
        };
        let Some(position) = required_parsed_row_value::<InsiderPosition>(
            &mut ctx,
            "insider_roster_holder",
            Some(name.clone()),
            "relation",
            h.relation.as_deref(),
            string_to_insider_position,
        )?
        else {
            continue;
        };
        let Some(most_recent_transaction) = required_parsed_row_value::<TransactionType>(
            &mut ctx,
            "insider_roster_holder",
            Some(name.clone()),
            "transactionDescription",
            h.most_recent_transaction.as_deref(),
            string_to_transaction_type,
        )?
        else {
            continue;
        };
        let Some(latest_transaction_date) = required_date(
            &mut ctx,
            "insider_roster_holder",
            Some(name.clone()),
            "latestTransDate",
            h.latest_transaction_date,
        )?
        else {
            continue;
        };
        let Some(position_direct_date) = required_date(
            &mut ctx,
            "insider_roster_holder",
            Some(name.clone()),
            "positionDirectDate",
            h.position_direct_date,
        )?
        else {
            continue;
        };

        rows.push(InsiderRosterHolder {
            name,
            position,
            most_recent_transaction,
            latest_transaction_date,
            shares_owned_directly: from_raw(h.shares_owned_directly),
            position_direct_date,
        });
    }

    Ok(ctx.finish(rows))
}

pub(super) async fn net_share_purchase_activity(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Option<NetSharePurchaseActivity>>, YfError> {
    let ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let data = root
        .net_share_purchase_activity
        .map(|n| {
            Ok::<_, YfError>(NetSharePurchaseActivity {
                period: crate::core::conversions::string_to_period(
                    n.period.as_deref().unwrap_or(""),
                )?,
                buy_shares: from_raw(n.buy_info_shares),
                buy_count: from_raw(n.buy_info_count),
                sell_shares: from_raw(n.sell_info_shares),
                sell_count: from_raw(n.sell_info_count),
                net_shares: from_raw(n.net_info_shares),
                net_count: from_raw(n.net_info_count),
                total_insider_shares: from_raw(n.total_insider_shares),
                net_percent_insider_shares: from_raw(n.net_percent_insider_shares)
                    .and_then(decimal_from_f64),
            })
        })
        .transpose()?;
    Ok(ctx.finish(data))
}
