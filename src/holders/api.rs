use super::model::{
    InsiderRosterHolder, InsiderTransaction, InstitutionalHolder, MajorHolder,
    NetSharePurchaseActivity,
};
use super::wire::{
    InsiderRosterHolderNode, InsiderTransactionNode, InstitutionalHolderNode, OwnershipNode,
    V10Result,
};
use crate::core::wire::{from_raw, from_raw_date};
use crate::core::{
    DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError, YfResponse,
    client::{CacheMode, RetryConfig},
    conversions::{i64_to_datetime, string_to_insider_position, string_to_transaction_type},
    currency_resolver::{CurrencyKind, ReportingCurrencyEvidence, ResolvedCurrencyUnit},
    diagnostics::{optional_decimal_f64, optional_money_u64},
    quotesummary,
};
use paft::Decimal;
use paft::fundamentals::holders::{InsiderPosition, TransactionType};

const INSTITUTION_OWNERSHIP_MODULE: &str = "institutionOwnership";
const FUND_OWNERSHIP_MODULE: &str = "fundOwnership";
const MAJOR_HOLDERS_MODULE: &str = "majorHoldersBreakdown";
const INSIDER_TRANSACTIONS_MODULE: &str = "insiderTransactions";
const INSIDER_HOLDERS_MODULE: &str = "insiderHolders";
const NET_SHARE_PURCHASE_ACTIVITY_MODULE: &str = "netSharePurchaseActivity";
const INSTITUTION_OWNERSHIP: OwnershipFeatureNames = OwnershipFeatureNames {
    module: "institutionOwnership",
    list: "institutionOwnership.ownershipList",
};
const FUND_OWNERSHIP: OwnershipFeatureNames = OwnershipFeatureNames {
    module: "fundOwnership",
    list: "fundOwnership.ownershipList",
};

#[derive(Clone, Copy)]
struct OwnershipFeatureNames {
    module: &'static str,
    list: &'static str,
}

