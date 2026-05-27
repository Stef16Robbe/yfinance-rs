use paft::money::{Currency, IsoCurrency};
use yfinance_rs::core::conversions::{decimal_from_f64, money_from_f64, price_from_f64};

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
