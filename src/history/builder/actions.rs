use crate::core::conversions::{i64_to_datetime, price_from_f64};
use crate::history::wire::Events;
use paft::market::action::Action;
use paft::money::Currency;
use std::num::NonZeroU32;

const SPLIT_SCALE: f64 = 1_000_000.0;

pub fn extract_actions(
    events: Option<&Events>,
    currency: &Currency,
) -> (Vec<Action>, Vec<(i64, f64)>) {
    let mut out: Vec<Action> = Vec::new();
    let mut split_events: Vec<(i64, f64)> = Vec::new();

    let Some(ev) = events else {
        return (out, split_events);
    };

    if let Some(divs) = ev.dividends.as_ref() {
        for (k, d) in divs {
            let Some(ts) = event_timestamp(k, d.date) else {
                continue;
            };
            let Ok(dt) = i64_to_datetime(ts) else {
                continue;
            };
            if let Some(amount) = d
                .amount
                .and_then(|amount| price_from_f64(amount, currency.clone()))
            {
                out.push(Action::Dividend { ts: dt, amount });
            }
        }
    }

    if let Some(gains) = ev.capital_gains.as_ref() {
        for (k, g) in gains {
            let Some(ts) = event_timestamp(k, g.date) else {
                continue;
            };
            let Ok(dt) = i64_to_datetime(ts) else {
                continue;
            };
            if let Some(gain) = g
                .amount
                .and_then(|gain| price_from_f64(gain, currency.clone()))
            {
                out.push(Action::CapitalGain { ts: dt, gain });
            }
        }
    }

    if let Some(splits) = ev.splits.as_ref() {
        for (k, s) in splits {
            let Some(ts) = event_timestamp(k, s.date) else {
                continue;
            };
            let Ok(dt) = i64_to_datetime(ts) else {
                continue;
            };
            let Some((num, den)) = normalize_split_event(s) else {
                continue;
            };

            out.push(Action::Split {
                ts: dt,
                numerator: num,
                denominator: den,
            });

            let ratio = f64::from(num.get()) / f64::from(den.get());
            split_events.push((ts, ratio));
        }
    }

    out.sort_by_key(|a| match a {
        Action::Dividend { ts, .. } | Action::Split { ts, .. } | Action::CapitalGain { ts, .. } => {
            ts.timestamp()
        }
        _ => i64::MAX,
    });
    split_events.sort_by_key(|(ts, _)| *ts);

    (out, split_events)
}

fn event_timestamp(key: &str, date: Option<i64>) -> Option<i64> {
    key.parse::<i64>().ok().or(date)
}

fn normalize_split_event(
    split: &crate::history::wire::SplitEvent,
) -> Option<(NonZeroU32, NonZeroU32)> {
    if let (Some(numerator), Some(denominator)) = (split.numerator, split.denominator)
        && let Some(pair) = normalize_split_pair(numerator, denominator)
    {
        return Some(pair);
    }

    split.split_ratio.as_deref().and_then(normalize_split_ratio)
}

fn normalize_split_ratio(ratio: &str) -> Option<(NonZeroU32, NonZeroU32)> {
    let ratio = ratio.trim();
    for separator in ['/', ':'] {
        if let Some((numerator, denominator)) = ratio.split_once(separator) {
            return normalize_split_pair(
                parse_split_component(numerator)?,
                parse_split_component(denominator)?,
            );
        }
    }

    normalize_split_pair(parse_split_component(ratio)?, 1.0)
}

fn parse_split_component(value: &str) -> Option<f64> {
    let value = value.trim().parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn normalize_split_pair(numerator: f64, denominator: f64) -> Option<(NonZeroU32, NonZeroU32)> {
    if !numerator.is_finite() || !denominator.is_finite() || numerator <= 0.0 || denominator <= 0.0
    {
        return None;
    }

    let numerator = scaled_split_component(numerator)?;
    let denominator = scaled_split_component(denominator)?;
    if numerator == 0 || denominator == 0 {
        return None;
    }

    let gcd = gcd(numerator, denominator);
    let numerator = numerator / gcd;
    let denominator = denominator / gcd;

    Some((
        NonZeroU32::new(u32::try_from(numerator).ok()?)?,
        NonZeroU32::new(u32::try_from(denominator).ok()?)?,
    ))
}

fn scaled_split_component(value: f64) -> Option<u128> {
    let scaled = (value * SPLIT_SCALE).round();
    let max_scaled = f64::from(u32::MAX) * SPLIT_SCALE;
    if !scaled.is_finite() || scaled < 0.0 || scaled > max_scaled {
        return None;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(scaled as u128)
}

const fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}
