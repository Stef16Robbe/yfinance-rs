use crate::core::conversions::{i64_to_datetime, quantity_from_u64};
use crate::core::{ProjectionContext, ProjectionIssue, YfError};
use crate::core::{conversions::decimal_from_f64, currency_resolver::ResolvedCurrencyUnit};
use crate::history::wire::QuoteBlock;
use paft::market::responses::history::{Candle, Ohlc};
use paft::money::PriceAmount;

use super::adjust::{AdjustmentBasis, price_factor_for_row, provider_adjustment_factor};

pub fn assemble_candles(
    ts: &[i64],
    q: &QuoteBlock,
    adj: &[Option<f64>],
    adjustment_basis: Option<AdjustmentBasis>,
    cum_split_after: &[f64],
    currency: &ResolvedCurrencyUnit,
    ctx: &mut ProjectionContext,
) -> Result<Vec<Candle>, YfError> {
    let mut out = Vec::with_capacity(candle_capacity_upper_bound(ts, q));

    for (i, &t) in ts.iter().enumerate() {
        let ts = match i64_to_datetime(t) {
            Ok(ts) => ts,
            Err(err) => {
                let key = t.to_string();
                ctx.dropped_item(
                    "candle",
                    Some(&key),
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
                let key = t.to_string();
                ctx.dropped_item("candle", Some(&key), reason)?;
                continue;
            }
        };
        let raw_close = close;

        if let Some(adjustment_basis) = adjustment_basis {
            let pf = price_factor_for_row(
                i,
                adjustment_basis,
                adj.get(i).and_then(|x| *x),
                Some(close),
                cum_split_after,
            );

            open *= pf;
            high *= pf;
            low *= pf;
            close *= pf;
        }

        let Some((open, high, low, close)) = candle_prices(open, high, low, close, currency) else {
            let key = t.to_string();
            ctx.dropped_item(
                "candle",
                Some(&key),
                ProjectionIssue::ConversionFailed {
                    target: "candle prices",
                },
            )?;
            continue;
        };
        let close_unadj = currency.price_amount_from_f64(raw_close);
        if close_unadj.is_none() {
            let key = t.to_string();
            ctx.omitted_present_field(
                "quote.close_unadj",
                Some(&key),
                ProjectionIssue::ConversionFailed {
                    target: "unadjusted close price",
                },
            )?;
        }
        out.push(Candle {
            ts,
            currency: currency.currency().clone(),
            ohlc: Ohlc::new(open, high, low, close),
            close_unadj,
            volume: volume0.and_then(quantity_from_u64),
            provider: (),
        });
    }

    Ok(out)
}

pub fn adjustment_basis_for_series(
    q: &QuoteBlock,
    adj: &[Option<f64>],
    len: usize,
    ctx: &mut ProjectionContext,
) -> Result<AdjustmentBasis, YfError> {
    let mut emitted_rows = 0usize;
    let mut provider_adjusted_rows = 0usize;

    for i in 0..len {
        let open = q.open.get(i).and_then(|value| *value);
        let high = q.high.get(i).and_then(|value| *value);
        let low = q.low.get(i).and_then(|value| *value);
        let close = q.close.get(i).and_then(|value| *value);

        let Ok((_, _, _, close)) = raw_ohlc_values(open, high, low, close) else {
            continue;
        };

        emitted_rows += 1;
        if provider_adjustment_factor(adj.get(i).and_then(|value| *value), Some(close)).is_some() {
            provider_adjusted_rows += 1;
        }
    }

    if provider_adjusted_rows == 0 {
        return Ok(AdjustmentBasis::SplitAdjusted);
    }

    if provider_adjusted_rows == emitted_rows {
        return Ok(AdjustmentBasis::ProviderAdjusted);
    }

    ctx.repaired_data(
        "candle_adjustment",
        None,
        "ignored sparse chart.indicators.adjclose and used split-only adjustment for all candles",
    )?;

    Ok(AdjustmentBasis::SplitAdjusted)
}

fn candle_capacity_upper_bound(ts: &[i64], q: &QuoteBlock) -> usize {
    [q.open.len(), q.high.len(), q.low.len(), q.close.len()]
        .into_iter()
        .fold(ts.len(), usize::min)
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
) -> Option<(PriceAmount, PriceAmount, PriceAmount, PriceAmount)> {
    Some((
        currency.price_amount_from_f64(open)?,
        currency.price_amount_from_f64(high)?,
        currency.price_amount_from_f64(low)?,
        currency.price_amount_from_f64(close)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candle_capacity_upper_bound_uses_shortest_required_array() {
        let quote = QuoteBlock {
            open: vec![Some(1.0), Some(2.0), Some(3.0)],
            high: vec![Some(1.0), Some(2.0)],
            low: vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0)],
            close: vec![Some(1.0)],
            volume: vec![Some(1), Some(2), Some(3), Some(4), Some(5)],
        };

        assert_eq!(candle_capacity_upper_bound(&[1, 2, 3, 4, 5], &quote), 1);
    }
}
