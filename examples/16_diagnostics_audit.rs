//! Live diagnostics audit across the public Yahoo-calling surfaces.
//!
//! This example is intentionally more operational than the smaller examples in
//! this directory. It shows the pattern for inspecting `YfResponse` diagnostics
//! and performs lightweight sanity checks on the parsed data. It is useful when
//! validating that the crate is still aligned with Yahoo's current wire shape.
//!
//! Run it manually:
//!
//! ```sh
//! cargo run --example 16_diagnostics_audit
//! ```

#![allow(clippy::too_many_lines)]

use std::{fmt::Display, future::Future, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use paft::Decimal;
use paft::aggregates::Snapshot;
use paft::fundamentals::analysis::{
    Earnings, EarningsTrendRow, PriceTarget, RecommendationRow, RecommendationSummary,
    UpgradeDowngradeRow,
};
use paft::fundamentals::holders::{
    InsiderRosterHolder, InsiderTransaction, InstitutionalHolder, MajorHolder,
    NetSharePurchaseActivity,
};
use paft::fundamentals::profile::ShareCount;
use paft::fundamentals::statements::{BalanceSheetRow, Calendar, CashflowRow, IncomeStatementRow};
use paft::market::news::NewsArticle;
use paft::market::options::OptionChain;
use paft::market::quote::QuoteUpdate;
use paft::market::responses::download::DownloadResponse;
use paft::market::responses::history::{Candle, HistoryResponse};
use paft::market::responses::search::SearchResponse;
use tokio::time::{Instant, timeout};
use yfinance_rs::analysis::AnalysisBuilder;
use yfinance_rs::core::{HistoryRequest, HistoryService};
use yfinance_rs::profile::{self, Profile};
use yfinance_rs::{
    Action, CacheMode, DownloadBuilder, DownloadConcurrency, EquityQuery, EsgBuilder, EtfCategory,
    EtfQuery, FundCategory, FundQuery, FundamentalsBuilder, HistoryBuilder, HoldersBuilder, Info,
    Interval, KeyStatistics, NewsBuilder, NewsTab, PredefinedScreener, QuotesBuilder, Range,
    Rating, Region, ResultOffset, ScreenerBuilder, ScreenerCount, ScreenerResponse, SearchBuilder,
    SortDirection, StreamBuilder, StreamMethod, Ticker, YfClient, YfError, YfResponse, YfWarning,
    equity_fields, etf_fields, fund_fields, quotes, quotes_with_diagnostics, screen,
    screen_with_diagnostics, search,
};

const PRIMARY: &str = "AAPL";
const SECONDARY: &str = "MSFT";
const TERTIARY: &str = "META";
const FUND: &str = "VFINX";
const CRYPTO: &str = "BTC-USD";

#[tokio::main]
async fn main() -> Result<(), YfError> {
    let client = YfClient::default();
    let mut audit = Audit::default();

    println!("Live diagnostics audit started at {}", Utc::now());
    println!("Primary symbols: {PRIMARY}, {SECONDARY}, {TERTIARY}, {FUND}, {CRYPTO}");
    println!("A 429 response is retried once after five minutes.\n");

    audit_quote_surfaces(&client, &mut audit).await;
    audit_history_and_download_surfaces(&client, &mut audit).await;
    audit_ticker_diagnostic_surfaces(&client, &mut audit).await;
    audit_analysis_surfaces(&client, &mut audit).await;
    audit_fundamentals_surfaces(&client, &mut audit).await;
    audit_holders_surfaces(&client, &mut audit).await;
    audit_news_search_and_screener_surfaces(&client, &mut audit).await;
    audit_esg_surfaces(&client, &mut audit).await;
    audit_public_surfaces_without_diagnostics(&client, &mut audit).await;

    audit.print_report();
    Ok(())
}

async fn audit_quote_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_response(
        audit,
        "quote::quotes_with_diagnostics(AAPL,MSFT,META)",
        || quotes_with_diagnostics(client, [PRIMARY, SECONDARY, TERTIARY]),
        coerce_slice(summary_quotes),
    )
    .await;

    audit_response(
        audit,
        "QuotesBuilder::fetch_with_diagnostics(AAPL,MSFT)",
        || async {
            QuotesBuilder::new(client)
                .symbols([PRIMARY, SECONDARY])
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        coerce_slice(summary_quotes),
    )
    .await;
}

async fn audit_history_and_download_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_response(
        audit,
        "HistoryBuilder::fetch_with_diagnostics(AAPL,1mo,1d)",
        || async {
            HistoryBuilder::new(client, PRIMARY)
                .range(Range::M1)
                .interval(Interval::D1)
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        coerce_slice(summary_candles),
    )
    .await;

    audit_response(
        audit,
        "HistoryBuilder::fetch_full_with_diagnostics(AAPL,6mo,1d)",
        || async {
            HistoryBuilder::new(client, PRIMARY)
                .range(Range::M6)
                .interval(Interval::D1)
                .actions(true)
                .cache_mode(CacheMode::Use)
                .fetch_full_with_diagnostics()
                .await
        },
        summary_history,
    )
    .await;

    audit_response(
        audit,
        "DownloadBuilder::run_with_diagnostics(AAPL,MSFT,META)",
        || async {
            DownloadBuilder::new(client)
                .symbols([PRIMARY, SECONDARY, TERTIARY])
                .range(Range::M1)
                .interval(Interval::D1)
                .concurrency(DownloadConcurrency::new(2)?)
                .cache_mode(CacheMode::Use)
                .run_with_diagnostics()
                .await
        },
        summary_download,
    )
    .await;
}

async fn audit_ticker_diagnostic_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_response(
        audit,
        "Ticker::info_with_diagnostics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.info_with_diagnostics().await
        },
        summary_info,
    )
    .await;

    audit_response(
        audit,
        "Ticker::quote_with_diagnostics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quote_with_diagnostics().await
        },
        summary_quote,
    )
    .await;

    audit_response(
        audit,
        "Ticker::fast_info_with_diagnostics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.fast_info_with_diagnostics().await
        },
        |fast| summary_snapshot("fast_info.snapshot", &fast.snapshot),
    )
    .await;

    audit_response(
        audit,
        "Ticker::key_statistics_with_diagnostics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.key_statistics_with_diagnostics().await
        },
        summary_key_statistics,
    )
    .await;

    audit_response(
        audit,
        "Ticker::option_chain_with_diagnostics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.option_chain_with_diagnostics(None).await
        },
        summary_option_chain,
    )
    .await;
}

