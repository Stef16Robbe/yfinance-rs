use url::Url;

use super::YfClient;
use crate::core::YfError;

#[derive(Clone, Copy, Debug)]
pub enum SymbolEndpoint {
    Chart,
    OptionsV7,
    QuoteSummary,
    Timeseries,
}

impl YfClient {
    pub(crate) fn symbol_url(
        &self,
        endpoint: SymbolEndpoint,
        symbol: &str,
    ) -> Result<Url, YfError> {
        let base = match endpoint {
            SymbolEndpoint::Chart => &self.base_chart,
            SymbolEndpoint::OptionsV7 => &self.base_options_v7,
            SymbolEndpoint::QuoteSummary => &self.base_quote_api,
            SymbolEndpoint::Timeseries => &self.base_timeseries,
        };

        append_symbol_path_segment(base, symbol)
    }
}

pub fn normalize_symbol(symbol: &str) -> Result<String, YfError> {
    let symbol = paft::domain::Symbol::new(symbol)
        .map_err(|err| YfError::InvalidParams(format!("invalid symbol {symbol:?}: {err}")))?;
    let symbol = symbol.as_str();

    if matches!(symbol, "." | "..") {
        return Err(YfError::InvalidParams(
            "symbol cannot be a dot path segment".into(),
        ));
    }

    if !symbol.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
        return Err(YfError::InvalidParams(
            "symbol must contain at least one ASCII alphanumeric character".into(),
        ));
    }

    if !symbol.bytes().all(is_yahoo_symbol_byte) {
        return Err(YfError::InvalidParams(format!(
            "symbol {symbol:?} contains characters unsupported by Yahoo Finance"
        )));
    }

    Ok(symbol.to_string())
}

pub fn normalize_symbols<'a>(
    symbols: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<String>, YfError> {
    symbols.into_iter().map(normalize_symbol).collect()
}

const fn is_yahoo_symbol_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'=' | b'^')
}

fn append_symbol_path_segment(base: &Url, symbol: &str) -> Result<Url, YfError> {
    let symbol = normalize_symbol(symbol)?;

    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| YfError::InvalidParams("base URL cannot accept path segments".into()))?
        .pop_if_empty()
        .push(&symbol);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_segment_keeps_supported_punctuation_without_retargeting_url() {
        let base = Url::parse("https://query1.finance.yahoo.com/v8/finance/chart/").unwrap();

        let url = append_symbol_path_segment(&base, "^gspc").unwrap();

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("query1.finance.yahoo.com"));
        assert_eq!(url.path(), "/v8/finance/chart/^GSPC");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn trailing_slash_base_still_gets_one_symbol_separator() {
        let base =
            Url::parse("https://query1.finance.yahoo.com/v10/finance/quoteSummary/").unwrap();

        let url = append_symbol_path_segment(&base, "BRK.B").unwrap();

        assert_eq!(
            url.as_str(),
            "https://query1.finance.yahoo.com/v10/finance/quoteSummary/BRK.B"
        );
    }

    #[test]
    fn rejects_dot_segments_that_url_would_drop() {
        let base = Url::parse("https://query1.finance.yahoo.com/v8/finance/chart/").unwrap();

        assert!(append_symbol_path_segment(&base, ".").is_err());
        assert!(append_symbol_path_segment(&base, "..").is_err());
    }

    #[test]
    fn rejects_empty_whitespace_and_url_syntax_symbols() {
        let base = Url::parse("https://query1.finance.yahoo.com/v8/finance/chart/").unwrap();

        assert!(append_symbol_path_segment(&base, "").is_err());
        assert!(append_symbol_path_segment(&base, " \t ").is_err());
        assert!(append_symbol_path_segment(&base, "https://evil.example/AAPL?x=1#frag").is_err());
    }

    #[test]
    fn normalize_symbol_trims_and_uppercases_supported_yahoo_symbols() {
        assert_eq!(normalize_symbol(" brk.b ").unwrap(), "BRK.B");
        assert_eq!(normalize_symbol("eurusd=x").unwrap(), "EURUSD=X");
        assert_eq!(normalize_symbol("btc-usd").unwrap(), "BTC-USD");
    }
}
