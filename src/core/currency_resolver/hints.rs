use super::unit::ResolvedCurrencyUnit;
use crate::core::currency::currency_for_country;

#[derive(Clone, Debug, Default)]
pub(super) enum Hint<T> {
    #[default]
    Unknown,
    Missing,
    Invalid(String),
    Present(T),
}

impl<T> Hint<T> {
    pub(super) const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub(super) const fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    pub(super) const fn present(&self) -> Option<&T> {
        match self {
            Self::Present(value) => Some(value),
            Self::Unknown | Self::Missing | Self::Invalid(_) => None,
        }
    }

    pub(super) fn invalid_code(&self) -> Option<&str> {
        match self {
            Self::Invalid(code) => Some(code),
            Self::Unknown | Self::Missing | Self::Present(_) => None,
        }
    }
}

impl Hint<ResolvedCurrencyUnit> {
    fn set_from_code(&mut self, code: Option<&str>) {
        let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
            *self = Self::Missing;
            return;
        };

        *self = ResolvedCurrencyUnit::from_code(code)
            .map_or_else(|| Self::Invalid(code.to_string()), Self::Present);
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CurrencyHintField {
    Quote,
    Financial,
    QuoteSummaryFinancial,
    ProfileCountry,
}

impl CurrencyHintField {
    const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::Quote => 0,
            Self::Financial => 1,
            Self::QuoteSummaryFinancial => 2,
            Self::ProfileCountry => 3,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CurrencyHints {
    currencies: [Hint<ResolvedCurrencyUnit>; CurrencyHintField::COUNT],
    pub(super) exchange: Option<String>,
    pub(super) full_exchange_name: Option<String>,
    pub(super) quote_type: Option<String>,
    pub(super) country: Option<String>,
}

impl CurrencyHints {
    pub fn from_quote(
        quote_currency: Option<&str>,
        financial_currency: Option<&str>,
        exchange: Option<&str>,
        full_exchange_name: Option<&str>,
        quote_type: Option<&str>,
    ) -> Self {
        let mut hints = Self::default();
        hints.set_currency(CurrencyHintField::Quote, quote_currency);
        hints.set_currency(CurrencyHintField::Financial, financial_currency);
        hints.exchange = nonempty_owned(exchange);
        hints.full_exchange_name = nonempty_owned(full_exchange_name);
        hints.quote_type = nonempty_owned(quote_type);
        hints
    }

    pub fn from_chart(
        quote_currency: Option<&str>,
        exchange: Option<&str>,
        full_exchange_name: Option<&str>,
        quote_type: Option<&str>,
    ) -> Self {
        let mut hints = Self::default();
        hints.set_currency(CurrencyHintField::Quote, quote_currency);
        hints.exchange = nonempty_owned(exchange);
        hints.full_exchange_name = nonempty_owned(full_exchange_name);
        hints.quote_type = nonempty_owned(quote_type);
        hints
    }

    pub fn from_options_quote(
        quote_currency: Option<&str>,
        exchange: Option<&str>,
        full_exchange_name: Option<&str>,
        quote_type: Option<&str>,
    ) -> Self {
        let mut hints = Self::default();
        if quote_currency
            .map(str::trim)
            .is_some_and(|code| !code.is_empty())
        {
            hints.set_currency(CurrencyHintField::Quote, quote_currency);
        }
        hints.exchange = nonempty_owned(exchange);
        hints.full_exchange_name = nonempty_owned(full_exchange_name);
        hints.quote_type = nonempty_owned(quote_type);
        hints
    }

    pub fn from_profile(
        country: Option<&str>,
        exchange: Option<&str>,
        quote_type: Option<&str>,
    ) -> Self {
        let mut hints = Self {
            country: nonempty_owned(country),
            exchange: nonempty_owned(exchange),
            quote_type: nonempty_owned(quote_type),
            ..Self::default()
        };
        *hints.hint_mut(CurrencyHintField::ProfileCountry) = country
            .and_then(currency_for_country)
            .map(ResolvedCurrencyUnit::from_currency)
            .map_or(Hint::Missing, Hint::Present);
        hints
    }

    pub fn from_quote_summary_financial(financial_currency: Option<&str>) -> Self {
        let mut hints = Self::default();
        hints.set_currency(CurrencyHintField::QuoteSummaryFinancial, financial_currency);
        hints
    }

    pub(super) const fn hint(&self, field: CurrencyHintField) -> &Hint<ResolvedCurrencyUnit> {
        &self.currencies[field.index()]
    }

    const fn hint_mut(&mut self, field: CurrencyHintField) -> &mut Hint<ResolvedCurrencyUnit> {
        &mut self.currencies[field.index()]
    }

    fn set_currency(&mut self, field: CurrencyHintField, code: Option<&str>) {
        self.hint_mut(field).set_from_code(code);
    }

    pub(super) const fn unit(&self, field: CurrencyHintField) -> Option<&ResolvedCurrencyUnit> {
        self.hint(field).present()
    }

    pub(super) fn invalid_code(&self, field: CurrencyHintField) -> Option<&str> {
        self.hint(field).invalid_code()
    }

    pub(super) const fn is_unknown(&self, field: CurrencyHintField) -> bool {
        self.hint(field).is_unknown()
    }

    pub(super) const fn is_missing(&self, field: CurrencyHintField) -> bool {
        self.hint(field).is_missing()
    }

    pub(super) fn merge(&mut self, other: Self) {
        for (slot, incoming) in self.currencies.iter_mut().zip(other.currencies) {
            merge_hint(slot, incoming);
        }
        merge_option(&mut self.exchange, other.exchange);
        merge_option(&mut self.full_exchange_name, other.full_exchange_name);
        merge_option(&mut self.quote_type, other.quote_type);
        merge_option(&mut self.country, other.country);
    }
}

fn nonempty_owned(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn merge_option(slot: &mut Option<String>, incoming: Option<String>) {
    if let Some(value) = incoming {
        *slot = Some(value);
    }
}

fn merge_hint<T>(slot: &mut Hint<T>, incoming: Hint<T>) {
    match incoming {
        Hint::Unknown => {}
        Hint::Missing => {
            if slot.is_unknown() {
                *slot = Hint::Missing;
            }
        }
        Hint::Invalid(value) => {
            if !matches!(slot, Hint::Present(_)) {
                *slot = Hint::Invalid(value);
            }
        }
        Hint::Present(value) => *slot = Hint::Present(value),
    }
}