async fn audit_analysis_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_response(
        audit,
        "AnalysisBuilder::recommendations_with_diagnostics(AAPL)",
        || async {
            AnalysisBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .recommendations_with_diagnostics()
                .await
        },
        coerce_slice(summary_recommendations),
    )
    .await;

    audit_response(
        audit,
        "AnalysisBuilder::recommendations_summary_with_diagnostics(AAPL)",
        || async {
            AnalysisBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .recommendations_summary_with_diagnostics()
                .await
        },
        summary_recommendation_summary,
    )
    .await;

    audit_response(
        audit,
        "AnalysisBuilder::upgrades_downgrades_with_diagnostics(AAPL)",
        || async {
            AnalysisBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .upgrades_downgrades_with_diagnostics()
                .await
        },
        coerce_slice(summary_upgrades),
    )
    .await;

    audit_response(
        audit,
        "AnalysisBuilder::analyst_price_target_with_diagnostics(AAPL)",
        || async {
            AnalysisBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .analyst_price_target_with_diagnostics(None)
                .await
        },
        summary_price_target,
    )
    .await;

    audit_response(
        audit,
        "AnalysisBuilder::earnings_trend_with_diagnostics(AAPL)",
        || async {
            AnalysisBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .earnings_trend_with_diagnostics(None)
                .await
        },
        coerce_slice(summary_earnings_trend),
    )
    .await;
}

async fn audit_fundamentals_surfaces(client: &YfClient, audit: &mut Audit) {
    for quarterly in [false, true] {
        let label = if quarterly { "quarterly" } else { "annual" };
        audit_response(
            audit,
            &format!("FundamentalsBuilder::income_statement_with_diagnostics(AAPL,{label})"),
            || async move {
                FundamentalsBuilder::new(client, PRIMARY)
                    .cache_mode(CacheMode::Use)
                    .income_statement_with_diagnostics(quarterly, None)
                    .await
            },
            coerce_slice(summary_income_statement),
        )
        .await;

        audit_response(
            audit,
            &format!("FundamentalsBuilder::balance_sheet_with_diagnostics(AAPL,{label})"),
            || async move {
                FundamentalsBuilder::new(client, PRIMARY)
                    .cache_mode(CacheMode::Use)
                    .balance_sheet_with_diagnostics(quarterly, None)
                    .await
            },
            coerce_slice(summary_balance_sheet),
        )
        .await;

        audit_response(
            audit,
            &format!("FundamentalsBuilder::cashflow_with_diagnostics(AAPL,{label})"),
            || async move {
                FundamentalsBuilder::new(client, PRIMARY)
                    .cache_mode(CacheMode::Use)
                    .cashflow_with_diagnostics(quarterly, None)
                    .await
            },
            coerce_slice(summary_cashflow),
        )
        .await;

        audit_response(
            audit,
            &format!("FundamentalsBuilder::shares_with_diagnostics(AAPL,{label})"),
            || async move {
                FundamentalsBuilder::new(client, PRIMARY)
                    .cache_mode(CacheMode::Use)
                    .shares_with_diagnostics(quarterly)
                    .await
            },
            coerce_slice(summary_shares),
        )
        .await;

        let end = Utc::now();
        let start = end - ChronoDuration::days(730);
        audit_response(
            audit,
            &format!("FundamentalsBuilder::shares_between_with_diagnostics(AAPL,{label})"),
            || async move {
                FundamentalsBuilder::new(client, PRIMARY)
                    .cache_mode(CacheMode::Use)
                    .shares_between_with_diagnostics(quarterly, start, end)
                    .await
            },
            coerce_slice(summary_shares),
        )
        .await;
    }

    audit_response(
        audit,
        "FundamentalsBuilder::earnings_with_diagnostics(AAPL)",
        || async {
            FundamentalsBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .earnings_with_diagnostics(None)
                .await
        },
        summary_earnings,
    )
    .await;

    audit_response(
        audit,
        "FundamentalsBuilder::calendar_with_diagnostics(AAPL)",
        || async {
            FundamentalsBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .calendar_with_diagnostics()
                .await
        },
        summary_calendar,
    )
    .await;
}

async fn audit_holders_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_response(
        audit,
        "HoldersBuilder::major_holders_with_diagnostics(AAPL)",
        || async {
            HoldersBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .major_holders_with_diagnostics()
                .await
        },
        coerce_slice(summary_major_holders),
    )
    .await;

    audit_response(
        audit,
        "HoldersBuilder::institutional_holders_with_diagnostics(AAPL)",
        || async {
            HoldersBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .institutional_holders_with_diagnostics()
                .await
        },
        coerce_slice(summary_institutional_holders),
    )
    .await;

    audit_response(
        audit,
        "HoldersBuilder::mutual_fund_holders_with_diagnostics(AAPL)",
        || async {
            HoldersBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .mutual_fund_holders_with_diagnostics()
                .await
        },
        coerce_slice(summary_institutional_holders),
    )
    .await;

    audit_response(
        audit,
        "HoldersBuilder::insider_transactions_with_diagnostics(AAPL)",
        || async {
            HoldersBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .insider_transactions_with_diagnostics()
                .await
        },
        coerce_slice(summary_insider_transactions),
    )
    .await;

    audit_response(
        audit,
        "HoldersBuilder::insider_roster_holders_with_diagnostics(AAPL)",
        || async {
            HoldersBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .insider_roster_holders_with_diagnostics()
                .await
        },
        coerce_slice(summary_insider_roster),
    )
    .await;

    audit_response(
        audit,
        "HoldersBuilder::net_share_purchase_activity_with_diagnostics(AAPL)",
        || async {
            HoldersBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .net_share_purchase_activity_with_diagnostics()
                .await
        },
        coerce_option(summary_net_share_purchase_activity),
    )
    .await;
}

