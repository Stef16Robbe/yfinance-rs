//! Debug dump helpers for development / troubleshooting.

use std::io::Write;

pub fn debug_dump_api(symbol: &str, body: &str) -> std::io::Result<()> {
    let path = std::env::temp_dir().join(format!("yfinance_rs-quoteSummary-{symbol}.json"));
    let mut f = std::fs::File::create(&path)?;
    let _ = f.write_all(body.as_bytes());
    crate::core::logging::trace_info!(path = %path.display(), "wrote quoteSummary debug dump");
    Ok(())
}
