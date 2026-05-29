//! Yahoo Finance vocabulary normalization.
//!
//! This module is the adapter boundary for Yahoo-specific strings. Provider
//! codes and display names should be normalized here before being projected into
//! `paft` concepts.

use paft::domain::{AssetKind, Exchange};
use std::{fmt::Display, str::FromStr};

use crate::YfError;

/// Parse a Yahoo exchange token into a provider-agnostic exchange.
///
/// # Errors
/// Returns `YfError` if the token is empty or cannot be represented by `paft`.
pub fn parse_yahoo_exchange(s: &str) -> Result<Exchange, YfError> {
    let token = s.trim();
    if token.is_empty() {
        return Err(YfError::MissingData("exchange missing".into()));
    }
    if is_yahoo_market_scope(token) {
        return Err(YfError::InvalidData(format!(
            "invalid exchange {s:?}: Yahoo market scope is not an exchange"
        )));
    }

    match token.to_ascii_uppercase().as_str() {
        "NASDAQGS"
        | "NASDAQCM"
        | "NASDAQGM"
        | "NASDAQ GLOBAL SELECT"
        | "NASDAQ GLOBAL SELECT MARKET"
        | "NASDAQ GLOBAL MARKET"
        | "NASDAQ CAPITAL MARKET"
        | "NMS"
        | "NGM"
        | "NCM"
        | "NAS" => Ok(Exchange::NASDAQ),
        "NYSE" | "NEW YORK STOCK EXCHANGE" | "NYQ" => Ok(Exchange::NYSE),
        "AMEX" | "NYSE AMERICAN" | "NYSEAMERICAN" | "ASE" => Ok(Exchange::AMEX),
        "BATS" | "BTS" => Ok(Exchange::BATS),
        "OTC" | "OTCMKTS" | "PNK" | "OQB" | "OQX" | "OEM" => Ok(Exchange::OTC),
        "NYSEARCA" | "NYSE ARCA" | "NYSE_ARCA" | "PCX" => parse_exchange_token("NYSE_ARCA", s),
        "LSE" | "LONDON STOCK EXCHANGE" => Ok(Exchange::LSE),
        "TSE" | "TOKYO" | "TOKYO STOCK EXCHANGE" | "JPX" | "TYO" => Ok(Exchange::TSE),
        "HKEX" | "HONG KONG STOCK EXCHANGE" | "HKG" => Ok(Exchange::HKEX),
        "SSE" | "SHANGHAI STOCK EXCHANGE" | "SHH" => Ok(Exchange::SSE),
        "SZSE" | "SHENZHEN STOCK EXCHANGE" | "SHZ" => Ok(Exchange::SZSE),
        "TSX" | "TORONTO STOCK EXCHANGE" | "TOR" => Ok(Exchange::TSX),
        "ASX" | "AUSTRALIAN SECURITIES EXCHANGE" => Ok(Exchange::ASX),
        "EURONEXT" | "ENX" => Ok(Exchange::Euronext),
        "XETRA" | "GER" => Ok(Exchange::XETRA),
        "SIX" | "SWISS EXCHANGE" | "EBS" => Ok(Exchange::SIX),
        "BIT" | "BORSA ITALIANA" | "MIL" => Ok(Exchange::BIT),
        "BME" | "BOLSA DE MADRID" | "MAD" => Ok(Exchange::BME),
        "AEX" | "EURONEXT AMSTERDAM" | "AMS" => Ok(Exchange::AEX),
        "BRU" | "EURONEXT BRUSSELS" => Ok(Exchange::BRU),
        "LIS" | "EURONEXT LISBON" => Ok(Exchange::LIS),
        "EPA" | "EURONEXT PARIS" | "PAR" | "PARIS" => Ok(Exchange::EPA),
        "OSL" | "OSLO BORS" => Ok(Exchange::OSL),
        "STO" | "STOCKHOLM STOCK EXCHANGE" => Ok(Exchange::STO),
        "CPH" | "COPENHAGEN STOCK EXCHANGE" => Ok(Exchange::CPH),
        "WSE" | "WARSAW STOCK EXCHANGE" => Ok(Exchange::WSE),
        "PRA" | "PRAGUE STOCK EXCHANGE" | "PSE_CZ" => Ok(Exchange::PSE_CZ),
        "BUD" | "BUDAPEST STOCK EXCHANGE" | "BSE_HU" => Ok(Exchange::BSE_HU),
        "MOEX" | "MOSCOW EXCHANGE" | "MCX" => Ok(Exchange::MOEX),
        "BIST" | "ISTANBUL STOCK EXCHANGE" | "IST" => Ok(Exchange::BIST),
        "JSE" | "JOHANNESBURG STOCK EXCHANGE" | "JNB" => Ok(Exchange::JSE),
        "TASE" | "TEL AVIV STOCK EXCHANGE" | "TLV" => Ok(Exchange::TASE),
        "BSE" | "BOMBAY STOCK EXCHANGE" | "BSE INDIA" => Ok(Exchange::BSE),
        "NSE" | "NATIONAL STOCK EXCHANGE OF INDIA" | "NSI" => Ok(Exchange::NSE),
        "KRX" | "KOREA EXCHANGE" | "KSC" | "KOE" => Ok(Exchange::KRX),
        "SGX" | "SINGAPORE EXCHANGE" | "SES" => Ok(Exchange::SGX),
        "SET" | "STOCK EXCHANGE OF THAILAND" => Ok(Exchange::SET),
        "KLSE" | "BURSA MALAYSIA" | "KLS" => Ok(Exchange::KLSE),
        "PSE" | "PHILIPPINE STOCK EXCHANGE" => Ok(Exchange::PSE),
        "IDX" | "INDONESIA STOCK EXCHANGE" | "JKT" => Ok(Exchange::IDX),
        "HOSE" | "HO CHI MINH STOCK EXCHANGE" => Ok(Exchange::HOSE),
        _ => parse_exchange_token(token, s),
    }
}

