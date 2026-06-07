use chrono::{Duration, Utc};
use yfinance_rs::core::{Action, Interval, Range};
use yfinance_rs::{DownloadBuilder, Ticker, YfClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = YfClient::default();

    // --- Part 1: Fetching Historical Dividends and Splits ---
    let aapl_ticker = Ticker::new(&client, "AAPL");

    println!("--- Fetching Historical Actions for AAPL (last 5 years) ---");
    let actions = aapl_ticker.actions(Some(Range::Y5)).await?;
    let dividends = actions
        .iter()
        .filter(|action| matches!(action, Action::Dividend { .. }))
        .count();
    println!("Found {dividends} dividends in the last 5 years.");
    if let Some(Action::Dividend { date, amount }) = actions
        .iter()
        .rev()
        .find(|action| matches!(action, Action::Dividend { .. }))
    {
        println!("  Latest dividend: {amount} on {date}");
    }

    let splits = actions
        .iter()
        .filter(|action| matches!(action, Action::Split { .. }))
        .count();
    println!("\nFound {splits} splits in the last 5 years.");
    for action in &actions {
        if let Action::Split {
            date,
            numerator,
            denominator,
        } = action
        {
            println!("  - Split of {numerator}:{denominator} on {date}");
        }
    }
    println!("--------------------------------------\n");

    // --- Part 2: Advanced Multi-Symbol Download with Customization ---
    let symbols = vec!["AAPL", "GOOGL", "MSFT", "AMZN"];
    println!("--- Downloading Custom Historical Data for Multiple Symbols ---");
    println!("Fetching 1-week, back-adjusted data for the last 30 days...");

    let thirty_days_ago = Utc::now() - Duration::days(30);
    let now = Utc::now();

    let results = DownloadBuilder::new(&client)
        .symbols(symbols)
        .between(thirty_days_ago, now)
        .interval(Interval::W1)
        .back_adjust() // show back-adjustment
        .rounding(true) // show rounding
        .run()
        .await?;

    for entry in &results.entries {
        let symbol = entry.instrument.symbol.as_str();
        let candles = &entry.history.candles;
        println!("- {symbol} ({} candles)", candles.len());
        if let Some(first_candle) = candles.first() {
            println!("  First Open: {}", first_candle.ohlc.open);
        }
        if let Some(last_candle) = candles.last() {
            println!("  Last Close: {}", last_candle.ohlc.close);
        }
    }
    println!("--------------------------------------");

    let meta = aapl_ticker.get_history_metadata(Some(Range::Y1)).await?;
    println!("\n--- History Metadata for AAPL ---");
    if let Some(m) = meta {
        println!("  Timezone: {}", m.timezone.unwrap_or_default());
        println!("  GMT Offset: {}", m.utc_offset_seconds.unwrap_or_default());
    }
    println!("--------------------------------------");

    Ok(())
}