async fn audit_news_search_and_screener_surfaces(client: &YfClient, audit: &mut Audit) {
    for tab in [NewsTab::News, NewsTab::All, NewsTab::PressReleases] {
        audit_response(
            audit,
            &format!("NewsBuilder::fetch_with_diagnostics(AAPL,{tab:?})"),
            || async move {
                NewsBuilder::new(client, PRIMARY)
                    .tab(tab)
                    .count(5)
                    .cache_mode(CacheMode::Use)
                    .fetch_with_diagnostics()
                    .await
            },
            move |articles| summary_news(articles, tab == NewsTab::PressReleases),
        )
        .await;
    }

    audit_response(
        audit,
        "SearchBuilder::fetch_with_diagnostics(\"Apple\")",
        || async {
            SearchBuilder::new(client, "Apple")
                .quotes_count(10)
                .news_count(0)
                .lists_count(0)
                .region("US")
                .lang("en-US")
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        summary_search,
    )
    .await;

    audit_response(
        audit,
        "screen_with_diagnostics(DayGainers)",
        || screen_with_diagnostics(client, PredefinedScreener::DayGainers),
        summary_screener,
    )
    .await;

    audit_response(
        audit,
        "ScreenerBuilder::predefined(MostActives).fetch_with_diagnostics()",
        || async {
            ScreenerBuilder::predefined(client, PredefinedScreener::MostActives)
                .count(ScreenerCount::new(5)?)
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        summary_screener,
    )
    .await;

    audit_response(
        audit,
        "ScreenerBuilder::equity(...).fetch_with_diagnostics()",
        || async {
            let exchange_filter =
                equity_fields::EXCHANGE.one_of([yfinance_rs::YahooExchangeCode::Nms])?;
            let query = EquityQuery::and(vec![
                equity_fields::REGION.eq(Region::Us),
                exchange_filter,
                equity_fields::INTRADAY_PRICE.gte(5),
                equity_fields::INTRADAY_MARKET_CAP.gte(1_000_000_000_u64),
            ])?;
            ScreenerBuilder::equity(client, query)
                .count(ScreenerCount::new(5)?)
                .offset(ResultOffset::new(0))
                .sort_by(equity_fields::DAY_VOLUME_SORT, SortDirection::Desc)
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        summary_screener,
    )
    .await;

    audit_response(
        audit,
        "ScreenerBuilder::etf(...).fetch_with_diagnostics()",
        || async {
            let query = EtfQuery::and(vec![
                etf_fields::REGION.eq(Region::Us),
                etf_fields::CATEGORY_NAME.eq(EtfCategory::Technology),
                etf_fields::INTRADAY_PRICE.gt(10),
            ])?;
            ScreenerBuilder::etf(client, query)
                .count(ScreenerCount::new(5)?)
                .sort_by(etf_fields::PERCENT_CHANGE, SortDirection::Desc)
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        summary_screener,
    )
    .await;

    audit_response(
        audit,
        "ScreenerBuilder::fund(...).fetch_with_diagnostics()",
        || async {
            let query = FundQuery::and(vec![
                fund_fields::CATEGORY_NAME.eq(FundCategory::LargeGrowth),
                fund_fields::PERFORMANCE_RATING_OVERALL.one_of([Rating::Four, Rating::Five])?,
                fund_fields::INITIAL_INVESTMENT.lt(100_001),
            ])?;
            ScreenerBuilder::fund(client, query)
                .count(ScreenerCount::new(5)?)
                .sort_by(fund_fields::FUND_NET_ASSETS, SortDirection::Desc)
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
        summary_screener,
    )
    .await;
}

async fn audit_esg_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_expected_maybe_dead_response(
        audit,
        "EsgBuilder::fetch_with_diagnostics(AAPL)",
        || async {
            EsgBuilder::new(client, PRIMARY)
                .cache_mode(CacheMode::Use)
                .fetch_with_diagnostics()
                .await
        },
    )
    .await;

    audit_expected_maybe_dead_response(
        audit,
        "Ticker::sustainability_with_diagnostics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.sustainability_with_diagnostics().await
        },
    )
    .await;
}

async fn audit_public_surfaces_without_diagnostics(client: &YfClient, audit: &mut Audit) {
    println!("\n--- Public surfaces without their own diagnostics return type ---");
    println!("These are still called so the audit covers the public surface area.");
    println!("The corresponding diagnostic builders above are the data-quality checks.\n");

    audit_plain(
        audit,
        "quote::quotes(AAPL,MSFT)",
        || quotes(client, [PRIMARY, SECONDARY]),
        coerce_slice(summary_quotes),
    )
    .await;

    audit_plain(
        audit,
        "QuotesBuilder::fetch(AAPL,MSFT)",
        || async {
            QuotesBuilder::new(client)
                .symbols([PRIMARY, SECONDARY])
                .cache_mode(CacheMode::Use)
                .fetch()
                .await
        },
        coerce_slice(summary_quotes),
    )
    .await;

    audit_plain(
        audit,
        "HistoryBuilder::fetch(AAPL,1mo,1d)",
        || async {
            HistoryBuilder::new(client, PRIMARY)
                .range(Range::M1)
                .interval(Interval::D1)
                .cache_mode(CacheMode::Use)
                .fetch()
                .await
        },
        coerce_slice(summary_candles),
    )
    .await;

    audit_plain(
        audit,
        "HistoryBuilder::fetch_full(AAPL,6mo,1d)",
        || async {
            HistoryBuilder::new(client, PRIMARY)
                .range(Range::M6)
                .interval(Interval::D1)
                .actions(true)
                .cache_mode(CacheMode::Use)
                .fetch_full()
                .await
        },
        summary_history,
    )
    .await;

    audit_plain(
        audit,
        "HistoryService::fetch_full_history(YfClient,AAPL)",
        || async {
            let request = HistoryRequest {
                range: Some(Range::M1),
                period: None,
                interval: Interval::D1,
                include_prepost: false,
                include_actions: true,
                auto_adjust: true,
            };
            client.fetch_full_history(PRIMARY, request).await
        },
        summary_history,
    )
    .await;

    audit_plain(
        audit,
        "DownloadBuilder::run(AAPL,MSFT)",
        || async {
            DownloadBuilder::new(client)
                .symbols([PRIMARY, SECONDARY])
                .range(Range::M1)
                .interval(Interval::D1)
                .concurrency(DownloadConcurrency::new(2)?)
                .cache_mode(CacheMode::Use)
                .run()
                .await
        },
        summary_download,
    )
    .await;

    audit_plain(
        audit,
        "profile::load_profile(AAPL)",
        || profile::load_profile(client, PRIMARY),
        summary_profile,
    )
    .await;

    audit_plain(
        audit,
        "profile::load_profile(VFINX)",
        || profile::load_profile(client, FUND),
        summary_profile,
    )
    .await;

    audit_plain(
        audit,
        "search::search(\"Apple\")",
        || search(client, "Apple"),
        summary_search,
    )
    .await;

    audit_plain(
        audit,
        "SearchBuilder::fetch(\"Apple\")",
        || async {
            SearchBuilder::new(client, "Apple")
                .quotes_count(10)
                .region("US")
                .lang("en-US")
                .cache_mode(CacheMode::Use)
                .fetch()
                .await
        },
        summary_search,
    )
    .await;

    audit_plain(
        audit,
        "screen(DayGainers)",
        || screen(client, PredefinedScreener::DayGainers),
        summary_screener,
    )
    .await;

    audit_plain(
        audit,
        "ScreenerBuilder::predefined(TopEtfsUs).fetch()",
        || async {
            ScreenerBuilder::predefined(client, PredefinedScreener::TopEtfsUs)
                .count(ScreenerCount::new(5)?)
                .cache_mode(CacheMode::Use)
                .fetch()
                .await
        },
        summary_screener,
    )
    .await;

    audit_plain(
        audit,
        "NewsBuilder::fetch(AAPL,News)",
        || async {
            NewsBuilder::new(client, PRIMARY)
                .tab(NewsTab::News)
                .count(5)
                .cache_mode(CacheMode::Use)
                .fetch()
                .await
        },
        |articles| summary_news(articles, false),
    )
    .await;

    audit_ticker_convenience_surfaces(client, audit).await;
    audit_stream_surfaces(client, audit).await;
}

async fn audit_ticker_convenience_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_ticker_market_convenience_surfaces(client, audit).await;
    audit_ticker_holder_analysis_convenience_surfaces(client, audit).await;
    audit_ticker_fundamental_convenience_surfaces(client, audit).await;
}

async fn audit_ticker_market_convenience_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_plain(
        audit,
        "Ticker::info(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.info().await
        },
        summary_info,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::info_strict(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.info_strict().await
        },
        summary_info,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::quote(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quote().await
        },
        summary_quote,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::fast_info(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.fast_info().await
        },
        |fast| summary_snapshot("fast_info.snapshot", &fast.snapshot),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::key_statistics(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.key_statistics().await
        },
        summary_key_statistics,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::news(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.news().await
        },
        |articles| summary_news(articles, false),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::history(AAPL,1mo,1d)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker
                .history(Some(Range::M1), Some(Interval::D1), false)
                .await
        },
        coerce_slice(summary_candles),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::actions(AAPL,max)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.actions(None).await
        },
        coerce_slice(summary_actions),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::actions(VFINX,max)",
        || async {
            let ticker = Ticker::new(client, FUND).cache_mode(CacheMode::Use);
            ticker.actions(None).await
        },
        coerce_slice(summary_actions),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::get_history_metadata(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.get_history_metadata(Some(Range::M1)).await
        },
        coerce_option(summary_history_meta),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::isin(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.isin().await
        },
        coerce_option(summary_isin),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::options(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.options().await
        },
        coerce_slice(summary_expirations),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::option_chain(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.option_chain(None).await
        },
        summary_option_chain,
    )
    .await;
}