async fn fetch_holders_modules(
    client: &YfClient,
    symbol: &str,
    modules: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<V10Result, YfError> {
    quotesummary::fetch_module_result(
        client,
        symbol,
        modules,
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
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(
        client,
        symbol,
        MAJOR_HOLDERS_MODULE,
        cache_mode,
        retry_override,
    )
    .await?;
    let Some(breakdown) = root.major_holders_breakdown else {
        ctx.unavailable_feature("majorHoldersBreakdown")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let mut result = Vec::new();

    if let Some(value) = optional_decimal_f64(
        &mut ctx,
        "majorHoldersBreakdown.insidersPercentHeld",
        None,
        from_raw(breakdown.insiders_percent_held),
        "major holder percent",
    )? {
        result.push(MajorHolder {
            category: "% of Shares Held by All Insiders".into(),
            value,
        });
    }
    if let Some(value) = optional_decimal_f64(
        &mut ctx,
        "majorHoldersBreakdown.institutionsPercentHeld",
        None,
        from_raw(breakdown.institutions_percent_held),
        "major holder percent",
    )? {
        result.push(MajorHolder {
            category: "% of Shares Held by Institutions".into(),
            value,
        });
    }
    if let Some(value) = optional_decimal_f64(
        &mut ctx,
        "majorHoldersBreakdown.institutionsFloatPercentHeld",
        None,
        from_raw(breakdown.institutions_float_percent_held),
        "major holder percent",
    )? {
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

fn holder_diag_key(
    value: &serde_json::Value,
    key_field: &'static str,
    fallback: &'static str,
    idx: usize,
) -> String {
    value
        .get(key_field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map_or_else(|| format!("{fallback}[{idx}]"), ToString::to_string)
}

fn raw_field_present(value: &serde_json::Value, field: &'static str) -> bool {
    value
        .get(field)
        .and_then(|value| value.get("raw"))
        .is_some_and(|value| !value.is_null())
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
    node: Option<OwnershipNode>,
    features: OwnershipFeatureNames,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    ctx: &mut ProjectionContext,
) -> Result<Vec<InstitutionalHolder>, YfError> {
    let Some(holders) = ownership_list_or_unavailable(node, features, ctx)? else {
        return Ok(Vec::new());
    };

    let currency = optional_reporting_currency(
        client,
        symbol,
        holders.iter().any(|h| raw_field_present(h, "value")),
        cache_mode,
        retry_override,
        ctx,
    )
    .await?;

    let mut rows = Vec::new();
    for (idx, h) in holders.into_iter().enumerate() {
        let key = Some(holder_diag_key(&h, "organization", "ownershipList", idx));
        let h = match serde_json::from_value::<InstitutionalHolderNode>(h) {
            Ok(holder) => holder,
            Err(err) => {
                ctx.dropped_item(
                    "institutional_holder",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "holder",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
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
        let pct_held = optional_decimal_f64(
            ctx,
            "ownershipList[].pctHeld",
            Some(holder.clone()),
            from_raw(h.pct_held),
            "holder percent",
        )?;

        rows.push(InstitutionalHolder {
            holder,
            shares: from_raw(h.shares),
            date_reported,
            pct_held,
            value,
        });
    }

    Ok(rows)
}

fn ownership_list_or_unavailable(
    node: Option<OwnershipNode>,
    features: OwnershipFeatureNames,
    ctx: &mut ProjectionContext,
) -> Result<Option<Vec<serde_json::Value>>, YfError> {
    let Some(node) = node else {
        ctx.unavailable_feature(features.module)?;
        return Ok(None);
    };
    let Some(holders) = node.ownership_list else {
        ctx.unavailable_feature(features.list)?;
        return Ok(None);
    };

    Ok(Some(holders))
}

async fn resolve_reporting_currency(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<crate::core::currency_resolver::ResolvedCurrency, YfError> {
    client
        .resolve_reporting_currency(
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
    ctx: &mut ProjectionContext,
) -> Result<Option<ResolvedCurrencyUnit>, YfError> {
    if !needed {
        return Ok(None);
    }

    match resolve_reporting_currency(client, symbol, cache_mode, retry_override).await {
        Ok(currency) => {
            ctx.currency_resolution(symbol, CurrencyKind::Reporting, &currency)?;
            Ok(Some(currency.into_unit()))
        }
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
    let root = fetch_holders_modules(
        client,
        symbol,
        INSTITUTION_OWNERSHIP_MODULE,
        cache_mode,
        retry_override,
    )
    .await?;
    let rows = map_ownership_list(
        client,
        symbol,
        root.institution_ownership,
        INSTITUTION_OWNERSHIP,
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
    let root = fetch_holders_modules(
        client,
        symbol,
        FUND_OWNERSHIP_MODULE,
        cache_mode,
        retry_override,
    )
    .await?;
    let rows = map_ownership_list(
        client,
        symbol,
        root.fund_ownership,
        FUND_OWNERSHIP,
        cache_mode,
        retry_override,
        &mut ctx,
    )
    .await?;
    Ok(ctx.finish(rows))
}

#[allow(clippy::too_many_lines)]
pub(super) async fn insider_transactions(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<Vec<InsiderTransaction>>, YfError> {
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(
        client,
        symbol,
        INSIDER_TRANSACTIONS_MODULE,
        cache_mode,
        retry_override,
    )
    .await?;
    let Some(insider_transactions) = root.insider_transactions else {
        ctx.unavailable_feature("insiderTransactions")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(transactions) = insider_transactions.transactions else {
        ctx.unavailable_feature("insiderTransactions.transactions")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let currency = optional_reporting_currency(
        client,
        symbol,
        transactions.iter().any(|t| raw_field_present(t, "value")),
        cache_mode,
        retry_override,
        &mut ctx,
    )
    .await?;

    let mut rows = Vec::new();
    for (idx, t) in transactions.into_iter().enumerate() {
        let key = Some(holder_diag_key(&t, "filerName", "transactions", idx));
        let t = match serde_json::from_value::<InsiderTransactionNode>(t) {
            Ok(transaction) => transaction,
            Err(err) => {
                ctx.dropped_item(
                    "insider_transaction",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "transaction",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
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
    let root = fetch_holders_modules(
        client,
        symbol,
        INSIDER_HOLDERS_MODULE,
        cache_mode,
        retry_override,
    )
    .await?;
    let Some(insider_holders) = root.insider_holders else {
        ctx.unavailable_feature("insiderHolders")?;
        return Ok(ctx.finish(Vec::new()));
    };
    let Some(holders) = insider_holders.holders else {
        ctx.unavailable_feature("insiderHolders.holders")?;
        return Ok(ctx.finish(Vec::new()));
    };

    let mut rows = Vec::new();
    for (idx, h) in holders.into_iter().enumerate() {
        let key = Some(holder_diag_key(&h, "name", "holders", idx));
        let h = match serde_json::from_value::<InsiderRosterHolderNode>(h) {
            Ok(holder) => holder,
            Err(err) => {
                ctx.dropped_item(
                    "insider_roster_holder",
                    key,
                    ProjectionIssue::InvalidField {
                        field: "holder",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
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
    let mut ctx = ProjectionContext::new("holders", data_quality);
    let root = fetch_holders_modules(
        client,
        symbol,
        NET_SHARE_PURCHASE_ACTIVITY_MODULE,
        cache_mode,
        retry_override,
    )
    .await?;
    let Some(n) = root.net_share_purchase_activity else {
        ctx.unavailable_feature("netSharePurchaseActivity")?;
        return Ok(ctx.finish(None));
    };

    let period_key = n.period.clone();
    let Some(period) = required_parsed_row_value(
        &mut ctx,
        "net_share_purchase_activity",
        period_key.clone(),
        "period",
        n.period.as_deref(),
        crate::core::conversions::string_to_period,
    )?
    else {
        return Ok(ctx.finish(None));
    };

    let data = Some(NetSharePurchaseActivity {
        period,
        buy_shares: from_raw(n.buy_info_shares),
        buy_count: from_raw(n.buy_info_count),
        sell_shares: from_raw(n.sell_info_shares),
        sell_count: from_raw(n.sell_info_count),
        net_shares: from_raw(n.net_info_shares),
        net_count: from_raw(n.net_info_count),
        total_insider_shares: from_raw(n.total_insider_shares),
        net_percent_insider_shares: optional_decimal_f64(
            &mut ctx,
            "netSharePurchaseActivity.netPercentInsiderShares",
            period_key,
            from_raw(n.net_percent_insider_shares),
            "net percent insider shares",
        )?,
    });
    Ok(ctx.finish(data))
}
