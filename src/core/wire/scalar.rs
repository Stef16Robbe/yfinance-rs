use std::{fmt, marker::PhantomData};

use serde::{
    Deserializer,
    de::{IgnoredAny, MapAccess, SeqAccess, Visitor},
};

pub(super) enum WireDecoded<T> {
    Missing,
    Valid(T),
    Invalid(String),
}

impl<T> WireDecoded<T> {
    pub(super) fn from_result(result: Result<T, String>) -> Self {
        match result {
            Ok(value) => Self::Valid(value),
            Err(details) => Self::Invalid(details),
        }
    }

    pub(super) fn map<U>(self, map: impl FnOnce(T) -> U) -> WireDecoded<U> {
        match self {
            Self::Missing => WireDecoded::Missing,
            Self::Valid(value) => WireDecoded::Valid(map(value)),
            Self::Invalid(details) => WireDecoded::Invalid(details),
        }
    }
}

pub(super) trait WireDecode<'de>: Sized {
    fn decode_wire<D>(deserializer: D) -> Result<WireDecoded<Self>, D::Error>
    where
        D: Deserializer<'de>;
}

pub(super) trait ScalarWire: Sized {
    const EXPECTED: &'static str;

    fn from_bool(value: bool) -> Result<Self, String> {
        Err(unexpected(
            Self::EXPECTED,
            format_args!("boolean `{value}`"),
        ))
    }

    fn from_i64(value: i64) -> Result<Self, String> {
        Err(unexpected(
            Self::EXPECTED,
            format_args!("integer `{value}`"),
        ))
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        Err(unexpected(
            Self::EXPECTED,
            format_args!("integer `{value}`"),
        ))
    }

    fn from_f64(value: f64) -> Result<Self, String> {
        Err(unexpected(
            Self::EXPECTED,
            format_args!("floating point `{value}`"),
        ))
    }

    fn from_str(value: &str) -> Result<Self, String> {
        Err(unexpected(Self::EXPECTED, format_args!("string `{value}`")))
    }

    fn from_string(value: String) -> Result<Self, String> {
        Self::from_str(&value)
    }
}

impl<'de, T> WireDecode<'de> for T
where
    T: ScalarWire,
{
    fn decode_wire<D>(deserializer: D) -> Result<WireDecoded<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ScalarVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for ScalarVisitor<T>
        where
            T: ScalarWire,
        {
            type Value = WireDecoded<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(T::EXPECTED)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(WireDecoded::Missing)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(WireDecoded::Missing)
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(WireDecoded::from_result(T::from_bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(WireDecoded::from_result(T::from_i64(value)))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(WireDecoded::from_result(T::from_u64(value)))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(WireDecoded::from_result(T::from_f64(value)))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(WireDecoded::from_result(T::from_str(value)))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(WireDecoded::from_result(T::from_string(value)))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                drain_seq(&mut seq)?;
                Ok(WireDecoded::Invalid(unexpected(T::EXPECTED, "array")))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                drain_map(&mut map)?;
                Ok(WireDecoded::Invalid(unexpected(T::EXPECTED, "object")))
            }
        }

        deserializer.deserialize_any(ScalarVisitor::<T>(PhantomData))
    }
}

impl ScalarWire for String {
    const EXPECTED: &'static str = "string";

    fn from_str(value: &str) -> Result<Self, String> {
        Ok(value.to_owned())
    }

    fn from_string(value: String) -> Result<Self, String> {
        Ok(value)
    }
}

impl ScalarWire for bool {
    const EXPECTED: &'static str = "boolean";

    fn from_bool(value: bool) -> Result<Self, String> {
        Ok(value)
    }
}

impl ScalarWire for i64 {
    const EXPECTED: &'static str = "signed integer";

    fn from_i64(value: i64) -> Result<Self, String> {
        Ok(value)
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        Self::try_from(value).map_err(|_| format!("integer `{value}` does not fit in i64"))
    }
}

impl ScalarWire for u64 {
    const EXPECTED: &'static str = "unsigned integer";

    fn from_i64(value: i64) -> Result<Self, String> {
        Self::try_from(value).map_err(|_| format!("integer `{value}` does not fit in u64"))
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        Ok(value)
    }
}

#[allow(clippy::cast_precision_loss)]
impl ScalarWire for f64 {
    const EXPECTED: &'static str = "floating point number";

    fn from_i64(value: i64) -> Result<Self, String> {
        Ok(value as Self)
    }

    fn from_u64(value: u64) -> Result<Self, String> {
        Ok(value as Self)
    }

    fn from_f64(value: f64) -> Result<Self, String> {
        Ok(value)
    }
}

pub(super) fn drain_seq<'de, A>(seq: &mut A) -> Result<(), A::Error>
where
    A: SeqAccess<'de>,
{
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

fn drain_map<'de, A>(map: &mut A) -> Result<(), A::Error>
where
    A: MapAccess<'de>,
{
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

pub(super) fn unexpected(expected: &'static str, actual: impl fmt::Display) -> String {
    format!("expected {expected}, got {actual}")
}
