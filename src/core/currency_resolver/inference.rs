use super::{CurrencyHints, ResolvedCurrencyUnit};
use crate::core::yahoo_vocab::yahoo_exchange_to_listing_currency;

pub(super) fn infer_listing_currency(
    symbol: &str,
    hints: &CurrencyHints,
) -> Option<ResolvedCurrencyUnit> {
    let symbol_upper = symbol.trim().to_ascii_uppercase();
    let code = suffix_currency(&symbol_upper).or_else(|| {
        [
            hints.full_exchange_name.as_deref(),
            hints.exchange.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find_map(yahoo_exchange_to_listing_currency)
    })?;
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
