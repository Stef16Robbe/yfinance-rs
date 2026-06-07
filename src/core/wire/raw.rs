use std::{fmt, marker::PhantomData};

use paft::Decimal;
use serde::{
    Deserialize, Deserializer,
    de::{IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_field_result::{Field, FieldDecode, invalid_seq};

use super::{
    number::{JsonDecimal, JsonU64, de_decimal_from_json, de_u64_from_json},
    value::WireValue,
};

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct RawNum<T> {
    pub(crate) raw: Option<T>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct RawDecimal {
    #[serde(default, deserialize_with = "de_decimal_from_json")]
    pub(crate) raw: Option<Decimal>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct RawDate {
    pub(crate) raw: Option<i64>,
}

pub fn from_raw_date(r: Option<RawDate>) -> Option<i64> {
    r.and_then(|d| d.raw)
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct RawNumU64 {
    #[serde(default, deserialize_with = "de_u64_from_json")]
    pub(crate) raw: Option<u64>,
}

impl<'de, T> FieldDecode<'de> for RawNum<T>
where
    T: FieldDecode<'de>,
{
    fn decode_field<D>(deserializer: D) -> Result<Field<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        decode_raw_map::<_, T>(deserializer).map(|decoded| decoded.map(|raw| Self { raw }))
    }
}

impl<'de> FieldDecode<'de> for RawDecimal {
    fn decode_field<D>(deserializer: D) -> Result<Field<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        decode_raw_map::<_, JsonDecimal>(deserializer).map(|decoded| {
            decoded.map(|raw| Self {
                raw: raw.map(JsonDecimal::into_decimal),
            })
        })
    }
}

impl<'de> FieldDecode<'de> for RawDate {
    fn decode_field<D>(deserializer: D) -> Result<Field<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        decode_raw_map::<_, i64>(deserializer).map(|decoded| decoded.map(|raw| Self { raw }))
    }
}

impl<'de> FieldDecode<'de> for RawNumU64 {
    fn decode_field<D>(deserializer: D) -> Result<Field<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        decode_raw_map::<_, JsonU64>(deserializer).map(|decoded| {
            decoded.map(|raw| Self {
                raw: raw.map(JsonU64::into_u64),
            })
        })
    }
}

fn decode_raw_map<'de, D, T>(deserializer: D) -> Result<Field<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: FieldDecode<'de>,
{
    struct RawMapVisitor<T>(PhantomData<T>);

    impl<'de, T> Visitor<'de> for RawMapVisitor<T>
    where
        T: FieldDecode<'de>,
    {
        type Value = Field<Option<T>>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(RAW_OBJECT)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(Field::Missing)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(Field::Missing)
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut raw = None;
            let mut invalid = None;

            while let Some(field) = map.next_key::<RawField>()? {
                match field {
                    RawField::Raw => match map.next_value::<WireValue<T>>()? {
                        WireValue::Missing => raw = None,
                        WireValue::Valid(value) => raw = Some(value),
                        WireValue::Invalid(error) => {
                            if invalid.is_none() {
                                invalid = Some(error);
                            }
                        }
                    },
                    RawField::Other => {
                        let _: IgnoredAny = map.next_value()?;
                    }
                }
            }

            Ok(invalid.map_or_else(|| Field::Valid(raw), Field::Invalid))
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
            Ok(invalid_raw(format_args!("boolean `{value}`")))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
            Ok(invalid_raw(format_args!("integer `{value}`")))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(invalid_raw(format_args!("integer `{value}`")))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
            Ok(invalid_raw(format_args!("floating point `{value}`")))
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(invalid_raw(format_args!("string `{value}`")))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
            Ok(invalid_raw(format_args!("string `{value}`")))
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            invalid_seq(&mut seq, unexpected(RAW_OBJECT, "array"))
        }
    }

    deserializer.deserialize_any(RawMapVisitor::<T>(PhantomData))
}

const RAW_OBJECT: &str = "Yahoo raw value object";

fn invalid_raw<T>(actual: impl fmt::Display) -> Field<T> {
    Field::invalid(unexpected(RAW_OBJECT, actual))
}

fn unexpected(expected: &'static str, actual: impl fmt::Display) -> String {
    format!("expected {expected}, got {actual}")
}

#[derive(Deserialize)]
#[serde(field_identifier)]
enum RawField {
    #[serde(rename = "raw")]
    Raw,
    #[serde(other)]
    Other,
}