async fn audit_ticker_holder_analysis_convenience_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_plain(
        audit,
        "Ticker::major_holders(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.major_holders().await
        },
        coerce_slice(summary_major_holders),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::institutional_holders(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.institutional_holders().await
        },
        coerce_slice(summary_institutional_holders),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::mutual_fund_holders(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.mutual_fund_holders().await
        },
        coerce_slice(summary_institutional_holders),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::insider_transactions(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.insider_transactions().await
        },
        coerce_slice(summary_insider_transactions),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::insider_roster_holders(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.insider_roster_holders().await
        },
        coerce_slice(summary_insider_roster),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::net_share_purchase_activity(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.net_share_purchase_activity().await
        },
        coerce_option(summary_net_share_purchase_activity),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::recommendations(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.recommendations().await
        },
        coerce_slice(summary_recommendations),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::recommendations_summary(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.recommendations_summary().await
        },
        summary_recommendation_summary,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::upgrades_downgrades(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.upgrades_downgrades().await
        },
        coerce_slice(summary_upgrades),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::analyst_price_target(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.analyst_price_target(None).await
        },
        summary_price_target,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::earnings_trend(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.earnings_trend(None).await
        },
        coerce_slice(summary_earnings_trend),
    )
    .await;

    audit_expected_maybe_dead_plain(audit, "Ticker::sustainability(AAPL)", || async {
        let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
        ticker.sustainability().await
    })
    .await;
}

async fn audit_ticker_fundamental_convenience_surfaces(client: &YfClient, audit: &mut Audit) {
    audit_plain(
        audit,
        "Ticker::income_stmt(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.income_stmt(None).await
        },
        coerce_slice(summary_income_statement),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::quarterly_income_stmt(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quarterly_income_stmt(None).await
        },
        coerce_slice(summary_income_statement),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::balance_sheet(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.balance_sheet(None).await
        },
        coerce_slice(summary_balance_sheet),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::quarterly_balance_sheet(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quarterly_balance_sheet(None).await
        },
        coerce_slice(summary_balance_sheet),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::cashflow(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.cashflow(None).await
        },
        coerce_slice(summary_cashflow),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::quarterly_cashflow(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quarterly_cashflow(None).await
        },
        coerce_slice(summary_cashflow),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::earnings(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.earnings(None).await
        },
        summary_earnings,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::calendar(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.calendar().await
        },
        summary_calendar,
    )
    .await;

    audit_plain(
        audit,
        "Ticker::shares(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.shares().await
        },
        coerce_slice(summary_shares),
    )
    .await;

    let end = Utc::now();
    let start = end - ChronoDuration::days(730);
    audit_plain(
        audit,
        "Ticker::shares_between(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.shares_between(start, end).await
        },
        coerce_slice(summary_shares),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::quarterly_shares(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quarterly_shares().await
        },
        coerce_slice(summary_shares),
    )
    .await;

    audit_plain(
        audit,
        "Ticker::quarterly_shares_between(AAPL)",
        || async {
            let ticker = Ticker::new(client, PRIMARY).cache_mode(CacheMode::Use);
            ticker.quarterly_shares_between(start, end).await
        },
        coerce_slice(summary_shares),
    )
    .await;
}

async fn audit_stream_surfaces(client: &YfClient, audit: &mut Audit) {
    for method in [
        StreamMethod::WebsocketWithFallback,
        StreamMethod::Websocket,
        StreamMethod::Polling,
    ] {
        audit_plain(
            audit,
            &format!("StreamBuilder::start({method:?},AAPL,BTC-USD)"),
            || collect_stream_updates(client, method),
            coerce_slice(summary_stream_updates),
        )
        .await;
    }
}

async fn collect_stream_updates(
    client: &YfClient,
    method: StreamMethod,
) -> Result<Vec<QuoteUpdate>, YfError> {
    let (handle, mut receiver) = StreamBuilder::new(client)
        .symbols([PRIMARY, CRYPTO])
        .method(method)
        .interval(Duration::from_secs(2))
        .diff_only(false)
        .cache_mode(CacheMode::Use)
        .start()
        .await?;

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut updates = Vec::new();
    while updates.len() < 8 {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        match timeout(remaining.min(Duration::from_secs(8)), receiver.recv()).await {
            Ok(Some(update)) => updates.push(update),
            Ok(None) | Err(_) => break,
        }
    }
    handle.stop().await;
    Ok(updates)
}

