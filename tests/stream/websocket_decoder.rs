use super::common;
use base64::{Engine as _, engine::general_purpose};
use prost::Message;

#[derive(Clone, PartialEq, Message)]
struct TestPricingData {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(float, tag = "2")]
    pub price: f32,
    #[prost(sint64, tag = "3")]
    pub time: i64,
    #[prost(string, tag = "4")]
    pub currency: String,
    #[prost(string, tag = "5")]
    pub exchange: String,
    #[prost(int32, tag = "6")]
    pub quote_type: i32,
    #[prost(sint64, tag = "9")]
    pub day_volume: i64,
    #[prost(float, tag = "16")]
    pub previous_close: f32,
    #[prost(sint64, tag = "27")]
    pub price_hint: i64,
}

#[test]
fn decode_real_websocket_message() {
    let base64_msg = common::fixture("stream_ws", "MULTI", "b64");
    let update = yfinance_rs::stream::decode_and_map_message(&base64_msg).unwrap();

    // Generic assertions, as the symbol/price will change with each recording
    assert!(
        !update.instrument.symbol.as_str().is_empty(),
        "symbol should not be empty"
    );
    assert!(update.price.is_some(), "price should be present");
    assert!(
        update
            .price
            .as_ref()
            .map_or(0.0, yfinance_rs::core::conversions::money_to_f64)
            > 0.0,
        "price should be positive"
    );

    // Decoder is stateless: volume must be None
    assert!(update.volume.is_none(), "decoder should not set volume");
}

#[test]
fn decode_equity_websocket_message_infers_currency_from_exchange() {
    let base64_msg = encode_test_pricing_data(&TestPricingData {
        id: "AAPL".to_string(),
        price: 314.6,
        time: 1_780_426_509_000,
        currency: String::new(),
        exchange: "NMS".to_string(),
        quote_type: 8,
        day_volume: 26_248_990,
        previous_close: 0.0,
        price_hint: 2,
    });

    let update = yfinance_rs::stream::decode_and_map_message(&base64_msg).unwrap();

    assert_eq!(update.instrument.symbol.as_str(), "AAPL");
    assert!(matches!(
        update.instrument.kind,
        yfinance_rs::AssetKind::Equity
    ));
    let price = update.price.as_ref().expect("price should be present");
    assert_eq!(price.currency().to_string(), "USD");
    assert!(
        yfinance_rs::core::conversions::money_to_f64(price) > 0.0,
        "price should be positive"
    );
    assert!(
        update.previous_close.is_none(),
        "protobuf default zero should not be projected as a real previous close"
    );
}

#[test]
fn decode_unknown_websocket_quote_type_uses_untyped_fallback() {
    let base64_msg = encode_test_pricing_data(&TestPricingData {
        id: "AAPL".to_string(),
        price: 314.6,
        time: 1_780_426_509_000,
        currency: "USD".to_string(),
        exchange: "NMS".to_string(),
        quote_type: 1001,
        day_volume: 26_248_990,
        previous_close: 313.0,
        price_hint: 2,
    });

    let update = yfinance_rs::stream::decode_and_map_message(&base64_msg).unwrap();

    assert_eq!(update.instrument.symbol.as_str(), "AAPL");
    assert_eq!(
        update.instrument.kind.to_string(),
        "YAHOO_STREAM_UNTYPED",
        "unknown stream quote types should keep using the explicit fallback"
    );
}

fn encode_test_pricing_data(message: &TestPricingData) -> String {
    let mut bytes = Vec::new();
    message.encode(&mut bytes).unwrap();
    general_purpose::STANDARD.encode(bytes)
}
