use paft::domain::{AssetKind, Period};
use paft::fundamentals::{
    analysis::{RecommendationAction, RecommendationGrade},
    holders::{InsiderPosition, TransactionType},
    profile::FundKind,
};
use paft::money::{Currency, IsoCurrency};
use std::str::FromStr;
use yfinance_rs::YfError;
use yfinance_rs::core::conversions::{
    decimal_from_f64, i64_to_datetime, i64_to_money_with_currency, money_from_f64, price_from_f64,
    string_to_asset_kind, string_to_fund_kind, string_to_insider_position, string_to_period,
    string_to_recommendation_action, string_to_recommendation_grade, string_to_transaction_type,
    u64_to_money_with_currency,
};

const fn usd() -> Currency {
    Currency::Iso(IsoCurrency::USD)
}

#[test]
fn invalid_float_conversions_return_none() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e100] {
        assert!(decimal_from_f64(value).is_none());
        assert!(money_from_f64(value, usd()).is_none());
        assert!(price_from_f64(value, usd()).is_none());
    }
}

#[test]
fn valid_zero_remains_zero() {
    let price = price_from_f64(0.0, usd()).expect("zero is a valid price");
    assert_eq!(price.amount(), paft::Decimal::from(0));
}

#[test]
fn integer_money_conversions_return_errors_for_missing_currency_metadata() {
    let currency = Currency::from_str("ZZZ").unwrap();

    assert!(matches!(
        i64_to_money_with_currency(1, currency.clone()),
        Err(YfError::Money(_))
    ));
    assert!(matches!(
        u64_to_money_with_currency(1, currency),
        Err(YfError::Money(_))
    ));
}

#[test]
fn invalid_timestamps_return_errors() {
    assert!(i64_to_datetime(0).is_ok());
    assert!(matches!(
        i64_to_datetime(i64::MAX),
        Err(YfError::InvalidData(_))
    ));
}

#[test]
fn missing_or_invalid_required_tokens_do_not_default_to_plausible_values() {
    assert!(matches!(string_to_period(""), Err(YfError::MissingData(_))));
    assert!(matches!(
        string_to_recommendation_grade("!!!"),
        Err(YfError::InvalidData(_))
    ));
    assert!(matches!(
        string_to_recommendation_action("!!!"),
        Err(YfError::InvalidData(_))
    ));
    assert!(matches!(
        string_to_insider_position(""),
        Err(YfError::MissingData(_))
    ));
    assert!(matches!(
        string_to_transaction_type(""),
        Err(YfError::MissingData(_))
    ));
    assert!(matches!(
        string_to_asset_kind(""),
        Err(YfError::MissingData(_))
    ));
    assert!(string_to_fund_kind(None).unwrap().is_none());
}

#[test]
fn unknown_nonempty_tokens_are_preserved_as_extensible_other_values() {
    assert!(matches!(
        string_to_period("current quarter").unwrap(),
        Period::Other(_)
    ));
    assert!(matches!(
        string_to_recommendation_grade("conviction buy").unwrap(),
        RecommendationGrade::Other(_)
    ));
    assert!(matches!(
        string_to_recommendation_action("price target raised only").unwrap(),
        RecommendationAction::Other(_)
    ));
    assert!(matches!(
        string_to_insider_position("chief product officer").unwrap(),
        InsiderPosition::Other(_)
    ));
    assert!(matches!(
        string_to_transaction_type("tax withholding").unwrap(),
        TransactionType::Other(_)
    ));
    assert!(matches!(
        string_to_asset_kind("structured note").unwrap(),
        AssetKind::Other(_)
    ));
    assert!(matches!(
        string_to_fund_kind(Some("Interval Fund".to_string()))
            .unwrap()
            .unwrap(),
        FundKind::Other(_)
    ));
}
