use crate::core::{
    ProjectionContext, ProjectionIssue, YfError, conversions::i64_to_date,
    currency_resolver::ResolvedCurrencyUnit,
};
use crate::history::wire::Events;
use paft::market::action::Action;
use std::num::NonZeroU32;

const SPLIT_SCALE: f64 = 1_000_000.0;

pub type ExtractedActions = (Vec<Action>, Vec<(i64, f64)>);

#[allow(clippy::too_many_lines)]
pub fn extract_actions(
    events: Option<&Events>,
    default_currency: Option<&ResolvedCurrencyUnit>,
    ctx: &mut ProjectionContext,
) -> Result<ExtractedActions, YfError> {
    let mut out: Vec<Action> = Vec::new();
    let mut split_events: Vec<(i64, f64)> = Vec::new();

    let Some(ev) = events else {
        return Ok((out, split_events));
    };

    if let Some(divs) = ev.dividends.as_ref() {
        for (k, d) in divs {
            let Some(ts) = event_timestamp(k, d.date) else {
                ctx.dropped_item(
                    "dividend",
                    Some(k.clone()),
                    ProjectionIssue::MissingRequiredField { field: "date" },
                )?;
                continue;
            };
            let date = match i64_to_date(ts) {
                Ok(date) => date,
                Err(err) => {
                    ctx.dropped_item(
                        "dividend",
                        Some(k.clone()),
                        ProjectionIssue::InvalidField {
                            field: "date",
                            details: err.to_string(),
                        },
                    )?;
                    continue;
                }
            };
            let Some(amount) = d.amount else {
                ctx.dropped_item(
                    "dividend",
                    Some(k.clone()),
                    ProjectionIssue::MissingRequiredField { field: "amount" },
                )?;
                continue;
            };
            let currency = match event_currency(d.currency.as_deref(), default_currency) {
                Ok(Some(currency)) => currency,
                Ok(None) => {
                    ctx.dropped_item(
                        "dividend",
                        Some(k.clone()),
                        ProjectionIssue::CurrencyUnresolved,
                    )?;
                    continue;
                }
                Err(reason) => {
                    ctx.dropped_item("dividend", Some(k.clone()), reason)?;
                    continue;
                }
            };
            let Some(amount) = currency.price_from_f64(amount) else {
                ctx.dropped_item(
                    "dividend",
                    Some(k.clone()),
                    ProjectionIssue::ConversionFailed {
                        target: "dividend amount",
                    },
                )?;
                continue;
            };
            out.push(Action::Dividend { date, amount });
        }
    }

    if let Some(gains) = ev.capital_gains.as_ref() {
        for (k, g) in gains {
            let Some(ts) = event_timestamp(k, g.date) else {
                ctx.dropped_item(
                    "capital_gain",
                    Some(k.clone()),
                    ProjectionIssue::MissingRequiredField { field: "date" },
                )?;
                continue;
            };
            let date = match i64_to_date(ts) {
                Ok(date) => date,
                Err(err) => {
                    ctx.dropped_item(
                        "capital_gain",
                        Some(k.clone()),
                        ProjectionIssue::InvalidField {
                            field: "date",
                            details: err.to_string(),
                        },
                    )?;
                    continue;
                }
            };
            let Some(amount) = g.amount else {
                ctx.dropped_item(
                    "capital_gain",
                    Some(k.clone()),
                    ProjectionIssue::MissingRequiredField { field: "amount" },
                )?;
                continue;
            };
            let currency = match event_currency(g.currency.as_deref(), default_currency) {
                Ok(Some(currency)) => currency,
                Ok(None) => {
                    ctx.dropped_item(
                        "capital_gain",
                        Some(k.clone()),
                        ProjectionIssue::CurrencyUnresolved,
                    )?;
                    continue;
                }
                Err(reason) => {
                    ctx.dropped_item("capital_gain", Some(k.clone()), reason)?;
                    continue;
                }
            };
            let Some(gain) = currency.price_from_f64(amount) else {
                ctx.dropped_item(
                    "capital_gain",
                    Some(k.clone()),
                    ProjectionIssue::ConversionFailed {
                        target: "capital gain amount",
                    },
                )?;
                continue;
            };
            out.push(Action::CapitalGain { date, gain });
        }
    }

    if let Some(splits) = ev.splits.as_ref() {
        for (k, s) in splits {
            let Some(ts) = event_timestamp(k, s.date) else {
                ctx.dropped_item(
                    "split",
                    Some(k.clone()),
                    ProjectionIssue::MissingRequiredField { field: "date" },
                )?;
                continue;
            };
            let date = match i64_to_date(ts) {
                Ok(date) => date,
                Err(err) => {
                    ctx.dropped_item(
                        "split",
                        Some(k.clone()),
                        ProjectionIssue::InvalidField {
                            field: "date",
                            details: err.to_string(),
                        },
                    )?;
                    continue;
                }
            };
            let Some((num, den)) = normalize_split_event(s) else {
                ctx.dropped_item(
                    "split",
                    Some(k.clone()),
                    ProjectionIssue::InvalidField {
                        field: "splitRatio",
                        details: "missing, zero, negative, non-finite, or too large".into(),
                    },
                )?;
                continue;
            };

            out.push(Action::Split {
                date,
                numerator: num,
                denominator: den,
            });

            let ratio = f64::from(num.get()) / f64::from(den.get());
            split_events.push((ts, ratio));
        }
    }

    out.sort_by_key(action_sort_key);
    split_events.sort_by_key(|(ts, _)| *ts);

    Ok((out, split_events))
}

const fn action_sort_key(action: &Action) -> (bool, chrono::NaiveDate) {
    match action {
        Action::Dividend { date, .. }
        | Action::Split { date, .. }
        | Action::CapitalGain { date, .. } => (false, *date),
        _ => (true, chrono::NaiveDate::MAX),
    }
}

fn event_currency(
    code: Option<&str>,
    default_currency: Option<&ResolvedCurrencyUnit>,
) -> Result<Option<ResolvedCurrencyUnit>, ProjectionIssue> {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(default_currency.cloned());
    };

    ResolvedCurrencyUnit::from_code(code)
        .map(Some)
        .ok_or_else(|| ProjectionIssue::InvalidCurrency {
            code: code.to_string(),
        })
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
