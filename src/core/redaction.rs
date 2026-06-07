use std::fmt;

use url::Url;

const REDACTED: &str = "REDACTED";

#[derive(Clone, Copy)]
pub struct RedactedUrl<'a> {
    url: &'a Url,
}

impl<'a> RedactedUrl<'a> {
    pub const fn new(url: &'a Url) -> Self {
        Self { url }
    }
}

impl fmt::Display for RedactedUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&redact_url(self.url))
    }
}

impl fmt::Debug for RedactedUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(feature = "tracing")]
pub struct RedactedDisplay<'a, T: fmt::Display + ?Sized> {
    value: &'a T,
}

#[cfg(feature = "tracing")]
impl<'a, T: fmt::Display + ?Sized> RedactedDisplay<'a, T> {
    pub const fn new(value: &'a T) -> Self {
        Self { value }
    }
}

#[cfg(feature = "tracing")]
impl<T: fmt::Display + ?Sized> fmt::Display for RedactedDisplay<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&redact_auth_query_params_in_text(&self.value.to_string()))
    }
}

#[cfg(feature = "tracing")]
impl<T: fmt::Display + ?Sized> fmt::Debug for RedactedDisplay<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

pub fn redact_url(url: &Url) -> String {
    let pairs = url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    if !pairs.iter().any(|(name, _)| is_sensitive_query_param(name)) {
        return url.as_str().to_owned();
    }

    let mut redacted = url.clone();
    redacted.set_query(None);
    {
        let mut query = redacted.query_pairs_mut();
        for (name, value) in pairs {
            query.append_pair(
                &name,
                if is_sensitive_query_param(&name) {
                    REDACTED
                } else {
                    &value
                },
            );
        }
    }

    redacted.to_string()
}

pub fn redact_auth_query_params_in_text(text: &str) -> String {
    let mut redacted = String::with_capacity(text.len());
    let mut cursor = 0;

    while let Some(start) = next_url_start(text, cursor) {
        redacted.push_str(&text[cursor..start]);
        let end = url_end(text, start);
        let candidate = &text[start..end];

        match Url::parse(candidate) {
            Ok(url) => redacted.push_str(&redact_url(&url)),
            Err(_) => redacted.push_str(candidate),
        }

        cursor = end;
    }

    redacted.push_str(&text[cursor..]);
    redacted
}

fn next_url_start(text: &str, from: usize) -> Option<usize> {
    const URL_PREFIXES: [&str; 4] = ["http://", "https://", "ws://", "wss://"];

    let haystack = &text[from..];
    URL_PREFIXES
        .iter()
        .filter_map(|prefix| haystack.find(prefix))
        .min()
        .map(|offset| from + offset)
}

fn url_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find_map(|(offset, ch)| is_url_terminator(ch).then_some(start + offset))
        .unwrap_or(text.len())
}

const fn is_url_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | ')' | ']' | '}')
}

fn is_sensitive_query_param(name: &str) -> bool {
    let normalized = name
        .chars()
        .map(|ch| match ch {
            '-' | '.' => '_',
            _ => ch.to_ascii_lowercase(),
        })
        .collect::<String>();

    matches!(
        normalized.as_str(),
        "apikey"
            | "api_key"
            | "auth"
            | "authorization"
            | "cookie"
            | "csrf"
            | "jwt"
            | "key"
            | "secret"
            | "session"
            | "sid"
            | "sig"
            | "signature"
            | "token"
            | "xsrf"
    ) || normalized.contains("crumb")
        || normalized.ends_with("_cookie")
        || normalized.ends_with("_key")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_signature")
        || normalized.ends_with("_token")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_url_masks_crumb_and_authish_params() {
        let url = Url::parse(
            "https://example.test/path?symbols=AAPL&crumb=secret&api_key=k&token=t&client-secret=s&period1=1",
        )
        .unwrap();

        let redacted = RedactedUrl::new(&url).to_string();

        assert!(redacted.contains("symbols=AAPL"));
        assert!(redacted.contains("crumb=REDACTED"));
        assert!(redacted.contains("api_key=REDACTED"));
        assert!(redacted.contains("token=REDACTED"));
        assert!(redacted.contains("client-secret=REDACTED"));
        assert!(redacted.contains("period1=1"));
        assert!(!redacted.contains("crumb=secret"));
        assert!(!redacted.contains("api_key=k"));
        assert!(!redacted.contains("token=t"));
        assert!(!redacted.contains("client-secret=s"));
    }

    #[test]
    fn redacted_text_masks_embedded_urls() {
        let text = "request failed for url (https://example.test/path?crumb=abc&symbols=AAPL)";

        let redacted = redact_auth_query_params_in_text(text);

        assert_eq!(
            redacted,
            "request failed for url (https://example.test/path?crumb=REDACTED&symbols=AAPL)"
        );
    }

    #[test]
    fn redacted_text_masks_crumb_after_comma_separated_query_param() {
        let text = "failed for https://query1.finance.yahoo.com/v10/finance/quoteSummary/AAPL?modules=a,b&crumb=s3cr3t";

        let redacted = redact_auth_query_params_in_text(text);

        assert!(!redacted.contains("s3cr3t"));
        assert!(redacted.contains("crumb=REDACTED"));
    }

    #[test]
    fn redacted_text_masks_websocket_urls() {
        let text = concat!(
            "websocket failed for ",
            "wss://streamer.finance.yahoo.com/?version=2&crumb=s3cr3t&token=t ",
            "while proxying ws://localhost/stream?symbols=AAPL&cookie=session"
        );

        let redacted = redact_auth_query_params_in_text(text);

        assert!(redacted.contains("wss://streamer.finance.yahoo.com/?version=2"));
        assert!(redacted.contains("crumb=REDACTED"));
        assert!(redacted.contains("token=REDACTED"));
        assert!(redacted.contains("ws://localhost/stream?symbols=AAPL"));
        assert!(redacted.contains("cookie=REDACTED"));
        assert!(!redacted.contains("s3cr3t"));
        assert!(!redacted.contains("token=t"));
        assert!(!redacted.contains("cookie=session"));
    }

    #[test]
    fn redacted_text_leaves_non_sensitive_urls_alone() {
        let text = "request failed for url (https://example.test/path?symbols=AAPL&period1=1)";

        let redacted = redact_auth_query_params_in_text(text);

        assert_eq!(redacted, text);
    }
}
