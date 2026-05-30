use crate::core::{
    ProjectionContext, ProjectionIssue, YfError, conversions::decimal_from_f64,
    currency_resolver::ResolvedCurrencyUnit,
};
use paft::Decimal;

pub fn optional_decimal_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<Decimal>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(decimal) = decimal_from_f64(value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(decimal))
}

pub fn optional_money_u64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    value: Option<u64>,
    target: &'static str,
) -> Result<Option<paft::money::Money>, YfError> {
    optional_with_unit(ctx, path, key, unit, value, target, |unit, value| {
        unit.money_from_u64(value).ok()
    })
}

#[derive(Clone, Copy)]
struct CurrencyUnitContext<'a> {
    unit: Option<&'a ResolvedCurrencyUnit>,
    issue: Option<&'a ProjectionIssue>,
}

pub fn optional_money_i64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    value: Option<i64>,
    target: &'static str,
) -> Result<Option<paft::money::Money>, YfError> {
    optional_with_unit(ctx, path, key, unit, value, target, |unit, value| {
        unit.money_from_i64(value).ok()
    })
}

pub fn optional_money_decimal(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    value: Option<Decimal>,
    target: &'static str,
) -> Result<Option<paft::money::Money>, YfError> {
    optional_with_unit(ctx, path, key, unit, value, target, |unit, value| {
        unit.money_from_decimal(value).ok()
    })
}

pub fn optional_money_decimal_with_currency_issue(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    currency_issue: Option<&ProjectionIssue>,
    value: Option<Decimal>,
    target: &'static str,
) -> Result<Option<paft::money::Money>, YfError> {
    optional_with_unit_with_currency_issue(
        ctx,
        path,
        key,
        CurrencyUnitContext {
            unit,
            issue: currency_issue,
        },
        value,
        target,
        |unit, value| unit.money_from_decimal(value).ok(),
    )
}

pub fn optional_price_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<paft::money::Price>, YfError> {
    optional_with_unit(ctx, path, key, unit, value, target, |unit, value| {
        unit.price_from_f64(value)
    })
}

fn optional_with_unit<T, U>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    value: Option<T>,
    target: &'static str,
    convert: impl FnOnce(&ResolvedCurrencyUnit, T) -> Option<U>,
) -> Result<Option<U>, YfError> {
    optional_with_unit_with_currency_issue(
        ctx,
        path,
        key,
        CurrencyUnitContext { unit, issue: None },
        value,
        target,
        convert,
    )
}

fn optional_with_unit_with_currency_issue<T, U>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    currency: CurrencyUnitContext<'_>,
    value: Option<T>,
    target: &'static str,
    convert: impl FnOnce(&ResolvedCurrencyUnit, T) -> Option<U>,
) -> Result<Option<U>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(unit) = currency.unit else {
        ctx.omitted_present_field(
            path,
            key,
            currency
                .issue
                .cloned()
                .unwrap_or(ProjectionIssue::CurrencyUnresolved),
        )?;
        return Ok(None);
    };
    let Some(converted) = convert(unit, value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(converted))
}
