use std::{error::Error as StdError, fmt};

use crate::core::redaction::redact_auth_query_params_in_text;

/// A redacted wrapper around HTTP-client errors.
///
/// The underlying HTTP client can include full request URLs in its formatted
/// errors. Store only the redacted display text so `YfError` display/debug
/// output cannot leak crumb or auth-like query parameters.
pub struct RedactedHttpError {
    message: String,
}

impl RedactedHttpError {
    pub(crate) fn new(error: &reqwest::Error) -> Self {
        Self {
            message: redact_auth_query_params_in_text(&error.to_string()),
        }
    }
}

impl fmt::Display for RedactedHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Debug for RedactedHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for RedactedHttpError {}

/// An opaque wrapper around WebSocket transport errors.
pub struct WebsocketError {
    source: Box<dyn StdError + Send + Sync + 'static>,
}

impl WebsocketError {
    #[cfg(feature = "stream")]
    pub(crate) fn new(source: tokio_tungstenite::tungstenite::Error) -> Self {
        Self {
            source: Box::new(source),
        }
    }

    fn as_source(&self) -> &(dyn StdError + 'static) {
        self.source.as_ref()
    }
}

impl fmt::Display for WebsocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, f)
    }
}

impl fmt::Debug for WebsocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl StdError for WebsocketError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.as_source())
    }
}