async fn audit_response<T, F, Fut, V>(audit: &mut Audit, surface: &str, op: F, validate: V)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<YfResponse<T>, YfError>>,
    V: Fn(&T) -> Result<String, String>,
{
    println!("running {surface}");
    match retry_after_rate_limit(op).await {
        Ok(response) => {
            let diagnostics = response.diagnostics.warnings.clone();
            match validate(&response.data) {
                Ok(summary) => {
                    let status = if diagnostics.is_empty() {
                        Status::Pass
                    } else {
                        Status::Warn
                    };
                    audit.push(Entry {
                        surface: surface.to_string(),
                        status,
                        diagnostics: Some(diagnostics.len()),
                        summary,
                        warnings: diagnostics,
                    });
                }
                Err(details) => audit.push(Entry {
                    surface: surface.to_string(),
                    status: Status::Fail,
                    diagnostics: Some(diagnostics.len()),
                    summary: details,
                    warnings: diagnostics,
                }),
            }
        }
        Err(error) => audit.push_error(surface, &error),
    }
}

async fn audit_plain<T, F, Fut, V>(audit: &mut Audit, surface: &str, op: F, validate: V)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, YfError>>,
    V: Fn(&T) -> Result<String, String>,
{
    println!("running {surface}");
    match retry_after_rate_limit(op).await {
        Ok(data) => match validate(&data) {
            Ok(summary) => audit.push(Entry {
                surface: surface.to_string(),
                status: Status::Pass,
                diagnostics: None,
                summary,
                warnings: Vec::new(),
            }),
            Err(details) => audit.push(Entry {
                surface: surface.to_string(),
                status: Status::Fail,
                diagnostics: None,
                summary: details,
                warnings: Vec::new(),
            }),
        },
        Err(error) => audit.push_error(surface, &error),
    }
}

fn coerce_slice<T>(
    validate: fn(&[T]) -> Result<String, String>,
) -> impl Fn(&Vec<T>) -> Result<String, String> {
    move |values| validate(values.as_slice())
}

fn coerce_option<T>(
    validate: fn(Option<&T>) -> Result<String, String>,
) -> impl Fn(&Option<T>) -> Result<String, String> {
    move |value| validate(value.as_ref())
}

