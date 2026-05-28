use crate::core::conversions::i64_to_datetime;
use crate::core::{ProjectionContext, ProjectionIssue, YfError};
use crate::core::{conversions::decimal_from_f64, currency_resolver::ResolvedCurrencyUnit};
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
    currency: &ResolvedCurrencyUnit,
    ctx: &mut ProjectionContext,
) -> Result<Vec<Candle>, YfError> {
    let mut out = Vec::new();

    for (i, &t) in ts.iter().enumerate() {
        let ts = match i64_to_datetime(t) {
            Ok(ts) => ts,
            Err(err) => {
                ctx.dropped_item(
                    "candle",
                    Some(t.to_string()),
                    ProjectionIssue::InvalidField {
                        field: "timestamp",
                        details: err.to_string(),
                    },
                )?;
                continue;
            }
        };
        let getter_f64 = |v: &Vec<Option<f64>>| v.get(i).and_then(|x| *x);
        let open = getter_f64(&q.open);
        let high = getter_f64(&q.high);
        let low = getter_f64(&q.low);
        let close = getter_f64(&q.close);
        let volume0 = q.volume.get(i).and_then(|x| *x);

        let (mut open, mut high, mut low, mut close) = match raw_ohlc_values(open, high, low, close)
        {
            Ok(values) => values,
            Err(reason) => {
                ctx.dropped_item("candle", Some(t.to_string()), reason)?;
                continue;
            }
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

        let Some((open, high, low, close)) = candle_prices(open, high, low, close, currency) else {
            ctx.dropped_item(
                "candle",
                Some(t.to_string()),
                ProjectionIssue::ConversionFailed {
                    target: "candle prices",
                },
            )?;
            continue;
        };
        let close_unadj = currency.price_from_f64(raw_close);
        if close_unadj.is_none() {
            ctx.omitted_present_field(
                "quote.close_unadj",
                Some(t.to_string()),
                ProjectionIssue::ConversionFailed {
                    target: "unadjusted close price",
                },
            )?;
        }
        out.push(Candle {
            ts,
            open,
            high,
            low,
            close,
            close_unadj,
            volume: volume0,
            provider: (),
        });
    }

    Ok(out)
}

fn raw_ohlc_values(
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
    close: Option<f64>,
) -> Result<(f64, f64, f64, f64), ProjectionIssue> {
    let mut missing = Vec::with_capacity(4);
    if open.is_none() {
        missing.push("open");
    }
    if high.is_none() {
        missing.push("high");
    }
    if low.is_none() {
        missing.push("low");
    }
    if close.is_none() {
        missing.push("close");
    }
    if !missing.is_empty() {
        return Err(ProjectionIssue::MissingRequiredFields { fields: missing });
    }

    Ok((
        valid_decimal_value("open", open.expect("checked above"))?,
        valid_decimal_value("high", high.expect("checked above"))?,
        valid_decimal_value("low", low.expect("checked above"))?,
        valid_decimal_value("close", close.expect("checked above"))?,
    ))
}

fn valid_decimal_value(field: &'static str, value: f64) -> Result<f64, ProjectionIssue> {
    decimal_from_f64(value)
        .map(|_| value)
        .ok_or_else(|| ProjectionIssue::InvalidField {
            field,
            details: format!("non-finite or not representable as Decimal: {value}"),
        })
}

fn candle_prices(
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    currency: &ResolvedCurrencyUnit,
) -> Option<(Price, Price, Price, Price)> {
    Some((
        currency.price_from_f64(open)?,
        currency.price_from_f64(high)?,
        currency.price_from_f64(low)?,
        currency.price_from_f64(close)?,
    ))
}
