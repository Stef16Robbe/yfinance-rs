//! Internal Yahoo-to-`paft` conversion utilities.
//!
//! This module is public only so integration tests can share the crate's adapter
//! helpers. It is not part of the stable user-facing API.

use chrono::{DateTime, Utc};
use paft::Decimal;
use paft::domain::{AssetKind, Exchange, MarketState, Period};
use paft::fundamentals::analysis::{RecommendationAction, RecommendationGrade};
use paft::fundamentals::holders::{InsiderPosition, TransactionType};
use paft::fundamentals::profile::FundKind;
use paft::money::{Currency, MonetaryAmount, Money, Price};
use rust_decimal::prelude::ToPrimitive;
use std::{fmt::Display, str::FromStr};

use crate::{YfError, core::currency_resolver::ResolvedCurrencyUnit};

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
    let unit = currency_str.and_then(ResolvedCurrencyUnit::from_code)?;
    unit.money_from_f64(value)
}

/// Convert an exact decimal amount to `Money` with a parsed currency string.
#[must_use]
pub fn money_from_decimal_with_currency_str(
    value: Decimal,
    currency_str: Option<&str>,
) -> Option<Money> {
    let unit = currency_str.and_then(ResolvedCurrencyUnit::from_code)?;
    unit.money_from_decimal(value).ok()
}

/// Convert a finite `f64` to `Price` with specified currency.
#[must_use]
pub fn price_from_f64(value: f64, currency: Currency) -> Option<Price> {
    decimal_from_f64(value).map(|decimal| Price::new(decimal, currency))
}

/// Convert a finite `f64` to `Price` with a parsed currency string.
#[must_use]
pub fn price_from_f64_with_currency_str(value: f64, currency_str: Option<&str>) -> Option<Price> {
    let unit = currency_str.and_then(ResolvedCurrencyUnit::from_code)?;
    unit.price_from_f64(value)
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
///
/// # Errors
/// Returns `YfError::InvalidData` when the timestamp is outside chrono's
/// representable range.
pub fn i64_to_datetime(timestamp: i64) -> Result<DateTime<Utc>, YfError> {
    DateTime::from_timestamp(timestamp, 0)
        .ok_or_else(|| YfError::InvalidData(format!("invalid Unix timestamp: {timestamp}")))
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
pub fn string_to_fund_kind(s: Option<String>) -> Result<Option<FundKind>, YfError> {
    s.map(|s| {
        // Map Yahoo Finance legal types to paft FundKind values
        match s.as_str() {
            "Exchange Traded Fund" => Ok(FundKind::Etf),
            "Mutual Fund" => Ok(FundKind::MutualFund),
            "Index Fund" => Ok(FundKind::IndexFund),
            "Closed-End Fund" => Ok(FundKind::ClosedEndFund),
            "Money Market Fund" => Ok(FundKind::MoneyMarketFund),
            "Hedge Fund" => Ok(FundKind::HedgeFund),
            "Real Estate Investment Trust" => Ok(FundKind::Reit),
            "Unit Investment Trust" => Ok(FundKind::UnitInvestmentTrust),
            _ => parse_required_token(&s, "fund kind"),
        }
    })
    .transpose()
}

/// Convert `FundKind` to String
#[must_use]
pub fn fund_kind_to_string(kind: Option<FundKind>) -> Option<String> {
    kind.map(|k| k.to_string())
}

/// Convert String to `InsiderPosition` enum
pub fn string_to_insider_position(s: &str) -> Result<InsiderPosition, YfError> {
    parse_required_token(s, "insider position")
}

/// Convert String to `TransactionType` enum
pub fn string_to_transaction_type(s: &str) -> Result<TransactionType, YfError> {
    parse_required_token(s, "insider transaction type")
}

/// Convert String to Period
pub fn string_to_period(s: &str) -> Result<Period, YfError> {
    parse_required_token(s, "period")
}

/// Convert String to `RecommendationGrade` enum
pub fn string_to_recommendation_grade(s: &str) -> Result<RecommendationGrade, YfError> {
    parse_required_token(s, "recommendation grade")
}

/// Convert String to `RecommendationAction` enum
pub fn string_to_recommendation_action(s: &str) -> Result<RecommendationAction, YfError> {
    parse_required_token(s, "recommendation action")
}

/// Convert a Yahoo quote type / asset kind string to `AssetKind`.
pub fn string_to_asset_kind(s: &str) -> Result<AssetKind, YfError> {
    match s.trim() {
        "ETF" | "MUTUALFUND" | "MUTUAL_FUND" => Ok(AssetKind::Fund),
        "INDEX" => Ok(AssetKind::Index),
        "CRYPTOCURRENCY" => Ok(AssetKind::Crypto),
        "CURRENCY" => Ok(AssetKind::Forex),
        token => parse_required_token(token, "asset kind"),
    }
}

fn parse_required_token<T>(s: &str, name: &str) -> Result<T, YfError>
where
    T: FromStr,
    T::Err: Display,
{
    let token = s.trim();
    if token.is_empty() {
        return Err(YfError::MissingData(format!("{name} missing")));
    }

    token
        .parse()
        .map_err(|err| YfError::InvalidData(format!("invalid {name} {s:?}: {err}")))
}
