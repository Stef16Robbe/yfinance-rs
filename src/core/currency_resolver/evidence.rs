#[derive(Clone, Copy, Debug)]
pub enum TradingCurrencyEvidence<'a> {
    None,
    ChartMeta(Option<&'a str>),
    OptionsQuote(Option<&'a str>),
}

impl<'a> TradingCurrencyEvidence<'a> {
    pub(super) const fn direct_code(self) -> Option<&'a str> {
        match self {
            Self::None => None,
            Self::ChartMeta(code) | Self::OptionsQuote(code) => code,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ChartMeta(_) => "chart.meta.currency",
            Self::OptionsQuote(_) => "options quote currency",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ReportingCurrencyEvidence<'a> {
    FinancialCurrency(Option<&'a str>),
    TimeseriesCurrencyCode(Option<&'a str>),
}

impl<'a> ReportingCurrencyEvidence<'a> {
    pub(super) const fn direct_code(self) -> Option<&'a str> {
        match self {
            Self::FinancialCurrency(code) | Self::TimeseriesCurrencyCode(code) => code,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::FinancialCurrency(_) => "financialCurrency",
            Self::TimeseriesCurrencyCode(_) => "timeseries currencyCode",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CorporateActionCurrencyEvidence<'a> {
    ChartMeta(Option<&'a str>),
}

impl<'a> CorporateActionCurrencyEvidence<'a> {
    pub(super) const fn direct_code(self) -> Option<&'a str> {
        match self {
            Self::ChartMeta(code) => code,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::ChartMeta(_) => "chart.meta.currency",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum AnalystEstimateCurrencyEvidence<'a> {
    Earnings(Option<&'a str>),
    Revenue(Option<&'a str>),
    EpsTrend(Option<&'a str>),
}

impl<'a> AnalystEstimateCurrencyEvidence<'a> {
    pub(super) const fn direct_code(self) -> Option<&'a str> {
        match self {
            Self::Earnings(code) | Self::Revenue(code) | Self::EpsTrend(code) => code,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Earnings(_) => "earningsCurrency",
            Self::Revenue(_) => "revenueCurrency",
            Self::EpsTrend(_) => "epsTrendCurrency",
        }
    }
}
