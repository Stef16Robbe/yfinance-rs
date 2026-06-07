use serde::Deserialize;
use serde_json::Value;

use crate::core::wire::WireValue;

#[derive(Deserialize)]
pub struct NewsEnvelope {
    pub(crate) data: Option<NewsData>,
}

#[derive(Deserialize)]
pub struct NewsData {
    #[serde(rename = "tickerStream")]
    pub(crate) ticker_stream: Option<TickerStream>,
}

#[derive(Deserialize)]
pub struct TickerStream {
    pub(crate) stream: Option<Vec<Value>>,
}

#[derive(Deserialize)]
pub struct StreamItem {
    #[serde(default)]
    pub(crate) id: WireValue<String>,
    #[serde(default)]
    pub(crate) content: WireValue<Content>,
    // The python 'ad' check might be for a field at this level.
    pub(crate) ad: Option<Value>,
}

#[derive(Deserialize)]
pub struct Content {
    #[serde(default)]
    pub(crate) title: WireValue<String>,
    #[serde(rename = "pubDate")]
    #[serde(default)]
    pub(crate) pub_date: WireValue<String>,
    #[serde(default)]
    pub(crate) provider: WireValue<Provider>,
    #[serde(rename = "canonicalUrl")]
    #[serde(default)]
    pub(crate) canonical_url: WireValue<CanonicalUrl>,
}

#[derive(Deserialize)]
pub struct Provider {
    #[serde(rename = "displayName")]
    #[serde(default)]
    pub(crate) display_name: WireValue<String>,
}

#[derive(Deserialize)]
pub struct CanonicalUrl {
    #[serde(default)]
    pub(crate) url: WireValue<String>,
}
