// Generated from `yaticker.proto` and committed to avoid build-time protobuf codegen.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PricingData {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String,
    #[prost(float, tag = "2")]
    pub price: f32,
    #[prost(sint64, tag = "3")]
    pub time: i64,
    #[prost(string, tag = "4")]
    pub currency: ::prost::alloc::string::String,
    #[prost(string, tag = "5")]
    pub exchange: ::prost::alloc::string::String,
    #[prost(int32, tag = "6")]
    pub quote_type: i32,
    #[prost(int32, tag = "7")]
    pub market_hours: i32,
    #[prost(float, tag = "8")]
    pub change_percent: f32,
    #[prost(sint64, tag = "9")]
    pub day_volume: i64,
    #[prost(float, tag = "10")]
    pub day_high: f32,
    #[prost(float, tag = "11")]
    pub day_low: f32,
    #[prost(float, tag = "12")]
    pub change: f32,
    #[prost(string, tag = "13")]
    pub short_name: ::prost::alloc::string::String,
    #[prost(sint64, tag = "14")]
    pub expire_date: i64,
    #[prost(float, tag = "15")]
    pub open_price: f32,
    #[prost(float, tag = "16")]
    pub previous_close: f32,
    #[prost(float, tag = "17")]
    pub strike_price: f32,
    #[prost(string, tag = "18")]
    pub underlying_symbol: ::prost::alloc::string::String,
    #[prost(sint64, tag = "19")]
    pub open_interest: i64,
    #[prost(sint64, tag = "20")]
    pub options_type: i64,
    #[prost(sint64, tag = "21")]
    pub mini_option: i64,
    #[prost(sint64, tag = "22")]
    pub last_size: i64,
    #[prost(float, tag = "23")]
    pub bid: f32,
    #[prost(sint64, tag = "24")]
    pub bid_size: i64,
    #[prost(float, tag = "25")]
    pub ask: f32,
    #[prost(sint64, tag = "26")]
    pub ask_size: i64,
    #[prost(sint64, tag = "27")]
    pub price_hint: i64,
    #[prost(sint64, tag = "28")]
    pub vol_24hr: i64,
    #[prost(sint64, tag = "29")]
    pub vol_all_currencies: i64,
    #[prost(string, tag = "30")]
    pub from_currency: ::prost::alloc::string::String,
    #[prost(string, tag = "31")]
    pub last_market: ::prost::alloc::string::String,
    #[prost(double, tag = "32")]
    pub circulating_supply: f64,
    #[prost(double, tag = "33")]
    pub market_cap: f64,
}
