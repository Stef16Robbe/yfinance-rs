use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::value::RawValue;

use super::scalar::{WireDecode, WireDecoded};

#[derive(Clone, Debug, Default)]
pub enum WireValue<T> {
    #[default]
    Missing,
    Valid(T),
    Invalid(String),
}

impl<T> WireValue<T> {
    pub(crate) const fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Missing | Self::Invalid(_) => None,
        }
    }

    pub(crate) fn into_option(self) -> Option<T> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Missing | Self::Invalid(_) => None,
        }
    }

    pub(crate) fn invalid_details(&self) -> Option<&str> {
        match self {
            Self::Invalid(details) => Some(details),
            Self::Missing | Self::Valid(_) => None,
        }
    }
}

impl WireValue<String> {
    pub(crate) fn as_str(&self) -> Option<&str> {
        self.as_ref().map(String::as_str)
    }

    pub(crate) fn cloned_string(&self) -> Option<String> {
        self.as_ref().cloned()
    }
}

impl<T> From<WireDecoded<T>> for WireValue<T> {
    fn from(decoded: WireDecoded<T>) -> Self {
        match decoded {
            WireDecoded::Missing => Self::Missing,
            WireDecoded::Valid(value) => Self::Valid(value),
            WireDecoded::Invalid(details) => Self::Invalid(details),
        }
    }
}

impl<'de, T> Deserialize<'de> for WireValue<T>
where
    T: WireDecode<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(T::decode_wire(deserializer)?.into())
    }
}

#[derive(Clone, Debug)]
pub struct BufferedWireValue<T>(pub(super) WireValue<T>);

impl<T> BufferedWireValue<T> {
    pub(crate) const fn as_ref(&self) -> Option<&T> {
        self.0.as_ref()
    }

    pub(crate) fn into_option(self) -> Option<T> {
        self.0.into_option()
    }

    pub(crate) fn invalid_details(&self) -> Option<&str> {
        self.0.invalid_details()
    }
}

impl<T> Default for BufferedWireValue<T> {
    fn default() -> Self {
        Self(WireValue::Missing)
    }
}

impl<'de, T> Deserialize<'de> for BufferedWireValue<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Box::<RawValue>::deserialize(deserializer)?;
        if value.get() == "null" {
            return Ok(Self(WireValue::Missing));
        }

        Ok(Self(match serde_json::from_str(value.get()) {
            Ok(value) => WireValue::Valid(value),
            Err(err) => WireValue::Invalid(err.to_string()),
        }))
    }
}

pub trait WireField<T> {
    fn as_ref(&self) -> Option<&T>;
    fn invalid_details(&self) -> Option<&str>;
}

impl<T> WireField<T> for WireValue<T> {
    fn as_ref(&self) -> Option<&T> {
        self.as_ref()
    }

    fn invalid_details(&self) -> Option<&str> {
        self.invalid_details()
    }
}

impl<T> WireField<T> for BufferedWireValue<T> {
    fn as_ref(&self) -> Option<&T> {
        self.as_ref()
    }

    fn invalid_details(&self) -> Option<&str> {
        self.invalid_details()
    }
}
