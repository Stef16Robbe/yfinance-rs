//! Strongly typed Yahoo screener field constants.

/// Equity screener fields supported by the typed API.
pub mod equity_fields {
    use super::super::query::{
        EnumField, Equity, EquitySector, NumericField, PercentField, Region, SortField,
        YahooExchangeCode,
    };

    /// Yahoo ticker field, used as the default sort.
    pub const TICKER: SortField<Equity> = SortField::new("ticker");
    /// Exchange code.
    pub const EXCHANGE: EnumField<Equity, YahooExchangeCode> = EnumField::new("exchange");
    /// Region code.
    pub const REGION: EnumField<Equity, Region> = EnumField::new("region");
    /// Sector.
    pub const SECTOR: EnumField<Equity, EquitySector> = EnumField::new("sector");
    /// End-of-day price.
    pub const EOD_PRICE: NumericField<Equity> = NumericField::new("eodprice");
    /// Intraday price.
    pub const INTRADAY_PRICE: NumericField<Equity> = NumericField::new("intradayprice");
    /// Intraday market capitalization.
    pub const INTRADAY_MARKET_CAP: NumericField<Equity> = NumericField::new("intradaymarketcap");
    /// Percent change, in percentage points.
    pub const PERCENT_CHANGE: PercentField<Equity> = PercentField::new("percentchange");
    /// Percent change sort field.
    pub const PERCENT_CHANGE_SORT: SortField<Equity> = SortField::new("percentchange");
    /// Day volume.
    pub const DAY_VOLUME: NumericField<Equity> = NumericField::new("dayvolume");
    /// Day volume sort field.
    pub const DAY_VOLUME_SORT: SortField<Equity> = SortField::new("dayvolume");
    /// End-of-day volume sort field.
    pub const EOD_VOLUME: SortField<Equity> = SortField::new("eodvolume");
    /// Average daily volume over 3 months.
    pub const AVERAGE_DAILY_VOLUME_3M: NumericField<Equity> = NumericField::new("avgdailyvol3m");
    /// Short percentage of shares outstanding.
    pub const SHORT_PERCENTAGE_OF_SHARES_OUTSTANDING: SortField<Equity> =
        SortField::new("short_percentage_of_shares_outstanding.value");
    /// EPS growth over the trailing twelve months.
    pub const EPS_GROWTH_LAST_TWELVE_MONTHS: NumericField<Equity> =
        NumericField::new("epsgrowth.lasttwelvemonths");
    /// Quarterly revenue growth.
    pub const QUARTERLY_REVENUE_GROWTH: NumericField<Equity> =
        NumericField::new("quarterlyrevenuegrowth.quarterly");
    /// Trailing twelve-month PE ratio.
    pub const PE_RATIO_LAST_TWELVE_MONTHS: NumericField<Equity> =
        NumericField::new("peratio.lasttwelvemonths");
    /// Five-year PEG ratio.
    pub const PEG_RATIO_5Y: NumericField<Equity> = NumericField::new("pegratio_5y");
}

/// Mutual fund screener fields supported by the typed API.
pub mod fund_fields {
    use super::super::query::{
        EnumField, Fund, FundCategory, NumericField, Rating, SortField, YahooExchangeCode,
    };

    /// Yahoo ticker field, used as the default sort.
    pub const TICKER: SortField<Fund> = SortField::new("ticker");
    /// Exchange code.
    pub const EXCHANGE: EnumField<Fund, YahooExchangeCode> = EnumField::new("exchange");
    /// Fund category.
    pub const CATEGORY_NAME: EnumField<Fund, FundCategory> = EnumField::new("categoryname");
    /// Overall performance rating.
    pub const PERFORMANCE_RATING_OVERALL: EnumField<Fund, Rating> =
        EnumField::new("performanceratingoverall");
    /// Overall risk rating.
    pub const RISK_RATING_OVERALL: EnumField<Fund, Rating> = EnumField::new("riskratingoverall");
    /// Intraday price.
    pub const INTRADAY_PRICE: NumericField<Fund> = NumericField::new("intradayprice");
    /// Initial investment.
    pub const INITIAL_INVESTMENT: NumericField<Fund> = NumericField::new("initialinvestment");
    /// One-year NAV category rank.
    pub const ANNUAL_RETURN_NAV_Y1_CATEGORY_RANK: NumericField<Fund> =
        NumericField::new("annualreturnnavy1categoryrank");
    /// Fund net assets sort field.
    pub const FUND_NET_ASSETS: SortField<Fund> = SortField::new("fundnetassets");
    /// Percent change sort field.
    pub const PERCENT_CHANGE: SortField<Fund> = SortField::new("percentchange");
}

/// ETF screener fields supported by the typed API.
pub mod etf_fields {
    use super::super::query::{
        EnumField, Etf, EtfCategory, NumericField, Rating, Region, SortField,
    };

    /// Yahoo ticker field, used as the default sort.
    pub const TICKER: SortField<Etf> = SortField::new("ticker");
    /// Region code.
    pub const REGION: EnumField<Etf, Region> = EnumField::new("region");
    /// ETF category.
    pub const CATEGORY_NAME: EnumField<Etf, EtfCategory> = EnumField::new("categoryname");
    /// Overall performance rating.
    pub const PERFORMANCE_RATING_OVERALL: EnumField<Etf, Rating> =
        EnumField::new("performanceratingoverall");
    /// Intraday price.
    pub const INTRADAY_PRICE: NumericField<Etf> = NumericField::new("intradayprice");
    /// Percent change sort field.
    pub const PERCENT_CHANGE: SortField<Etf> = SortField::new("percentchange");
    /// Annual report net expense ratio sort field.
    pub const ANNUAL_REPORT_NET_EXPENSE_RATIO: SortField<Etf> =
        SortField::new("annualreportnetexpenseratio");
}
