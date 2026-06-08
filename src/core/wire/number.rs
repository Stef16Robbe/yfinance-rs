use std::str::FromStr;

use paft::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use serde::{Deserialize, Deserializer, de};
use serde_field_result::{Field, FieldDecode, FieldError, ScalarFieldDecode};
use serde_json::{Number, Value};

#[derive(Clone, Copy, Debug)]
pub struct JsonDecimal {
    value: Decimal,
}

impl JsonDecimal {
    pub(crate) const fn into_decimal(self) -> Decimal {
        self.value
    }
}

impl<'de> Deserialize<'de> for JsonDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match <Self as FieldDecode<'de>>::decode_field(deserializer)? {
            Field::Valid(value) => Ok(value),
            Field::Missing => Err(de::Error::invalid_type(
                de::Unexpected::Unit,
                &"JSON number or numeric string",
            )),
            Field::Invalid(error) => Err(de::Error::custom(error)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct JsonU64 {
    value: u64,
}

impl JsonU64 {
    pub(crate) const fn into_u64(self) -> u64 {
        self.value
    }
}

impl<'de> Deserialize<'de> for JsonU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match <Self as FieldDecode<'de>>::decode_field(deserializer)? {
            Field::Valid(value) => Ok(value),
            Field::Missing => Err(de::Error::invalid_type(
                de::Unexpected::Unit,
                &"unsigned integer or unsigned integer string",
            )),
            Field::Invalid(error) => Err(de::Error::custom(error)),
        }
    }
}

pub(super) fn de_decimal_from_json<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<JsonDecimal>::deserialize(deserializer)
        .map(|value| value.map(JsonDecimal::into_decimal))
}

pub(super) fn de_u64_from_json<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<JsonU64>::deserialize(deserializer).map(|value| value.map(JsonU64::into_u64))
}

pub fn decimal_from_json_value(value: Value) -> Result<Decimal, String> {
    match value {
        Value::Number(number) => decimal_from_json_number(&number),
        Value::String(value) => decimal_from_str(value.trim()),
        other => Err(format!(
            "expected JSON number or numeric string, got {other}"
        )),
    }
}

fn decimal_from_json_number(number: &Number) -> Result<Decimal, String> {
    if let Some(value) = number.as_i64() {
        return Ok(Decimal::from(value));
    }
    if let Some(value) = number.as_u64() {
        return Ok(Decimal::from(value));
    }
    decimal_from_str(&number.to_string())
}

fn decimal_from_str(value: &str) -> Result<Decimal, String> {
    decimal_from_str_field(value).map_err(FieldError::into_message)
}

fn decimal_from_str_field(value: &str) -> Result<Decimal, FieldError> {
    if value.is_empty() {
        return Err(FieldError::static_message("empty numeric value"));
    }
    Decimal::from_str(value)
        .or_else(|_| Decimal::from_scientific(value))
        .map_err(|err| FieldError::new(format!("cannot parse decimal {value:?}: {err}")))
}

fn decimal_from_f64_field(value: f64) -> Result<Decimal, FieldError> {
    Decimal::try_from(value)
        .map_err(|err| FieldError::new(format!("cannot parse decimal {value:?}: {err}")))
}

fn decimal_from_i128_field(value: i128) -> Result<Decimal, FieldError> {
    Decimal::from_i128(value)
        .ok_or_else(|| FieldError::new(format!("cannot parse decimal {value}: out of range")))
}

fn decimal_from_u128_field(value: u128) -> Result<Decimal, FieldError> {
    Decimal::from_u128(value)
        .ok_or_else(|| FieldError::new(format!("cannot parse decimal {value}: out of range")))
}

fn u64_from_decimal(decimal: Decimal) -> Result<u64, FieldError> {
    if decimal.is_sign_negative() || !decimal.fract().is_zero() {
        return Err(FieldError::new(format!(
            "cannot convert decimal {decimal} to u64"
        )));
    }
    decimal
        .to_u64()
        .ok_or_else(|| FieldError::new(format!("cannot convert decimal {decimal} to u64")))
}

fn u64_from_f64(value: f64) -> Result<u64, FieldError> {
    const MAX_EXACT_U64_IN_F64: f64 = 9_007_199_254_740_992.0;

    if (0.0..=MAX_EXACT_U64_IN_F64).contains(&value)
        && value.fract() == 0.0
        && let Some(value) = value.to_u64()
    {
        return Ok(value);
    }

    decimal_from_f64_field(value).and_then(u64_from_decimal)
}

fn u64_from_i128(value: i128) -> Result<u64, FieldError> {
    u64::try_from(value)
        .map_err(|_| FieldError::new(format!("cannot convert integer {value} to u64")))
}

fn u64_from_u128(value: u128) -> Result<u64, FieldError> {
    u64::try_from(value)
        .map_err(|_| FieldError::new(format!("cannot convert integer {value} to u64")))
}

impl ScalarFieldDecode for JsonDecimal {
    const EXPECTED: &'static str = "JSON number or numeric string";

    fn from_i64(value: i64) -> Result<Self, FieldError> {
        Ok(Self {
            value: Decimal::from(value),
        })
    }

    fn from_u64(value: u64) -> Result<Self, FieldError> {
        Ok(Self {
            value: Decimal::from(value),
        })
    }

    fn from_i128(value: i128) -> Result<Self, FieldError> {
        decimal_from_i128_field(value).map(|value| Self { value })
    }

    fn from_u128(value: u128) -> Result<Self, FieldError> {
        decimal_from_u128_field(value).map(|value| Self { value })
    }

    fn from_f64(value: f64) -> Result<Self, FieldError> {
        decimal_from_f64_field(value).map(|value| Self { value })
    }

    fn from_str(value: &str) -> Result<Self, FieldError> {
        decimal_from_str_field(value.trim()).map(|value| Self { value })
    }
}

impl ScalarFieldDecode for JsonU64 {
    const EXPECTED: &'static str = "unsigned integer or unsigned integer string";

    fn from_i64(value: i64) -> Result<Self, FieldError> {
        u64_from_i128(i128::from(value)).map(|value| Self { value })
    }

    fn from_u64(value: u64) -> Result<Self, FieldError> {
        Ok(Self { value })
    }

    fn from_i128(value: i128) -> Result<Self, FieldError> {
        u64_from_i128(value).map(|value| Self { value })
    }

    fn from_u128(value: u128) -> Result<Self, FieldError> {
        u64_from_u128(value).map(|value| Self { value })
    }

    fn from_f64(value: f64) -> Result<Self, FieldError> {
        u64_from_f64(value).map(|value| Self { value })
    }

    fn from_str(value: &str) -> Result<Self, FieldError> {
        let value = value.trim();
        if let Ok(value) = value.parse() {
            return Ok(Self { value });
        }

        let decimal = decimal_from_str_field(value)?;
        u64_from_decimal(decimal).map(|value| Self { value })
    }
}
