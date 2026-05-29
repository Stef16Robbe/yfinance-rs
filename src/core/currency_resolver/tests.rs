use super::{
    AnalystEstimateCurrencyEvidence, CorporateActionCurrencyEvidence, CurrencyHints, CurrencyKind,
    ReportingCurrencyEvidence, ResolvedCurrency, ResolvedCurrencyUnit, TradingCurrencyEvidence,
    hints::CurrencyHintField,
    types::{CurrencySource, EvidenceStrength},
};
use crate::core::{YfClient, client::CacheMode};
use httpmock::{Method::GET, MockServer};
use paft::Decimal;
use paft::money::{Currency, IsoCurrency};
use url::Url;

fn unit(code: &str) -> ResolvedCurrencyUnit {
    ResolvedCurrencyUnit::from_code(code).expect("valid test currency")
}

fn currency(unit: &ResolvedCurrencyUnit) -> Currency {
    unit.money_from_i64(1)
        .expect("known-good test money")
        .currency()
        .clone()
}

#[tokio::test]
async fn direct_provider_replaces_weaker_profile_cache() {
    let client = YfClient::default();
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Reporting,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::ProfileCountryHeuristic,
                EvidenceStrength::ProfileHeuristic,
            ),
        )
        .await;
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Reporting,
            ResolvedCurrency::new(
                unit("USD"),
                CurrencySource::DirectProvider,
                EvidenceStrength::DirectProvider,
            ),
        )
        .await;
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Reporting,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::ProfileCountryHeuristic,
                EvidenceStrength::ProfileHeuristic,
            ),
        )
        .await;

    let resolved = client
        .cached_resolved_currency("TEST", CurrencyKind::Reporting)
        .await
        .expect("cached currency");
    assert_eq!(currency(&resolved.unit), Currency::Iso(IsoCurrency::USD));
}

#[tokio::test]
async fn override_resolution_does_not_poison_inferred_cache() {
    let client = YfClient::default();
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Reporting,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::ProfileCountryHeuristic,
                EvidenceStrength::ProfileHeuristic,
            ),
        )
        .await;

    let override_unit = client
        .resolve_reporting_currency_unit(
            "TEST",
            Some(Currency::Iso(IsoCurrency::USD)),
            ReportingCurrencyEvidence::None,
            CacheMode::Use,
            None,
        )
        .await
        .expect("override currency");
    assert_eq!(currency(&override_unit), Currency::Iso(IsoCurrency::USD));

    let inferred_unit = client
        .resolve_reporting_currency_unit(
            "TEST",
            None,
            ReportingCurrencyEvidence::None,
            CacheMode::Use,
            None,
        )
        .await
        .expect("inferred currency");
    assert_eq!(currency(&inferred_unit), Currency::Iso(IsoCurrency::GBP));
}

#[tokio::test]
async fn provider_hint_replaces_cached_listing_heuristic() {
    let client = YfClient::default();
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Trading,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::ListingHeuristic,
                EvidenceStrength::ListingHeuristic,
            ),
        )
        .await;
    client
        .store_currency_hints(
            "TEST",
            CurrencyHints::from_quote(Some("USD"), None, None, None, None),
        )
        .await;

    let resolved = client
        .resolve_trading_currency_unit(
            "TEST",
            None,
            TradingCurrencyEvidence::None,
            CacheMode::Use,
            None,
        )
        .await
        .expect("provider hint should replace heuristic cache");

    assert_eq!(currency(&resolved), Currency::Iso(IsoCurrency::USD));
}

#[tokio::test]
async fn cached_reporting_profile_heuristic_retries_unknown_enrichment() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "TEST");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"TEST","quoteType":"EQUITY","financialCurrency":"USD"}],"error":null}}"#,
            );
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Reporting,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::ProfileCountryHeuristic,
                EvidenceStrength::ProfileHeuristic,
            ),
        )
        .await;

    let resolved = client
        .resolve_reporting_currency_unit(
            "TEST",
            None,
            ReportingCurrencyEvidence::None,
            CacheMode::Use,
            None,
        )
        .await
        .expect("quote enrichment should replace profile heuristic");

    assert_eq!(quote_mock.calls(), 1);
    assert_eq!(currency(&resolved), Currency::Iso(IsoCurrency::USD));
}

#[tokio::test]
async fn contextual_currency_kinds_use_typed_cache() {
    let client = YfClient::default();
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::AnalystEstimate,
            ResolvedCurrency::new(
                unit("EUR"),
                CurrencySource::DirectProvider,
                EvidenceStrength::DirectProvider,
            ),
        )
        .await;
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::CorporateAction,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::DirectProvider,
                EvidenceStrength::DirectProvider,
            ),
        )
        .await;

    let analyst_unit = client
        .resolve_analyst_estimate_currency_unit(
            "TEST",
            None,
            AnalystEstimateCurrencyEvidence::Earnings(None),
            CacheMode::Use,
            None,
        )
        .await
        .expect("cached analyst estimate currency");
    assert_eq!(currency(&analyst_unit), Currency::Iso(IsoCurrency::EUR));

    let action_unit = client
        .resolve_corporate_action_currency_unit(
            "TEST",
            None,
            CorporateActionCurrencyEvidence::ChartMeta(None),
            CacheMode::Use,
            None,
        )
        .await
        .expect("cached corporate action currency");
    assert_eq!(currency(&action_unit), Currency::Iso(IsoCurrency::GBP));
}

