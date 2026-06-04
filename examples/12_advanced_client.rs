use std::fmt::Display;
use std::time::Duration;
use yfinance_rs::{
    Ticker, YfClientBuilder, YfError,
    core::client::{Backoff, RetryConfig},
};

fn display_opt<T: Display>(value: Option<&T>) -> String {
    value.map_or_else(|| "N/A".to_string(), ToString::to_string)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. --- Advanced Client Configuration ---
    println!("--- Building a client with custom configuration ---");
    let custom_retry = RetryConfig {
        enabled: true,
        max_retries: 2,
        backoff: Backoff::Fixed(Duration::from_millis(500)),
        ..Default::default()
    };
    let client = YfClientBuilder::default()
        .retry_config(custom_retry)
        .cache_ttl(Duration::from_mins(1)) // Cache responses for 60 seconds
        .build()?;
    println!("Client built with custom retry policy.");
    println!();

    // 2. --- Using the custom client ---
    let aapl = Ticker::new(&client, "AAPL");
    let quote1 = aapl.quote().await?;
    println!(
        "First fetch for {}: {} (from network)",
        quote1.instrument,
        display_opt(quote1.price.as_ref())
    );
    let quote2 = aapl.quote().await?;
    println!(
        "Second fetch for {}: {} (should be from cache)",
        quote2.instrument,
        display_opt(quote2.price.as_ref())
    );
    println!();

    // 3. --- Cache Management ---
    println!("--- Managing the client cache ---");
    client.clear_cache().await;
    println!("Client cache cleared.");
    let quote3 = aapl.quote().await?;
    println!(
        "Third fetch for {}: {} (from network again)",
        quote3.instrument,
        display_opt(quote3.price.as_ref())
    );
    println!();

    // 4. --- Demonstrating a missing data point (dividend date) ---
    println!("--- Fetching Calendar Events for AAPL (including dividend date) ---");
    let calendar = aapl.calendar().await?;
    if let Some(date) = calendar.ex_dividend_date {
        println!("  Dividend date: {date}");
    } else {
        println!("  No upcoming dividend date found.");
    }
    println!();

    // 5. --- Error Handling Example ---
    println!("--- Handling a non-existent ticker ---");
    let bad_ticker = Ticker::new(&client, "THIS-TICKER-DOES-NOT-EXIST-XYZ");
    match bad_ticker.info().await {
        Ok(_) => println!("Unexpected success fetching bad ticker."),
        Err(YfError::MissingData(msg)) => {
            println!("Correctly failed with a missing data error: {msg}");
        }
        Err(e) => {
            println!("Failed with an unexpected error type: {e}");
        }
    }

    Ok(())
}
