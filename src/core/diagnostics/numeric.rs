use crate::core::{ProjectionContext, ProjectionIssue, YfError, wire::RawNum};

pub fn optional_u32_from_i64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    field: &'static str,
    value: Option<i64>,
) -> Result<Option<u32>, YfError> {
    let Some(value) = value else {
        return Ok(None);
    };

    if let Ok(value) = u32::try_from(value) {
        Ok(Some(value))
    } else {
        ctx.omitted_present_field(path, key, invalid_u32_count(field, value))?;
        Ok(None)
    }
}

pub fn optional_u32_from_raw_f64(
    ctx: &mut ProjectionContext,
    path: &'static str,
    key: Option<String>,
    field: &'static str,
    value: Option<RawNum<f64>>,
) -> Result<Option<u32>, YfError> {
    let Some(value) = value.and_then(|value| value.raw) else {
        return Ok(None);
    };

    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        ctx.omitted_present_field(path, key, invalid_u32_count(field, value))?;
        return Ok(None);
    }

    let rounded = value.round();
    if value.fract() != 0.0 {
        ctx.coerced_present_field(
            path,
            key,
            format!("rounded non-integer count {value} to {rounded}"),
        )?;
    }

    // This cast is safe because value was checked to be finite and within u32 bounds;
    // rounded cannot leave those bounds for values in that closed interval.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(Some(rounded as u32))
}

fn invalid_u32_count(field: &'static str, value: impl std::fmt::Display) -> ProjectionIssue {
    ProjectionIssue::InvalidField {
        field,
        details: format!(
            "expected finite integer count in 0..={}, got {value}",
            u32::MAX
        ),
    }
}
