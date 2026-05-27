use crate::core::conversions::{
    decimal_from_f64, i64_to_datetime, price_from_f64_with_currency_str,
};
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
        let Ok(ts) = i64_to_datetime(t) else {
            continue;
        };
        let getter_f64 = |v: &Vec<Option<f64>>| v.get(i).and_then(|x| *x);
        let open = getter_f64(&q.open);
        let high = getter_f64(&q.high);
        let low = getter_f64(&q.low);
        let close = getter_f64(&q.close);
        let volume0 = q.volume.get(i).and_then(|x| *x);

        let Some((mut open, mut high, mut low, mut close)) =
            raw_ohlc_values(open, high, low, close)
        else {
            continue;
        };
        let raw_close = close;

        if auto_adjust {
            let pf =
                price_factor_for_row(i, adj.get(i).and_then(|x| *x), Some(close), cum_split_after);

            open *= pf;
            high *= pf;
            low *= pf;
            close *= pf;
        }

        if let Some((open, high, low, close)) = candle_prices(open, high, low, close, currency) {
            out.push(Candle {
                ts,
                open,
                high,
                low,
                close,
                close_unadj: price_from_f64_with_currency_str(raw_close, currency),
                volume: volume0,
                provider: (),
            });
        }
    }

    out
}

fn raw_ohlc_values(
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
) -> Option<(f64, f64, f64, f64)> {
    Some((
        valid_decimal_value(open?)?,
        valid_decimal_value(high?)?,
        valid_decimal_value(low?)?,
        valid_decimal_value(close?)?,
    ))
}

fn valid_decimal_value(value: f64) -> Option<f64> {
    decimal_from_f64(value).map(|_| value)
}

fn candle_prices(
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    currency: Option<&str>,
) -> Option<(Price, Price, Price, Price)> {
    Some((
        price_from_f64_with_currency_str(open, currency)?,
        price_from_f64_with_currency_str(high, currency)?,
        price_from_f64_with_currency_str(low, currency)?,
        price_from_f64_with_currency_str(close, currency)?,
    ))
}
