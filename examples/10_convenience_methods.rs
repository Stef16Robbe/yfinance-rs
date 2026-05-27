use std::fmt::Display;
use yfinance_rs::core::{Action, Interval, Range};
use yfinance_rs::{Ticker, YfClient};

fn display_opt<T: Display>(value: Option<&T>) -> String {
    value.map_or_else(|| "N/A".to_string(), ToString::to_string)
}

fn display_action(action: &Action) -> String {
    match action {
        Action::Dividend { ts, amount } => {
            format!("dividend of {amount} on {}", ts.date_naive())
        }
        Action::Split {
            ts,
            numerator,
            denominator,
        } => format!("split {numerator}:{denominator} on {}", ts.date_naive()),
        Action::CapitalGain { ts, gain } => {
            format!("capital gain of {gain} on {}", ts.date_naive())
        }
        _ => "other corporate action".to_string(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = YfClient::default();
    let ticker = Ticker::new(&client, "AAPL");

    println!("--- Ticker Quote (Convenience) ---");
    let quote = ticker.quote().await?;
    let vol = quote
        .day_volume
        .map(|v| format!(" (vol: {v})"))
        .unwrap_or_default();
    println!(
        "  {}: {} (prev_close: {}){}",
        quote.instrument,
        display_opt(quote.price.as_ref()),
        display_opt(quote.previous_close.as_ref()),
        vol
    );
    println!();

    println!("--- Ticker News (Convenience, default count) ---");
    let news = ticker.news().await?;
    println!("  Found {} articles with default settings.", news.len());
    if let Some(article) = news.first() {
        println!("  First article: {}", article.title);
    }
    println!();

    println!("--- Ticker History (Convenience, last 5 days) ---");
    let history = ticker
        .history(Some(Range::D5), Some(Interval::D1), false)
        .await?;
    if let Some(candle) = history.last() {
        println!(
            "  Last close on {}: {}",
            candle.ts.date_naive(),
            candle.close
        );
    }
    println!();

    println!("--- Ticker Actions (Convenience, YTD) ---");
    let actions = ticker.actions(Some(Range::Ytd)).await?;
    println!("  Found {} actions (dividends/splits) YTD.", actions.len());
    if let Some(action) = actions.last() {
        println!("  Most recent action: {}", display_action(action));
    }
    println!();

    println!("--- Annual Financials (Convenience) ---");
    let annual_income = ticker.income_stmt(None).await?;
    if let Some(stmt) = annual_income.first() {
        println!(
            "  Latest annual revenue: {}",
            display_opt(stmt.total_revenue.as_ref())
        );
    }

    let annual_balance = ticker.balance_sheet(None).await?;
    if let Some(stmt) = annual_balance.first() {
        println!(
            "  Latest annual assets: {}",
            display_opt(stmt.total_assets.as_ref())
        );
    }

    let annual_cashflow = ticker.cashflow(None).await?;
    if let Some(stmt) = annual_cashflow.first() {
        println!(
            "  Latest annual free cash flow: {}",
            display_opt(stmt.free_cash_flow.as_ref())
        );
    }

    Ok(())
}
