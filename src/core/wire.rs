mod number;
mod raw;
mod scalar;
mod value;

pub use number::{JsonDecimal, JsonU64, decimal_from_json_value};
pub use raw::{RawDate, RawDecimal, RawNum, RawNumU64, from_raw, from_raw_date};
pub use value::{BufferedWireValue, WireField, WireValue};

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn invalid_scalar_is_recorded_without_losing_following_fields() {
        #[derive(Deserialize)]
        struct Row {
            #[serde(default)]
            bad: WireValue<f64>,
            #[serde(default)]
            after: WireValue<i64>,
        }

        let row: Row = serde_json::from_str(r#"{"bad":{"nested":[1,2,3]},"after":7}"#).unwrap();

        assert!(matches!(row.bad, WireValue::Invalid(_)));
        assert!(matches!(row.after, WireValue::Valid(7)));
    }

    #[test]
    fn invalid_raw_value_is_recorded_without_losing_following_fields() {
        #[derive(Deserialize)]
        struct Row {
            #[serde(default)]
            quote: WireValue<RawNum<f64>>,
            #[serde(default)]
            after: WireValue<String>,
        }

        let row: Row =
            serde_json::from_str(r#"{"quote":{"raw":[1,2],"fmt":"bad"},"after":"ok"}"#).unwrap();

        assert!(matches!(row.quote, WireValue::Invalid(_)));
        assert_eq!(row.after.as_str(), Some("ok"));
    }

    #[test]
    fn json_u64_accepts_integral_strings_and_rejects_fractional_strings() {
        let valid: WireValue<JsonU64> = serde_json::from_str(r#""42""#).unwrap();
        assert_eq!(valid.as_ref().copied().map(JsonU64::into_u64), Some(42));

        let invalid: WireValue<JsonU64> = serde_json::from_str(r#""42.5""#).unwrap();
        assert!(matches!(
            invalid.invalid_details(),
            Some(details) if details.contains("cannot convert decimal")
        ));
    }

    #[test]
    fn buffered_wire_value_keeps_composite_recovery_explicit() {
        #[allow(dead_code)]
        #[derive(Deserialize)]
        struct Nested {
            value: String,
        }

        #[derive(Deserialize)]
        struct Row {
            #[serde(default)]
            nested: BufferedWireValue<Nested>,
            #[serde(default)]
            after: WireValue<i64>,
        }

        let row: Row = serde_json::from_str(r#"{"nested":{"value":[]},"after":9}"#).unwrap();

        assert!(matches!(row.nested.0, WireValue::Invalid(_)));
        assert!(matches!(row.after, WireValue::Valid(9)));
    }
}
