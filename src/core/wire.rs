use paft::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Deserializer};
use serde_json::{Number, Value};
use std::str::FromStr;

#[derive(Deserialize, Clone, Copy)]
pub struct RawNum<T> {
    pub(crate) raw: Option<T>,
}

pub fn from_raw<T>(raw: Option<RawNum<T>>) -> Option<T> {
    raw.and_then(|n| n.raw)
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct RawDecimal {
    #[serde(default, deserialize_with = "de_decimal_from_json")]
    pub(crate) raw: Option<Decimal>,
}

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
        decimal_from_json_value(Value::deserialize(deserializer)?)
            .map(|value| Self { value })
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Clone, Copy)]
pub struct RawDate {
    pub(crate) raw: Option<i64>,
}

pub fn from_raw_date(r: Option<RawDate>) -> Option<i64> {
    r.and_then(|d| d.raw)
}

fn de_decimal_from_json<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer)?
        .map(decimal_from_json_value)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn de_u64_from_json<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer)?
        .map(u64_from_json_value)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn decimal_from_json_value(value: Value) -> Result<Decimal, String> {
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

fn u64_from_json_value(value: Value) -> Result<u64, String> {
    let decimal = decimal_from_json_value(value)?;
    if decimal.is_sign_negative() || !decimal.fract().is_zero() {
        return Err(format!("cannot convert decimal {decimal} to u64"));
    }
    decimal
        .to_u64()
        .ok_or_else(|| format!("cannot convert decimal {decimal} to u64"))
}

#[derive(Deserialize, Clone, Copy)]
pub struct RawNumU64 {
    #[serde(default, deserialize_with = "de_u64_from_json")]
    pub(crate) raw: Option<u64>,
}
