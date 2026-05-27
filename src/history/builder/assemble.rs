use crate::core::conversions::{i64_to_datetime, price_from_f64_with_currency_str};
use crate::history::wire::QuoteBlock;
use paft::market::responses::history::Candle;
use paft::money::Price;

use super::adjust::price_factor_for_row;

pub fn assemble_candles(
    ts: &[i64],
    q: &QuoteBlock,
    adj: &[Option<f64>],
    auto_adjust: bool,
    cum_split_after: &[f64],
    currency: Option<&str>,
) -> Vec<Candle> {
    let mut out = Vec::new();

    for (i, &t) in ts.iter().enumerate() {
        let getter_f64 = |v: &Vec<Option<f64>>| v.get(i).and_then(|x| *x);
        let mut open = getter_f64(&q.open);
        let mut high = getter_f64(&q.high);
        let mut low = getter_f64(&q.low);
        let mut close = getter_f64(&q.close);
        let volume0 = q.volume.get(i).and_then(|x| *x);

        let raw_close = close;

        if auto_adjust {
            let pf = price_factor_for_row(i, adj.get(i).and_then(|x| *x), close, cum_split_after);

            if let Some(v) = open.as_mut() {
                *v *= pf;
            }
            if let Some(v) = high.as_mut() {
                *v *= pf;
            }
            if let Some(v) = low.as_mut() {
                *v *= pf;
            }
            if let Some(v) = close.as_mut() {
                *v *= pf;
            }
        }

        if let Some((open, high, low, close)) = candle_prices(open, high, low, close, currency) {
            out.push(Candle {
                ts: i64_to_datetime(t),
                open,
                high,
                low,
                close,
                close_unadj: raw_close
                    .and_then(|value| price_from_f64_with_currency_str(value, currency)),
                volume: volume0,
                provider: (),
            });
        }
    }

    out
}

fn candle_prices(
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
    currency: Option<&str>,
) -> Option<(Price, Price, Price, Price)> {
    Some((
        price_from_f64_with_currency_str(open?, currency)?,
        price_from_f64_with_currency_str(high?, currency)?,
        price_from_f64_with_currency_str(low?, currency)?,
        price_from_f64_with_currency_str(close?, currency)?,
    ))
}
