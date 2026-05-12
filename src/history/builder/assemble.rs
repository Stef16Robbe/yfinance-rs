use crate::core::conversions::{f64_to_money_with_currency_str, i64_to_datetime};
use crate::history::wire::QuoteBlock;
use paft::market::responses::history::Candle;

use super::adjust::price_factor_for_row;

pub fn assemble_candles(
    ts: &[i64],
    q: &QuoteBlock,
    adj: &[Option<f64>],
    auto_adjust: bool,
    keepna: bool,
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

        let raw_close_val = close.unwrap_or(f64::NAN);

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

            if let (Some(ov), Some(hv), Some(lv), Some(cv)) = (open, high, low, close) {
                out.push(Candle {
                    ts: i64_to_datetime(t),
                    open: f64_to_money_with_currency_str(ov, currency),
                    high: f64_to_money_with_currency_str(hv, currency),
                    low: f64_to_money_with_currency_str(lv, currency),
                    close: f64_to_money_with_currency_str(cv, currency),
                    close_unadj: if raw_close_val.is_finite() {
                        Some(f64_to_money_with_currency_str(raw_close_val, currency))
                    } else {
                        None
                    },
                    volume: volume0,
                    provider: (),
                });
            } else if keepna {
                out.push(Candle {
                    ts: i64_to_datetime(t),
                    open: f64_to_money_with_currency_str(open.unwrap_or(f64::NAN), currency),
                    high: f64_to_money_with_currency_str(high.unwrap_or(f64::NAN), currency),
                    low: f64_to_money_with_currency_str(low.unwrap_or(f64::NAN), currency),
                    close: f64_to_money_with_currency_str(close.unwrap_or(f64::NAN), currency),
                    close_unadj: if raw_close_val.is_finite() {
                        Some(f64_to_money_with_currency_str(raw_close_val, currency))
                    } else {
                        None
                    },
                    volume: volume0,
                    provider: (),
                });
            }
        } else if let (Some(ov), Some(hv), Some(lv), Some(cv)) = (open, high, low, close) {
            out.push(Candle {
                ts: i64_to_datetime(t),
                open: f64_to_money_with_currency_str(ov, currency),
                high: f64_to_money_with_currency_str(hv, currency),
                low: f64_to_money_with_currency_str(lv, currency),
                close: f64_to_money_with_currency_str(cv, currency),
                close_unadj: if raw_close_val.is_finite() {
                    Some(f64_to_money_with_currency_str(raw_close_val, currency))
                } else {
                    None
                },
                volume: volume0,
                provider: (),
            });
        } else if keepna {
            out.push(Candle {
                ts: i64_to_datetime(t),
                open: f64_to_money_with_currency_str(open.unwrap_or(f64::NAN), currency),
                high: f64_to_money_with_currency_str(high.unwrap_or(f64::NAN), currency),
                low: f64_to_money_with_currency_str(low.unwrap_or(f64::NAN), currency),
                close: f64_to_money_with_currency_str(close.unwrap_or(f64::NAN), currency),
                close_unadj: if raw_close_val.is_finite() {
                    Some(f64_to_money_with_currency_str(raw_close_val, currency))
                } else {
                    None
                },
                volume: volume0,
                provider: (),
            });
        }
    }

    out
}
