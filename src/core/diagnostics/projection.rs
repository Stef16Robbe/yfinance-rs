use crate::core::{
    ProjectionContext, ProjectionIssue, YfError,
    conversions::{i64_to_date, i64_to_datetime, string_to_period},
    wire::{RawDate, RawDecimal, RawNum, RawNumU64, WireField, from_raw_date},
};
use chrono::{DateTime, NaiveDate, Utc};
use paft::domain::ReportingPeriod;

pub const fn diagnostic_key(key: Option<&str>) -> Option<&str> {
    key
}

pub fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub fn nonempty_string(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub fn required_nonempty_string(
    ctx: &mut ProjectionContext,
    item: &'static str,
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, YfError> {
    let Some(value) = value else {
        ctx.dropped_item(item, None, ProjectionIssue::MissingRequiredField { field })?;
        return Ok(None);
    };

    if value.trim().is_empty() {
        ctx.dropped_item(
            item,
            Some(value.as_str()),
            ProjectionIssue::MissingRequiredField { field },
        )?;
        return Ok(None);
    }

    Ok(Some(value))
}

pub fn optional_projected<T, U>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    value: Option<T>,
    project: impl FnOnce(T) -> Result<U, ProjectionIssue>,
) -> Result<Option<U>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };

    match project(value) {
        Ok(value) => Ok(Some(value)),
        Err(issue) => {
            ctx.omitted_present_field(path, key, issue)?;
            Ok(None)
        }
    }
}

pub trait WireProjection<T>: WireField<T> {
    fn optional_ref_field<'a>(
        &'a self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        field: &'static str,
    ) -> Result<Option<&'a T>, YfError> {
        optional_wire_value(ctx, path, key, field, self)
    }

    fn optional_copied(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
    ) -> Result<Option<T>, YfError>
    where
        T: Copy,
    {
        self.optional_copied_field(ctx, path, key, path)
    }

    fn optional_copied_field(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        field: &'static str,
    ) -> Result<Option<T>, YfError>
    where
        T: Copy,
    {
        optional_wire_copied(ctx, path, key, field, self)
    }

    fn optional_cloned(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
    ) -> Result<Option<T>, YfError>
    where
        T: Clone,
    {
        self.optional_cloned_field(ctx, path, key, path)
    }

    fn optional_cloned_field(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        field: &'static str,
    ) -> Result<Option<T>, YfError>
    where
        T: Clone,
    {
        optional_wire_cloned(ctx, path, key, field, self)
    }

    fn optional_copied_map<U>(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        map: impl FnOnce(T) -> U,
    ) -> Result<Option<U>, YfError>
    where
        T: Copy,
    {
        Ok(self.optional_copied(ctx, path, key)?.map(map))
    }

    fn optional_copied_and_then<U>(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        map: impl FnOnce(T) -> Option<U>,
    ) -> Result<Option<U>, YfError>
    where
        T: Copy,
    {
        self.optional_copied_and_then_field(ctx, path, key, path, map)
    }

    fn optional_copied_and_then_field<U>(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        field: &'static str,
        map: impl FnOnce(T) -> Option<U>,
    ) -> Result<Option<U>, YfError>
    where
        T: Copy,
    {
        Ok(self
            .optional_copied_field(ctx, path, key, field)?
            .and_then(map))
    }

    fn optional_raw_field(
        &self,
        ctx: &mut ProjectionContext,
        path: &'static str,
        key: Option<&str>,
        field: &'static str,
    ) -> Result<Option<T::Raw>, YfError>
    where
        T: Copy + WireRaw,
    {
        optional_wire_raw(ctx, path, key, field, self)
    }
}

impl<T, W> WireProjection<T> for W where W: WireField<T> + ?Sized {}

pub trait WireRaw {
    type Raw;

    fn raw(self) -> Option<Self::Raw>;
}

impl<T> WireRaw for RawNum<T> {
    type Raw = T;

