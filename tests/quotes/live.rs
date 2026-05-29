/// Live recorder to create `tests/fixtures/quote_v7_MULTI.json` via `internal::net::get_text`.
/// Run it explicitly with recording turned on:
///   `YF_RECORD=1` cargo test --test quotes -- --ignored `record_multi_quotes_live`
use url::Url;
use yfinance_rs::{ProjectionIssue, YfWarning};

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn record_multi_quotes_live() {
    let url = Url::parse("https://query1.finance.yahoo.com/v7/finance/quote").unwrap();
    let client = yfinance_rs::YfClient::builder()
        .base_quote_v7(url)
        .build()
        .unwrap();

    // Use the real base URL; this will record to quote_v7_MULTI.json
    let _ = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL", "MSFT"])
        .fetch()
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_aapl_exchange_candidates_do_not_warn_after_valid_primary_candidate() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let response = yfinance_rs::QuotesBuilder::new(&client)
        .symbols(["AAPL"])
        .fetch_with_diagnostics()
        .await
        .unwrap();

    assert_eq!(response.data.len(), 1);
    assert_eq!(
        response.data[0]
            .instrument
            .exchange
            .as_ref()
            .map(std::string::ToString::to_string)
            .as_deref(),
        Some("NASDAQ")
    );
    assert!(
        !response
            .diagnostics
            .warnings
            .iter()
            .any(is_exchange_candidate_warning),
        "valid live AAPL exchange metadata should not emit lower-priority exchange warnings"
    );
}

fn is_exchange_candidate_warning(warning: &YfWarning) -> bool {
    matches!(
        warning,
        YfWarning::OmittedPresentField {
            path: "fullExchangeName" | "exchange" | "market" | "marketCapFigureExchange",
            reason: ProjectionIssue::InvalidField { .. },
            ..
        }
    )
}
