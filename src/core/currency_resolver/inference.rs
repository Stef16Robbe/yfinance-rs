use super::{CurrencyHints, ResolvedCurrencyUnit};

pub(super) fn infer_listing_currency(
    symbol: &str,
    hints: &CurrencyHints,
) -> Option<ResolvedCurrencyUnit> {
    let symbol_upper = symbol.trim().to_ascii_uppercase();
    let exchange = hints
        .full_exchange_name
        .as_deref()
        .or(hints.exchange.as_deref())
        .unwrap_or("")
        .to_ascii_uppercase();

    let code = suffix_currency(&symbol_upper).or_else(|| exchange_currency(&exchange))?;
    ResolvedCurrencyUnit::from_code(code)
}

fn suffix_currency(symbol: &str) -> Option<&'static str> {
    [
        (".DE", "EUR"),
        (".F", "EUR"),
        (".PA", "EUR"),
        (".MI", "EUR"),
        (".AS", "EUR"),
        (".BR", "EUR"),
        (".MC", "EUR"),
        (".SW", "CHF"),
        (".L", "GBp"),
        (".TO", "CAD"),
        (".V", "CAD"),
        (".T", "JPY"),
        (".HK", "HKD"),
        (".SS", "CNY"),
        (".SZ", "CNY"),
        (".AX", "AUD"),
        (".KS", "KRW"),
        (".KQ", "KRW"),
        (".SI", "SGD"),
    ]
    .into_iter()
    .find_map(|(suffix, currency)| symbol.ends_with(suffix).then_some(currency))
}

fn exchange_currency(exchange: &str) -> Option<&'static str> {
    if exchange.is_empty() {
        return None;
    }

    if [
        "NMS", "NGM", "NCM", "NYQ", "ASE", "PCX", "NASDAQ", "NYSE", "NYSEARCA",
    ]
    .iter()
    .any(|needle| exchange.contains(needle))
    {
        return Some("USD");
    }
    if [
        "GER",
        "XETRA",
        "PARIS",
        "MILAN",
        "AMSTERDAM",
        "BRUSSELS",
        "MADRID",
    ]
    .iter()
    .any(|needle| exchange.contains(needle))
    {
        return Some("EUR");
    }
    if exchange.contains("LSE") || exchange.contains("LONDON") {
        return Some("GBp");
    }
    if exchange.contains("TORONTO") || exchange == "TOR" || exchange == "VAN" {
        return Some("CAD");
    }
    if exchange.contains("TOKYO") || exchange == "JPX" || exchange == "TYO" {
        return Some("JPY");
    }
    if exchange.contains("HONG KONG") || exchange == "HKG" {
        return Some("HKD");
    }

    None
}
