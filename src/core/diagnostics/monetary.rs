use crate::core::{
    ProjectionContext, ProjectionIssue, YfError, currency_resolver::ResolvedCurrencyUnit,
};

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

pub fn optional_money_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    unit: Option<&ResolvedCurrencyUnit>,
    value: Option<f64>,
    target: &'static str,
) -> Result<Option<paft::money::Money>, YfError> {
    optional_with_unit(ctx, path, key, unit, value, target, |unit, value| {
        unit.money_from_f64(value)
    })
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
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(unit) = unit else {
        ctx.omitted_present_field(path, key, ProjectionIssue::CurrencyUnresolved)?;
        return Ok(None);
    };
    let Some(converted) = convert(unit, value) else {
        ctx.omitted_present_field(path, key, ProjectionIssue::ConversionFailed { target })?;
        return Ok(None);
    };
    Ok(Some(converted))
}
