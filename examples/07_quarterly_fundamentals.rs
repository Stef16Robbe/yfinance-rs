use std::fmt::Display;
use yfinance_rs::{Ticker, YfClient};

fn display_opt<T: Display>(value: Option<&T>) -> String {
    value.map_or_else(|| "N/A".to_string(), ToString::to_string)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = YfClient::default();
    let ticker = Ticker::new(&client, "MSFT");

    println!("--- Fetching Quarterly Financial Statements for MSFT ---");
    println!("Fetching latest quarterly income statement...");
    let income_stmt = ticker.quarterly_income_stmt(None).await?;
    if let Some(latest) = income_stmt.first() {
        println!(
            "Latest quarterly revenue: {} (from {})",
            display_opt(latest.total_revenue.as_ref()),
            latest.period
        );
    } else {
        println!("No quarterly income statement found.");
    }

    println!("\nFetching latest quarterly balance sheet...");
    let balance_sheet = ticker.quarterly_balance_sheet(None).await?;
    if let Some(latest) = balance_sheet.first() {
        println!(
            "Latest quarterly total assets: {} (from {})",
            display_opt(latest.total_assets.as_ref()),
            latest.period
        );
    } else {
        println!("No quarterly balance sheet found.");
    }

    println!("\nFetching latest quarterly cash flow statement...");
    let cashflow_stmt = ticker.quarterly_cashflow(None).await?;
    if let Some(latest) = cashflow_stmt.first() {
        println!(
            "Latest quarterly operating cash flow: {} (from {})",
            display_opt(latest.operating_cashflow.as_ref()),
            latest.period
        );
    } else {
        println!("No quarterly cash flow statement found.");
    }

    println!("\nFetching latest quarterly shares outstanding...");
    let shares = ticker.quarterly_shares().await?;
    if let Some(latest) = shares.first() {
        println!(
            "Latest quarterly shares outstanding: {} (from {})",
            latest.shares, latest.date
        );
    } else {
        println!("No quarterly shares outstanding found.");
    }

    Ok(())
}
