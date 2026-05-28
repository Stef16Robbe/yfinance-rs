use url::Url;

use super::YfClient;
use crate::core::YfError;

#[derive(Clone, Copy, Debug)]
pub enum SymbolEndpoint {
    Chart,
    OptionsV7,
    Quote,
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
            SymbolEndpoint::Quote => &self.base_quote,
            SymbolEndpoint::QuoteSummary => &self.base_quote_api,
            SymbolEndpoint::Timeseries => &self.base_timeseries,
        };

        append_symbol_path_segment(base, symbol)
    }
}

fn append_symbol_path_segment(base: &Url, symbol: &str) -> Result<Url, YfError> {
    if matches!(symbol, "." | "..") {
        return Err(YfError::InvalidParams(
            "symbol cannot be a dot path segment".into(),
        ));
    }

    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|()| YfError::InvalidParams("base URL cannot accept path segments".into()))?
        .pop_if_empty()
        .push(symbol);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_segment_is_encoded_without_retargeting_url() {
        let base = Url::parse("https://query1.finance.yahoo.com/v8/finance/chart/").unwrap();

        let url = append_symbol_path_segment(&base, "https://evil.example/AAPL?x=1#frag").unwrap();

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("query1.finance.yahoo.com"));
        assert_eq!(
            url.path(),
            "/v8/finance/chart/https:%2F%2Fevil.example%2FAAPL%3Fx=1%23frag"
        );
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
}