    fn raw(self) -> Option<Self::Raw> {
        self.raw
    }
}

impl WireRaw for RawDecimal {
    type Raw = paft::Decimal;

    fn raw(self) -> Option<Self::Raw> {
        self.raw
    }
}

impl WireRaw for RawDate {
    type Raw = i64;

    fn raw(self) -> Option<Self::Raw> {
        self.raw
    }
}

impl WireRaw for RawNumU64 {
    type Raw = u64;

    fn raw(self) -> Option<Self::Raw> {
        self.raw
    }
}

pub fn optional_wire_value<'a, T, W>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &'a W,
) -> Result<Option<&'a T>, YfError>
where
    W: WireField<T> + ?Sized,
{
    if let Some(details) = value.invalid_details() {
        ctx.omitted_present_field(
            path,
            key,
            ProjectionIssue::InvalidField {
                field,
                details: details.to_string(),
            },
        )?;
    }

    Ok(value.as_ref())
}

pub fn optional_wire_copied<T: Copy, W>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &W,
) -> Result<Option<T>, YfError>
where
    W: WireField<T> + ?Sized,
{
    Ok(optional_wire_value(ctx, path, key, field, value)?.copied())
}

pub fn optional_wire_raw<T, W>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &W,
) -> Result<Option<T::Raw>, YfError>
where
    T: Copy + WireRaw,
    W: WireField<T> + ?Sized,
{
    Ok(optional_wire_copied(ctx, path, key, field, value)?.and_then(WireRaw::raw))
}

pub fn optional_wire_cloned<T: Clone, W>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &W,
) -> Result<Option<T>, YfError>
where
    W: WireField<T> + ?Sized,
{
    Ok(optional_wire_value(ctx, path, key, field, value)?.cloned())
}

pub fn required_wire_value<'a, T, W>(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &'a W,
) -> Result<Option<&'a T>, YfError>
where
    W: WireField<T> + ?Sized,
{
    if let Some(value) = value.as_ref() {
        return Ok(Some(value));
    }

    if let Some(details) = value.invalid_details() {
        ctx.dropped_item(
            item,
            key,
            ProjectionIssue::InvalidField {
                field,
                details: details.to_string(),
            },
        )?;
    } else {
        ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
    }

    Ok(None)
}

pub fn parse_optional<T>(
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, YfError>,
) -> Result<Option<T>, YfError> {
    nonempty(value).map(parse).transpose()
}

pub fn optional_parsed<T>(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, YfError>,
) -> Result<Option<T>, YfError> {
    optional_projected(ctx, path, key, nonempty(value), |value| {
        parse(value).map_err(|err| ProjectionIssue::InvalidField {
            field,
            details: err.to_string(),
        })
    })
}

pub fn required_parsed<T>(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: Option<&str>,
    parse: impl FnOnce(&str) -> Result<T, YfError>,
) -> Result<Option<T>, YfError> {
    let Some(value) = nonempty(value) else {
        ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
        return Ok(None);
    };

    match parse(value) {
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

pub fn required_period(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: Option<&str>,
) -> Result<Option<ReportingPeriod>, YfError> {
    required_parsed(ctx, item, key, field, value, string_to_period)
}

pub fn required_timestamp(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: Option<RawDate>,
) -> Result<Option<DateTime<Utc>>, YfError> {
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

pub fn required_wire_date<W>(
    ctx: &mut ProjectionContext,
    item: &'static str,
    key: Option<&str>,
    field: &'static str,
    value: &W,
) -> Result<Option<NaiveDate>, YfError>
where
    W: WireField<RawDate> + ?Sized,
{
    let Some(value) = required_wire_value(ctx, item, key, field, value)? else {
        return Ok(None);
    };
    let Some(raw) = value.raw else {
        ctx.dropped_item(item, key, ProjectionIssue::MissingRequiredField { field })?;
        return Ok(None);
    };

    match i64_to_date(raw) {
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
