//! Internal Yahoo-to-`paft` conversion utilities.
//!
//! This module is public only so integration tests can share the crate's adapter
//! helpers. It is not part of the stable user-facing API.

use chrono::{DateTime, Utc};
use paft::Decimal;
use paft::domain::{Exchange, MarketState, Period};
use paft::fundamentals::analysis::{RecommendationAction, RecommendationGrade};
use paft::fundamentals::holders::{InsiderPosition, TransactionType};
use paft::fundamentals::profile::FundKind;
use paft::money::{Currency, IsoCurrency, MonetaryAmount, Money, Price};
use rust_decimal::prelude::ToPrimitive;
use std::str::FromStr;

use crate::YfError;

/// Converts a finite `f64` value to `Decimal`.
///
/// Returns `None` if the value is non-finite or does not fit in the decimal
/// backend.
#[must_use]
pub fn decimal_from_f64(value: f64) -> Option<Decimal> {
    value
        .is_finite()
        .then_some(value)
        .and_then(|value| Decimal::try_from(value).ok())
}

/// Convert a finite `f64` to `Money` with specified currency.
///
/// Returns `None` if the value is non-finite, does not fit in the decimal
/// backend, or currency metadata is unavailable.
#[must_use]
pub fn money_from_f64(value: f64, currency: Currency) -> Option<Money> {
    decimal_from_f64(value).and_then(|decimal| Money::new(decimal, currency).ok())
}

/// Convert i64 to Money with specified currency (no precision loss).
///
/// # Errors
/// Returns `YfError::Money` if currency metadata is not registered for the
/// provided currency.
pub fn i64_to_money_with_currency(value: i64, currency: Currency) -> Result<Money, YfError> {
    let decimal = rust_decimal::Decimal::from_i128_with_scale(i128::from(value), 0);
    Ok(Money::new(decimal, currency)?)
}

/// Convert u64 to Money with specified currency (no precision loss).
///
/// # Errors
/// Returns `YfError::Money` if currency metadata is not registered for the
/// provided currency.
pub fn u64_to_money_with_currency(value: u64, currency: Currency) -> Result<Money, YfError> {
    let decimal = rust_decimal::Decimal::from_i128_with_scale(i128::from(value), 0);
    Ok(Money::new(decimal, currency)?)
}

/// Convert a finite `f64` to `Money` with a parsed currency string.
#[must_use]
pub fn money_from_f64_with_currency_str(value: f64, currency_str: Option<&str>) -> Option<Money> {
    let currency = currency_str
        .and_then(|s| Currency::from_str(s).ok())
        .unwrap_or(Currency::Iso(IsoCurrency::USD));
    money_from_f64(value, currency)
}

/// Convert a finite `f64` to `Price` with specified currency.
#[must_use]
pub fn price_from_f64(value: f64, currency: Currency) -> Option<Price> {
    decimal_from_f64(value).map(|decimal| Price::new(decimal, currency))
}

/// Convert a finite `f64` to `Price` with a parsed currency string.
#[must_use]
pub fn price_from_f64_with_currency_str(value: f64, currency_str: Option<&str>) -> Option<Price> {
    let currency = currency_str
        .and_then(|s| Currency::from_str(s).ok())
        .unwrap_or(Currency::Iso(IsoCurrency::USD));
    price_from_f64(value, currency)
}

/// Currency-denominated value that exposes a decimal amount and currency.
pub trait CurrencyValue {
    /// Returns the decimal amount.
    fn amount(&self) -> Decimal;

    /// Returns the associated currency.
    fn currency(&self) -> &Currency;
}

impl CurrencyValue for Money {
    fn amount(&self) -> Decimal {
        self.amount()
    }

    fn currency(&self) -> &Currency {
        self.currency()
    }
}

impl CurrencyValue for Price {
    fn amount(&self) -> Decimal {
        self.amount()
    }

    fn currency(&self) -> &Currency {
        self.currency()
    }
}

impl CurrencyValue for MonetaryAmount {
    fn amount(&self) -> Decimal {
        self.amount()
    }

    fn currency(&self) -> &Currency {
        self.currency()
    }
}

/// Convert a currency-denominated value to f64 (loses currency information).
#[must_use]
pub fn f64_from_currency_value(value: &impl CurrencyValue) -> Option<f64> {
    value.amount().to_f64()
}

/// Test convenience for converting ordinary currency values to `f64`.
///
/// This is intentionally hidden and should not be used by production mappings,
/// where failed conversion should be represented explicitly.
#[must_use]
pub fn money_to_f64(value: &impl CurrencyValue) -> f64 {
    f64_from_currency_value(value).expect("currency value should fit in f64")
}

/// Extract currency string from a currency-denominated value.
#[must_use]
pub fn money_to_currency_str(value: &impl CurrencyValue) -> Option<String> {
    Some(value.currency().to_string())
}

/// Convert i64 timestamp to `DateTime`<Utc>
#[must_use]
pub fn i64_to_datetime(timestamp: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(timestamp, 0).unwrap_or_default()
}

