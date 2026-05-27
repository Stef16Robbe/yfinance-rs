use super::model::{
    InsiderRosterHolder, InsiderTransaction, InstitutionalHolder, MajorHolder,
    NetSharePurchaseActivity,
};
use super::wire::V10Result;
use crate::core::conversions::decimal_from_f64;
use crate::core::wire::{from_raw, from_raw_date};
use crate::core::{
    YfClient, YfError,
    client::{CacheMode, RetryConfig},
    conversions::{
        i64_to_datetime, string_to_insider_position, string_to_transaction_type,
        u64_to_money_with_currency,
    },
    quotesummary,
};
use paft::Decimal;
use paft::fundamentals::holders::{InsiderPosition, TransactionType};
use paft::money::Currency;

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
) -> Result<Vec<MajorHolder>, YfError> {
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

    Ok(result)
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

fn map_ownership_list(
    node: Option<super::wire::OwnershipNode>,
    module_name: &str,
    currency: &Currency,
) -> Result<Vec<InstitutionalHolder>, YfError> {
    node.ok_or_else(|| YfError::MissingData(format!("{module_name} missing")))?
        .ownership_list
        .ok_or_else(|| YfError::MissingData(format!("{module_name}.ownershipList missing")))?
        .into_iter()
        .filter_map(|h| {
            let holder = nonempty(h.organization)?;
            let date_reported =
                from_raw_date(h.date_reported).and_then(|ts| i64_to_datetime(ts).ok())?;
            let value = match from_raw(h.value)
                .map(|v| u64_to_money_with_currency(v, currency.clone()))
                .transpose()
            {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };

            Some(Ok(InstitutionalHolder {
                holder,
                shares: from_raw(h.shares),
                date_reported,
                pct_held: from_raw(h.pct_held).and_then(decimal_from_f64),
                value,
            }))
        })
        .collect()
}

pub(super) async fn institutional_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<InstitutionalHolder>, YfError> {
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let currency = client.reporting_currency(symbol, None).await;
    map_ownership_list(
        root.institution_ownership,
        "institutionOwnership",
        &currency,
    )
}

pub(super) async fn mutual_fund_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<InstitutionalHolder>, YfError> {
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let currency = client.reporting_currency(symbol, None).await;
    map_ownership_list(root.fund_ownership, "fundOwnership", &currency)
}

pub(super) async fn insider_transactions(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<InsiderTransaction>, YfError> {
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let currency = client.reporting_currency(symbol, None).await;
    let transactions = root
        .insider_transactions
        .ok_or_else(|| YfError::MissingData("insiderTransactions missing".into()))?
        .transactions
        .ok_or_else(|| YfError::MissingData("insiderTransactions.transactions missing".into()))?;

    transactions
        .into_iter()
        .filter_map(|t| {
            let insider = nonempty(t.insider)?;
            let position = match required_row_value::<InsiderPosition>(
                t.position.as_deref(),
                string_to_insider_position,
            )? {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let transaction_type = match required_row_value::<TransactionType>(
                t.transaction.as_deref(),
                string_to_transaction_type,
            )? {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let transaction_date =
                from_raw_date(t.start_date).and_then(|ts| i64_to_datetime(ts).ok())?;
            let value = match from_raw(t.value)
                .map(|v| u64_to_money_with_currency(v, currency.clone()))
                .transpose()
            {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };

            Some(Ok(InsiderTransaction {
                insider,
                position,
                transaction_type,
                shares: from_raw(t.shares),
                value,
                transaction_date,
                url: t.url.unwrap_or_default(),
            }))
        })
        .collect()
}

pub(super) async fn insider_roster_holders(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Vec<InsiderRosterHolder>, YfError> {
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    let holders = root
        .insider_holders
        .ok_or_else(|| YfError::MissingData("insiderHolders missing".into()))?
        .holders
        .ok_or_else(|| YfError::MissingData("insiderHolders.holders missing".into()))?;

    holders
        .into_iter()
        .filter_map(|h| {
            let name = nonempty(h.name)?;
            let position = match required_row_value::<InsiderPosition>(
                h.relation.as_deref(),
                string_to_insider_position,
            )? {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let most_recent_transaction = match required_row_value::<TransactionType>(
                h.most_recent_transaction.as_deref(),
                string_to_transaction_type,
            )? {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let latest_transaction_date =
                from_raw_date(h.latest_transaction_date).and_then(|ts| i64_to_datetime(ts).ok())?;
            let position_direct_date =
                from_raw_date(h.position_direct_date).and_then(|ts| i64_to_datetime(ts).ok())?;

            Some(Ok(InsiderRosterHolder {
                name,
                position,
                most_recent_transaction,
                latest_transaction_date,
                shares_owned_directly: from_raw(h.shares_owned_directly),
                position_direct_date,
            }))
        })
        .collect()
}

pub(super) async fn net_share_purchase_activity(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
) -> Result<Option<NetSharePurchaseActivity>, YfError> {
    let root = fetch_holders_modules(client, symbol, cache_mode, retry_override).await?;
    root.net_share_purchase_activity
        .map(|n| {
            Ok(NetSharePurchaseActivity {
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
        .transpose()
}