#[tokio::test]
async fn analyst_direct_currency_does_not_poison_symbol_cache() {
    let client = YfClient::default();
    client
        .store_resolved_currency(
            "TEST",
            CurrencyKind::Reporting,
            ResolvedCurrency::new(
                unit("GBP"),
                CurrencySource::DirectProvider,
                EvidenceStrength::DirectProvider,
            ),
        )
        .await;

    let direct = client
        .resolve_analyst_estimate_currency_unit(
            "TEST",
            None,
            AnalystEstimateCurrencyEvidence::Earnings(Some("USD")),
            CacheMode::Use,
            None,
        )
        .await
        .expect("direct analyst currency");
    assert_eq!(currency(&direct), Currency::Iso(IsoCurrency::USD));
    assert!(
        client
            .cached_resolved_currency("TEST", CurrencyKind::AnalystEstimate)
            .await
            .is_none()
    );

    let fallback = client
        .resolve_analyst_estimate_currency_unit(
            "TEST",
            None,
            AnalystEstimateCurrencyEvidence::Earnings(None),
            CacheMode::Use,
            None,
        )
        .await
        .expect("reporting fallback");
    assert_eq!(currency(&fallback), Currency::Iso(IsoCurrency::GBP));
}

#[tokio::test]
async fn invalid_direct_currency_is_invalid_data() {
    let client = YfClient::default();
    let err = client
        .resolve_reporting_currency_unit(
            "TEST",
            None,
            ReportingCurrencyEvidence::FinancialCurrency(Some("!!!")),
            CacheMode::Use,
            None,
        )
        .await
        .expect_err("invalid direct currency should fail");

    assert!(matches!(err, crate::core::YfError::InvalidData(_)));
}

#[tokio::test]
async fn invalid_enriched_currency_is_invalid_data() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "BAD");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"BAD","quoteType":"EQUITY","currency":"!!!"}],"error":null}}"#,
            );
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let err = client
        .resolve_trading_currency_unit(
            "BAD",
            None,
            TradingCurrencyEvidence::None,
            CacheMode::Use,
            None,
        )
        .await
        .expect_err("invalid enriched currency should fail");

    assert_eq!(quote_mock.calls(), 1);
    assert!(matches!(err, crate::core::YfError::InvalidData(_)));
    assert!(
        !client
            .cached_currency_hints("BAD")
            .await
            .hint(CurrencyHintField::Quote)
            .is_unknown()
    );
}

#[tokio::test]
async fn listing_inference_uses_yahoo_quote_units() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "TSCO.L");
        then.status(500);
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    let unit = client
        .resolve_trading_currency_unit(
            "TSCO.L",
            None,
            TradingCurrencyEvidence::None,
            CacheMode::Use,
            None,
        )
        .await
        .expect("listing fallback currency");
    let price = unit
        .price_from_f64(123.0)
        .expect("scaled listing fallback price");

    assert!(quote_mock.calls() >= 1);
    assert_eq!(price.currency(), &Currency::Iso(IsoCurrency::GBP));
    assert_eq!(price.amount(), Decimal::new(123, 2));
}

#[tokio::test]
async fn failed_quote_enrichment_is_not_cached() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MISS");
        then.status(500);
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    client
        .enrich_quote_hints("MISS", CacheMode::Use, None)
        .await;
    client
        .enrich_quote_hints("MISS", CacheMode::Use, None)
        .await;

    assert!(quote_mock.calls() > 1);
    assert!(
        client
            .cached_currency_hints("MISS")
            .await
            .hint(CurrencyHintField::Quote)
            .is_unknown()
    );
}

#[tokio::test]
async fn successful_missing_quote_currency_is_cached() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MISS");
        then.status(200).header("content-type", "application/json").body(
            r#"{"quoteResponse":{"result":[{"symbol":"MISS","quoteType":"EQUITY"}],"error":null}}"#,
        );
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    client
        .enrich_quote_hints("MISS", CacheMode::Use, None)
        .await;
    client
        .enrich_quote_hints("MISS", CacheMode::Use, None)
        .await;

    assert_eq!(quote_mock.calls(), 1);
    assert!(
        !client
            .cached_currency_hints("MISS")
            .await
            .hint(CurrencyHintField::Quote)
            .is_unknown()
    );
}

#[tokio::test]
async fn successful_empty_quote_response_caches_requested_symbol_missing() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MISS");
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"quoteResponse":{"result":[],"error":null}}"#);
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    client
        .enrich_quote_hints("MISS", CacheMode::Use, None)
        .await;
    client
        .enrich_quote_hints("MISS", CacheMode::Use, None)
        .await;

    assert_eq!(quote_mock.calls(), 1);
    let hints = client.cached_currency_hints("MISS").await;
    assert!(!hints.hint(CurrencyHintField::Quote).is_unknown());
    assert!(!hints.hint(CurrencyHintField::Financial).is_unknown());
}

#[tokio::test]
async fn successful_normalized_quote_response_caches_requested_symbol_key() {
    let server = MockServer::start();
    let quote_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v7/finance/quote")
            .query_param("symbols", "MISS");
        then.status(200)
            .header("content-type", "application/json")
            .body(
                r#"{"quoteResponse":{"result":[{"symbol":"MISS","quoteType":"EQUITY","currency":"USD"}],"error":null}}"#,
            );
    });
    let client = YfClient::builder()
        .base_quote_v7(Url::parse(&format!("{}/v7/finance/quote", server.base_url())).unwrap())
        .build()
        .unwrap();

    client
        .enrich_quote_hints("miss", CacheMode::Use, None)
        .await;
    client
        .enrich_quote_hints("miss", CacheMode::Use, None)
        .await;

    assert_eq!(quote_mock.calls(), 1);
    let hints = client.cached_currency_hints("miss").await;
    assert!(hints.hint(CurrencyHintField::Quote).present().is_some());
    assert!(!hints.hint(CurrencyHintField::Financial).is_unknown());
}
