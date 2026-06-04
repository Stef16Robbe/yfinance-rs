use crate::core::{YfError, conversions::decimal_from_f64};
use paft::money::{Currency, Money, Price, PriceAmount};
use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCurrencyUnit {
    currency: Currency,
    scale: Decimal,
}

impl ResolvedCurrencyUnit {
    pub const fn from_currency(currency: Currency) -> Self {
        Self {
            currency,
            scale: Decimal::ONE,
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let trimmed = code.trim();
        if trimmed.is_empty() {
            return None;
        }

        let (code, scale) = match trimmed {
            "GBp" | "GBX" => ("GBP", Decimal::new(1, 2)),
            "ZAc" => ("ZAR", Decimal::new(1, 2)),
            "ILA" => ("ILS", Decimal::new(1, 2)),
            _ => (trimmed, Decimal::ONE),
        };

        Currency::from_str(code)
            .ok()
            .map(|currency| Self { currency, scale })
    }

    pub fn major_from_code(code: &str) -> Option<Self> {
        Self::from_code(code).map(|unit| unit.major_unit())
    }

    pub fn major_unit(&self) -> Self {
        Self::from_currency(self.currency.clone())
    }

    pub const fn currency(&self) -> &Currency {
        &self.currency
    }

    pub fn price_amount_from_f64(&self, value: f64) -> Option<PriceAmount> {
        self.scaled_decimal_from_f64(value).map(PriceAmount::new)
    }

    pub fn price_from_f64(&self, value: f64) -> Option<Price> {
        self.scaled_decimal_from_f64(value)
            .map(|decimal| Price::new(decimal, self.currency.clone()))
    }

    pub fn money_from_f64(&self, value: f64) -> Option<Money> {
        decimal_from_f64(value).and_then(|decimal| Money::new(decimal, self.currency.clone()).ok())
    }

    pub fn money_from_i64(&self, value: i64) -> Result<Money, YfError> {
        let decimal = Decimal::from_i128_with_scale(i128::from(value), 0);
        self.money_from_decimal(decimal)
    }

    pub fn money_from_u64(&self, value: u64) -> Result<Money, YfError> {
        let decimal = Decimal::from_i128_with_scale(i128::from(value), 0);
        self.money_from_decimal(decimal)
    }

    pub fn money_from_decimal(&self, decimal: Decimal) -> Result<Money, YfError> {
        Ok(Money::new(decimal, self.currency.clone())?)
    }

    fn scaled_decimal_from_f64(&self, value: f64) -> Option<Decimal> {
        decimal_from_f64(value).and_then(|decimal| decimal.checked_mul(self.scale))
    }
}