/// Returns the first present Yahoo exchange candidate that can be normalized.
#[must_use]
pub fn first_parsed_yahoo_exchange<'a>(
    candidates: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<Exchange> {
    candidates
        .into_iter()
        .flatten()
        .find_map(|candidate| parse_yahoo_exchange(candidate).ok())
}

/// Parse a Yahoo quoteType token into the closest provider-agnostic asset kind.
///
/// # Errors
/// Returns `YfError` if the token is empty or cannot be represented by `paft`.
pub fn parse_yahoo_quote_type(s: &str) -> Result<AssetKind, YfError> {
    match s.trim().to_ascii_uppercase().as_str() {
        "" => Err(YfError::MissingData("quoteType missing".into())),
        "ETF" | "MUTUALFUND" | "MUTUAL_FUND" => Ok(AssetKind::Fund),
        "INDEX" => Ok(AssetKind::Index),
        "CRYPTOCURRENCY" => Ok(AssetKind::Crypto),
        "CURRENCY" => Ok(AssetKind::Forex),
        token => parse_required_token(token, "asset kind"),
    }
}

/// Infer Yahoo listing currency from a Yahoo exchange token or display name.
#[must_use]
pub fn yahoo_exchange_to_listing_currency(exchange: &str) -> Option<&'static str> {
    let parsed = parse_yahoo_exchange(exchange).ok()?;
    match parsed {
        Exchange::NASDAQ | Exchange::NYSE | Exchange::AMEX | Exchange::BATS | Exchange::OTC => {
            Some("USD")
        }
        Exchange::LSE => Some("GBp"),
        Exchange::TSE => Some("JPY"),
        Exchange::HKEX => Some("HKD"),
        Exchange::TSX => Some("CAD"),
        Exchange::ASX => Some("AUD"),
        Exchange::Euronext
        | Exchange::XETRA
        | Exchange::BIT
        | Exchange::BME
        | Exchange::AEX
        | Exchange::BRU
        | Exchange::LIS
        | Exchange::EPA => Some("EUR"),
        Exchange::SIX => Some("CHF"),
        Exchange::KRX => Some("KRW"),
        Exchange::SGX => Some("SGD"),
        Exchange::Other(code) if code.as_ref() == "NYSE_ARCA" => Some("USD"),
        _ => yahoo_exchange_code_to_listing_currency(exchange),
    }
}

fn yahoo_exchange_code_to_listing_currency(exchange: &str) -> Option<&'static str> {
    match exchange.trim().to_ascii_uppercase().as_str() {
        "NASDAQ" | "NYSE" | "NYSEARCA" | "NYSE_ARCA" => Some("USD"),
        "LONDON" => Some("GBp"),
        "TOKYO" => Some("JPY"),
        "HONG KONG" => Some("HKD"),
        "TORONTO" | "VAN" => Some("CAD"),
        "PARIS" | "MILAN" | "AMSTERDAM" | "BRUSSELS" | "MADRID" | "FRA" | "DUS" | "HAM" | "HAN"
        | "MUN" | "STU" | "BER" => Some("EUR"),
        _ => None,
    }
}

fn is_yahoo_market_scope(token: &str) -> bool {
    token.trim().to_ascii_lowercase().ends_with("_market")
}

fn parse_exchange_token(token: &str, original: &str) -> Result<Exchange, YfError> {
    Exchange::try_from_str(token)
        .map_err(|err| YfError::InvalidData(format!("invalid exchange {original:?}: {err}")))
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
