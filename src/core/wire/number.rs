use std::str::FromStr;

use paft::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Deserializer, de};
use serde_json::{Number, Value};

use super::scalar::{ScalarWire, WireDecode, WireDecoded};

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
        match <Self as WireDecode<'de>>::decode_wire(deserializer)? {
            WireDecoded::Valid(value) => Ok(value),
            WireDecoded::Missing => Err(de::Error::invalid_type(
                de::Unexpected::Unit,
                &"JSON number or numeric string",
            )),
            WireDecoded::Invalid(details) => Err(de::Error::custom(details)),
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
        match <Self as WireDecode<'de>>::decode_wire(deserializer)? {
            WireDecoded::Valid(value) => Ok(value),
            WireDecoded::Missing => Err(de::Error::invalid_type(
                de::Unexpected::Unit,
                &"unsigned integer or unsigned integer string",
            )),
            WireDecoded::Invalid(details) => Err(de::Error::custom(details)),
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
    if value.is_empty() {
        return Err("empty numeric value".into());
    }
    Decimal::from_str(value)
        .or_else(|_| Decimal::from_scientific(value))
        .map_err(|err| format!("cannot parse decimal {value:?}: {err}"))
}

fn u64_from_decimal(decimal: Decimal) -> Result<u64, String> {
    if decimal.is_sign_negative() || !decimal.fract().is_zero() {
        return Err(format!("cannot convert decimal {decimal} to u64"));
    }
    decimal
        .to_u64()
        .ok_or_else(|| format!("cannot convert decimal {decimal} to u64"))
}

impl ScalarWire for JsonDecimal {
    const EXPECTED: &'static str = "JSON number or numeric string";

    fn from_i64(value: i64) -> Result<Self, String> {
        Ok(Self {
            value: Decimal::from(value),
        })
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        Ok(Self {
            value: Decimal::from(value),
        })
    }

    fn from_f64(value: f64) -> Result<Self, String> {
        decimal_from_str(&value.to_string()).map(|value| Self { value })
    }

    fn from_str(value: &str) -> Result<Self, String> {
        decimal_from_str(value.trim()).map(|value| Self { value })
    }
}

impl ScalarWire for JsonU64 {
    const EXPECTED: &'static str = "unsigned integer or unsigned integer string";

    fn from_i64(value: i64) -> Result<Self, String> {
        u64_from_decimal(Decimal::from(value)).map(|value| Self { value })
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        Ok(Self { value })
    }

    fn from_f64(value: f64) -> Result<Self, String> {
        let decimal = decimal_from_str(&value.to_string())?;
        u64_from_decimal(decimal).map(|value| Self { value })
    }

    fn from_str(value: &str) -> Result<Self, String> {
        let decimal = decimal_from_str(value.trim())?;
        u64_from_decimal(decimal).map(|value| Self { value })
    }
}