async fn audit_expected_maybe_dead_response<T, F, Fut>(audit: &mut Audit, surface: &str, op: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<YfResponse<T>, YfError>>,
    T: std::fmt::Debug,
{
    println!("running {surface}");
    match retry_after_rate_limit(op).await {
        Ok(response) => audit.push(Entry {
            surface: surface.to_string(),
            status: Status::Warn,
            diagnostics: Some(response.diagnostics.len()),
            summary: format!(
                "ESG endpoint returned data; inspect manually because Yahoo ESG is known unstable: {:?}",
                response.data
            ),
            warnings: response.diagnostics.warnings,
        }),
        Err(error) => audit.push(Entry {
            surface: surface.to_string(),
            status: Status::Warn,
            diagnostics: Some(0),
            summary: format!("expected ESG unavailable/dead path: {error}"),
            warnings: Vec::new(),
        }),
    }
}

async fn audit_expected_maybe_dead_plain<T, F, Fut>(audit: &mut Audit, surface: &str, op: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, YfError>>,
    T: std::fmt::Debug,
{
    println!("running {surface}");
    match retry_after_rate_limit(op).await {
        Ok(data) => audit.push(Entry {
            surface: surface.to_string(),
            status: Status::Warn,
            diagnostics: None,
            summary: format!(
                "ESG endpoint returned data; inspect manually because Yahoo ESG is known unstable: {data:?}"
            ),
            warnings: Vec::new(),
        }),
        Err(error) => audit.push(Entry {
            surface: surface.to_string(),
            status: Status::Warn,
            diagnostics: None,
            summary: format!("expected ESG unavailable/dead path: {error}"),
            warnings: Vec::new(),
        }),
    }
}

async fn retry_after_rate_limit<T, F, Fut>(mut op: F) -> Result<T, YfError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, YfError>>,
{
    match op().await {
        Ok(value) => Ok(value),
        Err(YfError::RateLimited { url }) => {
            eprintln!("rate limited at {url}; waiting five minutes before retrying");
            tokio::time::sleep(Duration::from_mins(5)).await;
            op().await
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug)]
struct Entry {
    surface: String,
    status: Status,
    diagnostics: Option<usize>,
    summary: String,
    warnings: Vec<YfWarning>,
}

#[derive(Default)]
struct Audit {
    entries: Vec<Entry>,
}

impl Audit {
    fn push(&mut self, entry: Entry) {
        let status = match entry.status {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
        };
        let diagnostics = entry
            .diagnostics
            .map_or_else(|| "n/a".to_string(), |count| count.to_string());
        println!(
            "  {status:<4} diagnostics={diagnostics:<3} {}\n",
            entry.summary
        );
        self.entries.push(entry);
    }

    fn push_error(&mut self, surface: &str, error: &YfError) {
        self.push(Entry {
            surface: surface.to_string(),
            status: Status::Fail,
            diagnostics: None,
            summary: format!("request failed: {error}"),
            warnings: Vec::new(),
        });
    }

    fn print_report(&self) {
        let pass = self
            .entries
            .iter()
            .filter(|entry| entry.status == Status::Pass)
            .count();
        let warn = self
            .entries
            .iter()
            .filter(|entry| entry.status == Status::Warn)
            .count();
        let fail = self
            .entries
            .iter()
            .filter(|entry| entry.status == Status::Fail)
            .count();
        let diagnostics = self
            .entries
            .iter()
            .filter_map(|entry| entry.diagnostics)
            .sum::<usize>();

        println!("\n================ Final diagnostics audit report ================");
        println!(
            "surfaces={} pass={pass} warn={warn} fail={fail} diagnostics={diagnostics}",
            self.entries.len()
        );

        for entry in &self.entries {
            let status = match entry.status {
                Status::Pass => "PASS",
                Status::Warn => "WARN",
                Status::Fail => "FAIL",
            };
            let diagnostics = entry
                .diagnostics
                .map_or_else(|| "n/a".to_string(), |count| count.to_string());
            println!("\n[{status}] {} (diagnostics={diagnostics})", entry.surface);
            println!("  {}", entry.summary);
            for warning in &entry.warnings {
                println!("  warning: {warning:?}");
            }
        }
    }
}

fn summary_quotes(quotes: &[yfinance_rs::Quote]) -> Result<String, String> {
    require_non_empty("quotes", quotes)?;
    let priced = quotes.iter().filter(|quote| quote.price.is_some()).count();
    if priced != quotes.len() {
        return Err(format!("only {priced}/{} quotes had prices", quotes.len()));
    }
    let sample = &quotes[0];
    Ok(format!(
        "{} quotes, all priced; sample {} name={} price={} volume={:?} state={:?}",
        quotes.len(),
        sample.instrument,
        display_opt(sample.name.as_deref()),
        display_opt(sample.price.as_ref()),
        sample.day_volume,
        sample.market_state
    ))
}

fn summary_quote(quote: &yfinance_rs::Quote) -> Result<String, String> {
    let price = quote
        .price
        .as_ref()
        .ok_or_else(|| "quote had no price".to_string())?;
    if price.amount() <= Decimal::ZERO {
        return Err(format!("quote price is not positive: {price}"));
    }
    Ok(format!(
        "{} name={} price={} previous_close={} volume={:?} as_of={}",
        quote.instrument,
        display_opt(quote.name.as_deref()),
        price,
        display_opt(quote.previous_close.as_ref()),
        quote.day_volume,
        display_opt(quote.as_of.as_ref())
    ))
}

fn summary_snapshot(label: &str, snapshot: &Snapshot) -> Result<String, String> {
    let price = snapshot
        .last
        .as_ref()
        .ok_or_else(|| format!("{label} had no last price"))?;
    if price.amount() <= Decimal::ZERO {
        return Err(format!("{label} last price is not positive: {price}"));
    }
    Ok(format!(
        "{label}: {} name={} last={} previous_close={} volume={:?} as_of={}",
        snapshot.instrument,
        display_opt(snapshot.name.as_deref()),
        price,
        display_opt(snapshot.previous_close.as_ref()),
        snapshot.volume,
        display_opt(snapshot.as_of.as_ref())
    ))
}

fn summary_info(info: &Info) -> Result<String, String> {
    let snapshot = summary_snapshot("info.snapshot", &info.snapshot)?;
    let profile = info.profile.as_ref().map_or_else(
        || "profile=None".to_string(),
        |profile| match profile {
            Profile::Company(company) => {
                format!(
                    "company={} sector={}",
                    company.name,
                    display_opt(company.sector.as_deref())
                )
            }
            Profile::Fund(fund) => format!(
                "fund={} family={}",
                fund.name,
                display_opt(fund.family.as_deref())
            ),
            _ => "profile=unsupported future profile variant".to_string(),
        },
    );
    Ok(format!(
        "{snapshot}; {profile}; calendar_present={} price_target_present={} recommendation_present={}",
        info.calendar.is_some(),
        info.price_target.is_some(),
        info.recommendation_summary.is_some()
    ))
}

fn summary_key_statistics(stats: &KeyStatistics) -> Result<String, String> {
    if stats.market_cap.is_none()
        && stats.shares_outstanding.is_none()
        && stats.eps_trailing_twelve_months.is_none()
    {
        return Err("key statistics had no market cap, shares, or EPS".to_string());
    }
    Ok(format!(
        "market_cap={} shares={:?} eps_ttm={} pe_ttm={:?} beta={:?} as_of={}",
        display_opt(stats.market_cap.as_ref()),
        stats.shares_outstanding,
        display_opt(stats.eps_trailing_twelve_months.as_ref()),
        stats.pe_trailing_twelve_months,
        stats.beta,
        display_opt(stats.as_of.as_ref())
    ))
}

fn summary_candles(candles: &[Candle]) -> Result<String, String> {
    require_non_empty("candles", candles)?;
    let bad = candles
        .iter()
        .filter(|candle| {
            candle.open.amount() <= Decimal::ZERO
                || candle.high.amount() <= Decimal::ZERO
                || candle.low.amount() <= Decimal::ZERO
                || candle.close.amount() <= Decimal::ZERO
        })
        .count();
    if bad > 0 {
        return Err(format!(
            "{bad}/{} candles had non-positive OHLC",
            candles.len()
        ));
    }
    let first = &candles[0];
    let last = candles.last().expect("checked non-empty");
    Ok(format!(
        "{} candles from {} to {}; sample close={} volume={:?}",
        candles.len(),
        first.ts.date_naive(),
        last.ts.date_naive(),
        last.close,
        last.volume
    ))
}

fn summary_history(history: &HistoryResponse) -> Result<String, String> {
    let candles = summary_candles(&history.candles)?;
    Ok(format!(
        "{candles}; actions={} adjusted={} timezone={} offset={:?}",
        history.actions.len(),
        history.adjusted,
        history
            .meta
            .as_ref()
            .and_then(|meta| meta.timezone)
            .map_or_else(|| "None".to_string(), |timezone| timezone.to_string()),
        history
            .meta
            .as_ref()
            .and_then(|meta| meta.utc_offset_seconds)
    ))
}

fn summary_download(download: &DownloadResponse) -> Result<String, String> {
    require_non_empty("download entries", &download.entries)?;
    let mut parts = Vec::new();
    for entry in &download.entries {
        if entry.history.candles.is_empty() {
            return Err(format!("{} had no candles", entry.instrument));
        }
        parts.push(format!(
            "{}:{} candles",
            entry.instrument,
            entry.history.candles.len()
        ));
    }
    Ok(parts.join(", "))
}

fn summary_actions(actions: &[Action]) -> Result<String, String> {
    require_non_empty("actions", actions)?;
    let dividends = actions
        .iter()
        .filter(|action| matches!(action, Action::Dividend { .. }))
        .count();
    let splits = actions
        .iter()
        .filter(|action| matches!(action, Action::Split { .. }))
        .count();
    let capital_gains = actions
        .iter()
        .filter(|action| matches!(action, Action::CapitalGain { .. }))
        .count();
    Ok(format!(
        "{} actions: dividends={dividends} splits={splits} capital_gains={capital_gains}",
        actions.len()
    ))
}

fn summary_history_meta(meta: Option<&yfinance_rs::HistoryMeta>) -> Result<String, String> {
    let meta = meta.ok_or_else(|| "history metadata was None".to_string())?;
    Ok(format!(
        "timezone={} offset={:?}",
        meta.timezone
            .map_or_else(|| "None".to_string(), |timezone| timezone.to_string()),
        meta.utc_offset_seconds
    ))
}

fn summary_isin(isin: Option<&String>) -> Result<String, String> {
    let isin = isin
        .map(String::as_str)
        .ok_or_else(|| "ISIN lookup returned None".to_string())?;
    if isin.len() != 12 {
        return Err(format!(
            "ISIN length was {}, expected 12: {isin}",
            isin.len()
        ));
    }
    Ok(format!("isin={isin}"))
}

fn summary_expirations(expirations: &[i64]) -> Result<String, String> {
    require_non_empty("option expirations", expirations)?;
    Ok(format!(
        "{} expirations; first={} last={}",
        expirations.len(),
        expirations[0],
        expirations[expirations.len() - 1]
    ))
}

fn summary_option_chain(chain: &OptionChain) -> Result<String, String> {
    require_non_empty("option contracts", &chain.contracts)?;
    let priced = chain
        .contracts
        .iter()
        .filter(|contract| {
            contract.price.is_some() || contract.bid.is_some() || contract.ask.is_some()
        })
        .count();
    if priced == 0 {
        return Err("option chain had contracts but no price/bid/ask fields".to_string());
    }
    let calls = chain.calls().count();
    let puts = chain.puts().count();
    let sample = &chain.contracts[0];
    Ok(format!(
        "{} contracts: calls={calls} puts={puts} priced_or_quoted={priced}; sample side={} strike={} expiration={}",
        chain.contracts.len(),
        sample.key.side,
        sample.key.strike,
        sample.key.expiration_date
    ))
}

fn summary_recommendations(rows: &[RecommendationRow]) -> Result<String, String> {
    require_non_empty("recommendation rows", rows)?;
    let sample = &rows[0];
    Ok(format!(
        "{} recommendation periods; sample {} strong_buy={:?} buy={:?} hold={:?} sell={:?}",
        rows.len(),
        sample.period,
        sample.strong_buy,
        sample.buy,
        sample.hold,
        sample.sell
    ))
}

fn summary_recommendation_summary(summary: &RecommendationSummary) -> Result<String, String> {
    if summary.mean.is_none()
        && summary.strong_buy.is_none()
        && summary.buy.is_none()
        && summary.hold.is_none()
    {
        return Err("recommendation summary had no mean or rating buckets".to_string());
    }
    Ok(format!(
        "period={} mean={:?} text={} strong_buy={:?} buy={:?} hold={:?}",
        display_opt(summary.latest_period.as_ref()),
        summary.mean,
        display_opt(summary.mean_rating_text.as_deref()),
        summary.strong_buy,
        summary.buy,
        summary.hold
    ))
}

fn summary_upgrades(rows: &[UpgradeDowngradeRow]) -> Result<String, String> {
    require_non_empty("upgrade/downgrade rows", rows)?;
    let sample = &rows[0];
    Ok(format!(
        "{} rows; sample {} firm={} action={}",
        rows.len(),
        sample.ts.date_naive(),
        display_opt(sample.firm.as_deref()),
        display_opt(sample.action.as_ref())
    ))
}

fn summary_price_target(target: &PriceTarget) -> Result<String, String> {
    if target.mean.is_none() && target.high.is_none() && target.low.is_none() {
        return Err("price target had no price fields".to_string());
    }
    Ok(format!(
        "mean={} high={} low={} analysts={:?}",
        display_opt(target.mean.as_ref()),
        display_opt(target.high.as_ref()),
        display_opt(target.low.as_ref()),
        target.number_of_analysts
    ))
}

fn summary_earnings_trend(rows: &[EarningsTrendRow]) -> Result<String, String> {
    require_non_empty("earnings trend rows", rows)?;
    let sample = &rows[0];
    Ok(format!(
        "{} trend periods; sample {} earnings_avg={} revenue_avg={} eps_revision_points={}",
        rows.len(),
        sample.period,
        display_opt(sample.earnings_estimate.avg.as_ref()),
        display_opt(sample.revenue_estimate.avg.as_ref()),
        sample.eps_revisions.historical.len()
    ))
}

fn summary_income_statement(rows: &[IncomeStatementRow]) -> Result<String, String> {
    require_non_empty("income statement rows", rows)?;
    if !rows.iter().any(|row| {
        row.total_revenue.is_some()
            || row.gross_profit.is_some()
            || row.operating_income.is_some()
            || row.net_income.is_some()
    }) {
        return Err("income statement rows had no core monetary fields".to_string());
    }
    let sample = &rows[0];
    Ok(format!(
        "{} rows; sample period={} revenue={} net_income={}",
        rows.len(),
        sample.period,
        display_opt(sample.total_revenue.as_ref()),
        display_opt(sample.net_income.as_ref())
    ))
}

fn summary_balance_sheet(rows: &[BalanceSheetRow]) -> Result<String, String> {
    require_non_empty("balance sheet rows", rows)?;
    if !rows.iter().any(|row| {
        row.total_assets.is_some() || row.total_liabilities.is_some() || row.total_equity.is_some()
    }) {
        return Err("balance sheet rows had no core monetary fields".to_string());
    }
    let sample = &rows[0];
    Ok(format!(
        "{} rows; sample period={} assets={} liabilities={} equity={}",
        rows.len(),
        sample.period,
        display_opt(sample.total_assets.as_ref()),
        display_opt(sample.total_liabilities.as_ref()),
        display_opt(sample.total_equity.as_ref())
    ))
}

fn summary_cashflow(rows: &[CashflowRow]) -> Result<String, String> {
    require_non_empty("cashflow rows", rows)?;
    if !rows.iter().any(|row| {
        row.operating_cashflow.is_some()
            || row.capital_expenditures.is_some()
            || row.free_cash_flow.is_some()
    }) {
        return Err("cashflow rows had no core monetary fields".to_string());
    }
    let sample = &rows[0];
    Ok(format!(
        "{} rows; sample period={} operating_cashflow={} free_cash_flow={}",
        rows.len(),
        sample.period,
        display_opt(sample.operating_cashflow.as_ref()),
        display_opt(sample.free_cash_flow.as_ref())
    ))
}

fn summary_earnings(earnings: &Earnings) -> Result<String, String> {
    if earnings.yearly.is_empty()
        && earnings.quarterly.is_empty()
        && earnings.quarterly_eps.is_empty()
    {
        return Err("earnings response had no yearly, quarterly, or EPS rows".to_string());
    }
    Ok(format!(
        "yearly={} quarterly={} quarterly_eps={}",
        earnings.yearly.len(),
        earnings.quarterly.len(),
        earnings.quarterly_eps.len()
    ))
}

fn summary_calendar(calendar: &Calendar) -> Result<String, String> {
    if calendar.earnings_dates.is_empty()
        && calendar.ex_dividend_date.is_none()
        && calendar.dividend_payment_date.is_none()
    {
        return Err("calendar had no earnings or dividend dates".to_string());
    }
    Ok(format!(
        "earnings_dates={} first_earnings={} ex_dividend={} dividend_payment={}",
        calendar.earnings_dates.len(),
        display_opt(calendar.earnings_dates.first()),
        display_opt(calendar.ex_dividend_date.as_ref()),
        display_opt(calendar.dividend_payment_date.as_ref())
    ))
}

fn summary_shares(shares: &[ShareCount]) -> Result<String, String> {
    require_non_empty("share-count rows", shares)?;
    let sample = &shares[0];
    if sample.shares == 0 {
        return Err("first share-count row had zero shares".to_string());
    }
    Ok(format!(
        "{} rows; sample {} shares={}",
        shares.len(),
        sample.date.date_naive(),
        sample.shares
    ))
}

fn summary_major_holders(rows: &[MajorHolder]) -> Result<String, String> {
    require_non_empty("major holder rows", rows)?;
    let sample = &rows[0];
    if sample.category.trim().is_empty() {
        return Err("major holder category was empty".to_string());
    }
    Ok(format!(
        "{} rows; sample category={} value={}",
        rows.len(),
        sample.category,
        sample.value
    ))
}

fn summary_institutional_holders(rows: &[InstitutionalHolder]) -> Result<String, String> {
    require_non_empty("holder rows", rows)?;
    let sample = &rows[0];
    if sample.holder.trim().is_empty() {
        return Err("holder name was empty".to_string());
    }
    Ok(format!(
        "{} rows; sample holder={} shares={:?} value={} date={}",
        rows.len(),
        sample.holder,
        sample.shares,
        display_opt(sample.value.as_ref()),
        sample.date_reported.date_naive()
    ))
}

fn summary_insider_transactions(rows: &[InsiderTransaction]) -> Result<String, String> {
    require_non_empty("insider transaction rows", rows)?;
    if let Some(row) = rows.iter().find(|row| row.insider.trim().is_empty()) {
        return Err(format!(
            "insider transaction had empty insider for {} {} shares={:?}",
            row.transaction_date.date_naive(),
            row.transaction_type,
            row.shares
        ));
    }
    let sample = &rows[0];
    let missing_urls = rows.iter().filter(|row| row.url.trim().is_empty()).count();
    Ok(format!(
        "{} rows; missing_urls={missing_urls}; sample insider={} type={} shares={:?} value={} date={}",
        rows.len(),
        sample.insider,
        sample.transaction_type,
        sample.shares,
        display_opt(sample.value.as_ref()),
        sample.transaction_date.date_naive()
    ))
}

fn summary_insider_roster(rows: &[InsiderRosterHolder]) -> Result<String, String> {
    require_non_empty("insider roster rows", rows)?;
    let sample = &rows[0];
    if sample.name.trim().is_empty() {
        return Err("insider roster name was empty".to_string());
    }
    Ok(format!(
        "{} rows; sample name={} position={} shares={:?} latest_tx={}",
        rows.len(),
        sample.name,
        sample.position,
        sample.shares_owned_directly,
        sample.latest_transaction_date.date_naive()
    ))
}

fn summary_net_share_purchase_activity(
    activity: Option<&NetSharePurchaseActivity>,
) -> Result<String, String> {
    let activity = activity.ok_or_else(|| "net share purchase activity was None".to_string())?;
    Ok(format!(
        "period={} buy_shares={:?} sell_shares={:?} net_shares={:?} total_insider_shares={:?}",
        activity.period,
        activity.buy_shares,
        activity.sell_shares,
        activity.net_shares,
        activity.total_insider_shares
    ))
}

fn summary_news(articles: &[NewsArticle], allow_empty: bool) -> Result<String, String> {
    if articles.is_empty() {
        return if allow_empty {
            Ok("0 articles; Yahoo returned no rows for this optional tab".to_string())
        } else {
            Err("news response had no articles".to_string())
        };
    }
    let sample = &articles[0];
    if sample.uuid.trim().is_empty() || sample.title.trim().is_empty() {
        return Err("news article had empty UUID or title".to_string());
    }
    Ok(format!(
        "{} articles; sample title={:?} publisher={} published={}",
        articles.len(),
        sample.title,
        display_opt(sample.publisher.as_deref()),
        sample.published_at.date_naive()
    ))
}

fn summary_search(response: &SearchResponse) -> Result<String, String> {
    require_non_empty("search results", &response.results)?;
    let sample = &response.results[0];
    Ok(format!(
        "{} results; sample {} name={}",
        response.results.len(),
        sample.instrument,
        display_opt(sample.name.as_deref())
    ))
}

fn summary_screener(response: &ScreenerResponse) -> Result<String, String> {
    require_non_empty("screener results", &response.results)?;
    let priced = response
        .results
        .iter()
        .filter(|result| result.price.is_some())
        .count();
    let sample = &response.results[0];
    Ok(format!(
        "{} returned / {:?} reported; priced={priced}; sample symbol={} name={} price={} exchange={}",
        response.results.len(),
        response.count,
        display_opt(sample.symbol.as_deref()),
        display_opt(sample.name.as_deref()),
        display_opt(sample.price.as_ref()),
        display_opt(sample.raw_exchange.as_deref())
    ))
}

fn summary_profile(profile: &Profile) -> Result<String, String> {
    match profile {
        Profile::Company(company) => {
            if company.name.trim().is_empty() {
                return Err("company profile name was empty".to_string());
            }
            Ok(format!(
                "company={} sector={} industry={} website={} isin={}",
                company.name,
                display_opt(company.sector.as_deref()),
                display_opt(company.industry.as_deref()),
                display_opt(company.website.as_deref()),
                display_opt(company.isin.as_ref())
            ))
        }
        Profile::Fund(fund) => {
            if fund.name.trim().is_empty() {
                return Err("fund profile name was empty".to_string());
            }
            Ok(format!(
                "fund={} family={} kind={} isin={}",
                fund.name,
                display_opt(fund.family.as_deref()),
                fund.kind,
                display_opt(fund.isin.as_ref())
            ))
        }
        _ => Ok("unsupported future profile variant".to_string()),
    }
}

fn summary_stream_updates(updates: &[QuoteUpdate]) -> Result<String, String> {
    require_non_empty("stream updates", updates)?;
    let priced = updates
        .iter()
        .filter(|update| update.price.is_some())
        .count();
    if priced == 0 {
        return Err(format!(
            "{} stream updates had timestamps/volumes but no price fields",
            updates.len()
        ));
    }
    let sample = updates
        .iter()
        .find(|update| update.price.is_some())
        .expect("priced count checked");
    Ok(format!(
        "{} updates; priced={priced}; sample {} price={} previous_close={} volume={:?} ts={}",
        updates.len(),
        sample.instrument,
        display_opt(sample.price.as_ref()),
        display_opt(sample.previous_close.as_ref()),
        sample.volume,
        sample.ts
    ))
}

fn require_non_empty<T>(label: &str, values: &[T]) -> Result<(), String> {
    if values.is_empty() {
        Err(format!("{label} was empty"))
    } else {
        Ok(())
    }
}

fn display_opt<T: Display>(value: Option<T>) -> String {
    value.map_or_else(|| "N/A".to_string(), |value| value.to_string())
}
