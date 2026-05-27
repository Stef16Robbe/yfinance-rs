use chrono::Duration;
use rust_decimal::Decimal;
use std::fmt::Display;
use yfinance_rs::{SearchBuilder, Ticker, YfClientBuilder};

fn display_opt<T: Display>(value: Option<T>) -> String {
    value.map_or_else(|| "N/A".to_string(), |value| value.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = YfClientBuilder::default()
        .timeout(Duration::seconds(5).to_std()?)
        .build()?;

    section_esg(&client).await?;
    section_analysis(&client).await?;
    section_search(&client).await?;
    Ok(())
}

async fn section_esg(client: &yfinance_rs::YfClient) -> Result<(), Box<dyn std::error::Error>> {
    let msft_ticker = Ticker::new(client, "MSFT");
    let esg_scores = msft_ticker.sustainability().await;
    println!("--- ESG Availability for MSFT ---");
    match esg_scores {
        Ok(summary) => {
            if let Some(scores) = summary.scores {
                let total_esg = [scores.environmental, scores.social, scores.governance]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                let total_esg_score = if total_esg.is_empty() {
                    Decimal::ZERO
                } else {
                    let denom = Decimal::from(total_esg.len() as u64);
                    total_esg.iter().copied().sum::<Decimal>() / denom
                };
                println!("Total ESG Score: {total_esg_score:.2}");
                println!("Environmental Score: {}", display_opt(scores.environmental));
                println!("Social Score: {}", display_opt(scores.social));
                println!("Governance Score: {}", display_opt(scores.governance));
            } else {
                println!("No ESG scores returned by Yahoo for this ticker.");
            }
            if !summary.involvement.is_empty() {
                println!("Involvement categories ({}):", summary.involvement.len());
                for inv in summary.involvement.iter().take(5) {
                    println!("  - {}", inv.category);
                }
            }
        }
        Err(e) => println!("ESG scores unavailable from Yahoo for this ticker: {e}"),
    }
    println!("--------------------------------------\n");
    Ok(())
}

async fn section_analysis(
    client: &yfinance_rs::YfClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let tsla_ticker = Ticker::new(client, "TSLA");
    let recommendations = tsla_ticker.recommendations().await;
    println!("--- Analyst Recommendations for TSLA ---");
    match recommendations {
        Ok(recs) => {
            if let Some(latest) = recs.first() {
                println!(
                    "Latest Recommendation Period ({}): Strong Buy: {}, Buy: {}, Hold: {}, Sell: {}, Strong Sell: {}",
                    latest.period,
                    display_opt(latest.strong_buy),
                    display_opt(latest.buy),
                    display_opt(latest.hold),
                    display_opt(latest.sell),
                    display_opt(latest.strong_sell)
                );
            }
        }
        Err(e) => println!("Recommendations unavailable from Yahoo: {e}"),
    }
    let upgrades = tsla_ticker.upgrades_downgrades().await;
    if let Ok(upgrades_list) = upgrades {
        println!("\nRecent Upgrades/Downgrades:");
        for upgrade in upgrades_list.iter().take(3) {
            println!(
                "  - Firm: {} | Action: {} | From: {} | To: {}",
                upgrade.firm.as_deref().unwrap_or("N/A"),
                upgrade
                    .action
                    .as_ref()
                    .map_or_else(|| "N/A".to_string(), std::string::ToString::to_string),
                upgrade
                    .from_grade
                    .as_ref()
                    .map_or_else(|| "N/A".to_string(), std::string::ToString::to_string),
                upgrade
                    .to_grade
                    .as_ref()
                    .map_or_else(|| "N/A".to_string(), std::string::ToString::to_string)
            );
        }
    }
    println!("--------------------------------------\n");
    Ok(())
}

async fn section_search(client: &yfinance_rs::YfClient) -> Result<(), Box<dyn std::error::Error>> {
    let query = "Apple Inc.";
    let search_results = SearchBuilder::new(client, query).fetch().await;
    println!("--- Searching for '{query}' ---");
    match search_results {
        Ok(results) => {
            println!("Found {} results:", results.results.len());
            for quote in results.results.iter().take(5) {
                println!(
                    "  - {} ({}) : {}",
                    quote.instrument,
                    quote.instrument.kind,
                    quote.name.as_deref().unwrap_or("N/A")
                );
            }
        }
        Err(e) => println!("Search unavailable from Yahoo: {e}"),
    }
    println!("--------------------------------------");
    Ok(())
}
