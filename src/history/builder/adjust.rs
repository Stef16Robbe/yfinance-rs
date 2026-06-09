use super::actions::SplitRatio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentBasis {
    ProviderAdjusted,
    SplitAdjusted,
}

pub fn cumulative_split_after(ts: &[i64], split_events: &[(i64, SplitRatio)]) -> Vec<f64> {
    let mut out = vec![1.0; ts.len()];
    if split_events.is_empty() || ts.is_empty() {
        return out;
    }

    let mut sp_idx = split_events.len();
    let mut running: f64 = 1.0;

    for i in (0..ts.len()).rev() {
        while sp_idx > 0 && split_events[sp_idx - 1].0 > ts[i] {
            sp_idx -= 1;
            running *= split_events[sp_idx].1.as_f64();
        }
        out[i] = running;
    }
    out
}

pub fn price_factor_for_row(
    i: usize,
    basis: AdjustmentBasis,
    adjclose_i: Option<f64>,
    close_i: Option<f64>,
    cum_split_after: &[f64],
) -> f64 {
    match basis {
        AdjustmentBasis::ProviderAdjusted => provider_adjustment_factor(adjclose_i, close_i)
            .expect("provider adjustment basis requires every emitted row to have usable adjclose"),
        AdjustmentBasis::SplitAdjusted => split_adjustment_factor(i, cum_split_after),
    }
}

pub fn provider_adjustment_factor(adjclose_i: Option<f64>, close_i: Option<f64>) -> Option<f64> {
    match (adjclose_i, close_i) {
        (Some(adj), Some(close)) if close != 0.0 => Some(adj / close),
        _ => None,
    }
}

fn split_adjustment_factor(i: usize, cum_split_after: &[f64]) -> f64 {
    1.0 / cum_split_after[i].max(1e-12)
}