macro_rules! boxed_source_error {
    (
        $(#[$meta:meta])*
        $name:ident
    ) => {
        $(#[$meta])*
        pub struct $name {
            source: Box<dyn StdError + Send + Sync + 'static>,
        }

        impl $name {
            #[cfg(feature = "stream")]
            fn from_source(source: impl StdError + Send + Sync + 'static) -> Self {
                Self {
                    source: Box::new(source),
                }
            }

            fn as_source(&self) -> &(dyn StdError + 'static) {
                self.source.as_ref()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.source, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl StdError for $name {
            fn source(&self) -> Option<&(dyn StdError + 'static)> {
                Some(self.as_source())
            }
        }
    };
}

macro_rules! opaque_source_error {
    (
        $(#[$meta:meta])*
        $name:ident($source:ty)
    ) => {
        $(#[$meta])*
        pub struct $name {
            source: $source,
        }

        impl $name {
            pub(crate) const fn new(source: $source) -> Self {
                Self { source }
            }

            fn as_source(&self) -> &(dyn StdError + 'static) {
                &self.source
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.source, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl StdError for $name {
            fn source(&self) -> Option<&(dyn StdError + 'static)> {
                Some(self.as_source())
            }
        }
    };
}

boxed_source_error!(
    /// An opaque wrapper around Protobuf decoding errors.
    ProtobufDecodeError
);

opaque_source_error!(
    /// An opaque wrapper around JSON serialization and deserialization errors.
    JsonError(serde_json::Error)
);

boxed_source_error!(
    /// An opaque wrapper around Base64 decoding errors.
    Base64DecodeError
);

opaque_source_error!(
    /// An opaque wrapper around URL parsing errors.
    UrlParseError(url::ParseError)
);

/// The primary error type for the `yfinance-rs` crate.
#[non_exhaustive]
#[derive(Debug)]
pub enum YfError {
    /// An error originating from the underlying HTTP client (`reqwest`).
    Http(RedactedHttpError),

    /// An error related to WebSocket communication.
    Websocket(WebsocketError),

    /// An error during Protobuf decoding, typically from a WebSocket stream.
    Protobuf(ProtobufDecodeError),

    /// An error during JSON serialization or deserialization.
    Json(JsonError),

    /// An error during Base64 decoding.
    Base64(Base64DecodeError),

    /// An error that occurs when parsing a URL.
    Url(UrlParseError),

    /// A 404 Not Found returned by Yahoo endpoints.
    NotFound {
        /// The URL that returned a 404.
        url: String,
    },

    /// A 429 Too Many Requests (rate limit) returned by Yahoo endpoints.
    RateLimited {
        /// The URL that returned a 429.
        url: String,
    },

    /// A 5xx server error returned by Yahoo endpoints.
    ServerError {
        /// The HTTP status code in the 5xx range.
        status: u16,
        /// The URL that returned a server error.
        url: String,
    },

    /// An error indicating an unexpected, non-successful HTTP status code (non-404/429/5xx).
    Status {
        /// The unexpected HTTP status code returned.
        status: u16,
        /// The URL that returned the status.
        url: String,
    },

    /// An error returned by the Yahoo Finance API within an otherwise successful response.
    ///
    /// For example, a `200 OK` response might contain a JSON body with an `error` field.
    Api(String),

    /// An error related to authentication, such as failing to retrieve a cookie or crumb.
    Auth(String),

    /// An error that occurs during the web scraping process.
    Scrape(String),

    /// Indicates that an expected piece of data was missing from the API response.
    MissingData(String),

    /// Indicates that provider data was present but could not be mapped into the public model.
    InvalidData(String),

    /// Option contracts were present, but Yahoo did not provide a usable underlying type.
    OptionUnderlyingTypeUnavailable {
        /// The requested option-chain symbol.
        symbol: String,
        /// The raw Yahoo `quoteType`, when Yahoo supplied one.
        quote_type: Option<String>,
    },

    /// Indicates that provider data could not be projected losslessly in strict mode.
    DataQuality(Box<crate::core::diagnostics::YfWarning>),

    /// An error indicating that the parameters provided by the caller were invalid.
    InvalidParams(String),

    /// An error indicating that an HTTP request could not be cloned for retry.
    RequestNotCloneable,

    /// An error originating from `paft` money modeling.
    Money(paft::money::MoneyError),

    /// An error indicating that the provided date range is invalid (e.g., start date after end date).
    InvalidDates,
}

impl YfError {
    #[cfg(feature = "stream")]
    pub(crate) fn websocket(error: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Websocket(WebsocketError::new(error))
    }

    #[cfg(feature = "stream")]
    pub(crate) fn protobuf(error: prost::DecodeError) -> Self {
        Self::Protobuf(ProtobufDecodeError::from_source(error))
    }

    pub(crate) const fn json(error: serde_json::Error) -> Self {
        Self::Json(JsonError::new(error))
    }

    #[cfg(feature = "stream")]
    pub(crate) fn base64(error: base64::DecodeError) -> Self {
        Self::Base64(Base64DecodeError::from_source(error))
    }

    pub(crate) const fn url(error: url::ParseError) -> Self {
        Self::Url(UrlParseError::new(error))
    }
}

impl fmt::Display for YfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(f, "HTTP error: {error}"),
            Self::Websocket(error) => write!(f, "WebSocket error: {error}"),
            Self::Protobuf(error) => write!(f, "Protobuf decoding error: {error}"),
            Self::Json(error) => write!(f, "JSON parsing error: {error}"),
            Self::Base64(error) => write!(f, "Base64 decoding error: {error}"),
            Self::Url(error) => write!(f, "Invalid URL: {error}"),
            Self::NotFound { url } => write!(f, "Not found at {url}"),
            Self::RateLimited { url } => write!(f, "Rate limited at {url}"),
            Self::ServerError { status, url } => write!(f, "Server error {status} at {url}"),
            Self::Status { status, url } => {
                write!(f, "Unexpected response status: {status} at {url}")
            }
            Self::Api(message) => write!(f, "Yahoo API error: {message}"),
            Self::Auth(message) => write!(f, "Authentication error: {message}"),
            Self::Scrape(message) => write!(f, "Web scraping error: {message}"),
            Self::MissingData(message) => write!(f, "Missing data in response: {message}"),
            Self::InvalidData(message) => write!(f, "Invalid data in response: {message}"),
            Self::OptionUnderlyingTypeUnavailable { symbol, .. } => {
                write!(
                    f,
                    "contracts present, underlying type unavailable for {symbol}"
                )
            }
            Self::DataQuality(warning) => write!(f, "Provider data quality issue: {warning}"),
            Self::InvalidParams(message) => write!(f, "Invalid parameters: {message}"),
            Self::RequestNotCloneable => write!(f, "Request cannot be cloned for retry"),
            Self::Money(error) => write!(f, "Money data error: {error}"),
            Self::InvalidDates => {
                f.write_str("Invalid date range: start date must be before end date")
            }
        }
    }
}

impl StdError for YfError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Websocket(error) => Some(error.as_source()),
            Self::Protobuf(error) => Some(error.as_source()),
            Self::Json(error) => Some(error.as_source()),
            Self::Base64(error) => Some(error.as_source()),
            Self::Url(error) => Some(error.as_source()),
            Self::Money(error) => Some(error),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for YfError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(RedactedHttpError::new(&e))
    }
}

impl From<paft::money::MoneyError> for YfError {
    fn from(error: paft::money::MoneyError) -> Self {
        Self::Money(error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    fn assert_source<T>(error: &YfError)
    where
        T: std::error::Error + 'static,
    {
        assert!(error.source().expect("error source").is::<T>());
    }

    #[test]
    fn opaque_json_and_url_errors_preserve_foreign_sources() {
        let json = YfError::json(
            serde_json::from_str::<serde_json::Value>("{").expect_err("invalid JSON"),
        );
        assert!(matches!(json, YfError::Json(_)));
        assert_source::<serde_json::Error>(&json);

        let url = YfError::url(url::Url::parse("://").expect_err("invalid URL"));
        assert!(matches!(url, YfError::Url(_)));
        assert_source::<url::ParseError>(&url);
    }

    #[cfg(feature = "stream")]
    #[test]
    fn opaque_stream_errors_preserve_foreign_sources() {
        use base64::{Engine as _, engine::general_purpose};
        use prost::DecodeError;

        let websocket = YfError::websocket(tokio_tungstenite::tungstenite::Error::ConnectionClosed);
        assert!(matches!(websocket, YfError::Websocket(_)));
        assert_source::<tokio_tungstenite::tungstenite::Error>(&websocket);

        let protobuf = YfError::protobuf(DecodeError::new("invalid protobuf"));
        assert!(matches!(protobuf, YfError::Protobuf(_)));
        assert_source::<prost::DecodeError>(&protobuf);

        let base64 = YfError::base64(
            general_purpose::STANDARD
                .decode("!")
                .expect_err("invalid base64"),
        );
        assert!(matches!(base64, YfError::Base64(_)));
        assert_source::<base64::DecodeError>(&base64);
    }
}