/// Convert `DateTime`<Utc> to i64 timestamp
#[must_use]
pub const fn datetime_to_i64(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

/// Convert String to Exchange enum
#[allow(clippy::single_option_map)]
#[must_use]
pub fn string_to_exchange(s: Option<String>) -> Option<Exchange> {
    s.and_then(|s| {
        // Map Yahoo Finance exchange names to paft Exchange values
        match s.as_str() {
            "NasdaqGS" | "NasdaqCM" | "NasdaqGM" => Some(Exchange::NASDAQ),
            "NYSE" => Some(Exchange::NYSE),
            "AMEX" => Some(Exchange::AMEX),
            "BATS" => Some(Exchange::BATS),
            "OTC" => Some(Exchange::OTC),
            "LSE" => Some(Exchange::LSE),
            "TSE" => Some(Exchange::TSE),
            "HKEX" => Some(Exchange::HKEX),
            "SSE" => Some(Exchange::SSE),
            "SZSE" => Some(Exchange::SZSE),
            "TSX" => Some(Exchange::TSX),
            "ASX" => Some(Exchange::ASX),
            "Euronext" => Some(Exchange::Euronext),
            "XETRA" => Some(Exchange::XETRA),
            "SIX" => Some(Exchange::SIX),
            "BIT" => Some(Exchange::BIT),
            "BME" => Some(Exchange::BME),
            "AEX" => Some(Exchange::AEX),
            "BRU" => Some(Exchange::BRU),
            "LIS" => Some(Exchange::LIS),
            "EPA" => Some(Exchange::EPA),
            "OSL" => Some(Exchange::OSL),
            "STO" => Some(Exchange::STO),
            "CPH" => Some(Exchange::CPH),
            "WSE" => Some(Exchange::WSE),
            "PSE" => Some(Exchange::PSE),
            "BSE" => Some(Exchange::BSE),
            "MOEX" => Some(Exchange::MOEX),
            "BIST" => Some(Exchange::BIST),
            "JSE" => Some(Exchange::JSE),
            "TASE" => Some(Exchange::TASE),
            "BSE_HU" => Some(Exchange::BSE_HU),
            "NSE" => Some(Exchange::NSE),
            "KRX" => Some(Exchange::KRX),
            "SGX" => Some(Exchange::SGX),
            "SET" => Some(Exchange::SET),
            "KLSE" => Some(Exchange::KLSE),
            "PSE_CZ" => Some(Exchange::PSE_CZ),
            "IDX" => Some(Exchange::IDX),
            "HOSE" => Some(Exchange::HOSE),
            _ => Exchange::try_from(s).ok(),
        }
    })
}

/// Convert Exchange to String
#[must_use]
pub fn exchange_to_string(exchange: Option<Exchange>) -> Option<String> {
    exchange.map(|e| e.to_string())
}

/// Convert String to `MarketState` enum
#[must_use]
pub fn string_to_market_state(s: Option<String>) -> Option<MarketState> {
    s.and_then(|s| s.parse().ok())
}

/// Convert `MarketState` to String
#[must_use]
pub fn market_state_to_string(state: Option<MarketState>) -> Option<String> {
    state.map(|s| s.to_string())
}

/// Convert String to `FundKind` enum
#[allow(clippy::single_option_map)]
#[must_use]
pub fn string_to_fund_kind(s: Option<String>) -> Option<FundKind> {
    s.and_then(|s| {
        // Map Yahoo Finance legal types to paft FundKind values
        match s.as_str() {
            "Exchange Traded Fund" => Some(FundKind::Etf),
            "Mutual Fund" => Some(FundKind::MutualFund),
            "Index Fund" => Some(FundKind::IndexFund),
            "Closed-End Fund" => Some(FundKind::ClosedEndFund),
            "Money Market Fund" => Some(FundKind::MoneyMarketFund),
            "Hedge Fund" => Some(FundKind::HedgeFund),
            "Real Estate Investment Trust" => Some(FundKind::Reit),
            "Unit Investment Trust" => Some(FundKind::UnitInvestmentTrust),
            _ => FundKind::try_from(s).ok(),
        }
    })
}

/// Convert `FundKind` to String
#[must_use]
pub fn fund_kind_to_string(kind: Option<FundKind>) -> Option<String> {
    kind.map(|k| k.to_string())
}

/// Convert String to `InsiderPosition` enum
#[must_use]
pub fn string_to_insider_position(s: &str) -> InsiderPosition {
    let token = s.trim();
    let token_nonempty = if token.is_empty() { "UNKNOWN" } else { token };
    token_nonempty.parse().unwrap_or(InsiderPosition::Officer)
}

/// Convert String to `TransactionType` enum
#[must_use]
pub fn string_to_transaction_type(s: &str) -> TransactionType {
    let token = s.trim();
    let token_nonempty = if token.is_empty() { "UNKNOWN" } else { token };
    token_nonempty.parse().unwrap_or(TransactionType::Buy)
}

/// Convert String to Period
#[must_use]
pub fn string_to_period(s: &str) -> Period {
    if s.trim().is_empty() {
        return "UNKNOWN".parse().map_or(Period::Year { year: 1970 }, |p| p);
    }
    s.parse()
        .unwrap_or_else(|_| "UNKNOWN".parse().map_or(Period::Year { year: 1970 }, |p| p))
}

/// Convert String to `RecommendationGrade` enum
#[must_use]
pub fn string_to_recommendation_grade(s: &str) -> RecommendationGrade {
    let token = s.trim();
    let token_nonempty = if token.is_empty() { "UNKNOWN" } else { token };
    token_nonempty.parse().unwrap_or(RecommendationGrade::Hold)
}

/// Convert String to `RecommendationAction` enum
#[must_use]
pub fn string_to_recommendation_action(s: &str) -> RecommendationAction {
    let token = s.trim();
    let token_nonempty = if token.is_empty() { "UNKNOWN" } else { token };
    token_nonempty
        .parse()
        .unwrap_or(RecommendationAction::Maintain)
}
