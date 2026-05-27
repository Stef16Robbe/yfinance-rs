use paft::Decimal;
use std::fmt::Display;
use yfinance_rs::{Ticker, YfClient};

fn display_opt<T: Display>(value: Option<T>) -> String {
    value.map_or_else(|| "N/A".to_string(), |value| value.to_string())
}

fn display_label(value: &impl Display) -> String {
    value.to_string().replace('_', " ")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = YfClient::default();
    let ticker = Ticker::new(&client, "TSLA");

    println!("--- Fetching Holder Information for TSLA ---");

    // Mutual Fund Holders
    let mf_holders = ticker.mutual_fund_holders().await?;
    println!("\nTop 5 Mutual Fund Holders:");
    for holder in mf_holders.iter().take(5) {
        println!(
            "  - {}: {} shares ({:.2}%)",
            holder.holder,
            display_opt(holder.shares),
            holder.pct_held.unwrap_or(Decimal::ZERO) * Decimal::from(100)
        );
    }

    // Insider Transactions
    let insider_txns = ticker.insider_transactions().await?;
    println!("\nLatest 5 Insider Transactions:");
    for txn in insider_txns.iter().take(5) {
        println!(
            "  - {}: {} {} shares on {}",
            txn.insider,
            display_label(&txn.transaction_type),
            display_opt(txn.shares),
            txn.transaction_date.date_naive()
        );
    }

    // Insider Roster
    let insider_roster = ticker.insider_roster_holders().await?;
    println!("\nTop 5 Insider Roster:");
    for insider in insider_roster.iter().take(5) {
        println!(
            "  - {} ({}): {} shares",
            insider.name,
            display_label(&insider.position),
            display_opt(insider.shares_owned_directly)
        );
    }

    println!("-----------------------------------------");

    Ok(())
}
