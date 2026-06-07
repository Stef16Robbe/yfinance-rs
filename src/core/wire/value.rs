use std::borrow::Cow;

use serde_field_result::{BorrowedJsonField, Field, JsonField};

pub type WireValue<T> = Field<T>;

pub type BufferedWireValue<T> = JsonField<T>;

pub type BorrowedWireValue<'de, T> = BorrowedJsonField<'de, T>;

pub trait WireField<T> {
    fn as_ref(&self) -> Option<&T>;
    fn invalid_details(&self) -> Option<Cow<'_, str>>;
}

impl<T> WireField<T> for WireValue<T> {
    fn as_ref(&self) -> Option<&T> {
        self.as_ref()
    }

    fn invalid_details(&self) -> Option<Cow<'_, str>> {
        self.error().map(serde_field_result::FieldError::message)
    }
}

impl<T> WireField<T> for BorrowedWireValue<'_, T> {
    fn as_ref(&self) -> Option<&T> {
        self.as_ref()
    }

    fn invalid_details(&self) -> Option<Cow<'_, str>> {
        self.error().map(serde_field_result::FieldError::message)
    }
}

impl<T> WireField<T> for BufferedWireValue<T> {
    fn as_ref(&self) -> Option<&T> {
        self.as_ref()
    }

    fn invalid_details(&self) -> Option<Cow<'_, str>> {
        self.error().map(serde_field_result::FieldError::message)
    }
}
