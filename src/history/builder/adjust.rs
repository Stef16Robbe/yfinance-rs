use super::actions::SplitRatio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentBasis {
    ProviderAdjusted,
    SplitAdjusted,
}

#[derive(Debug, Clone)]
pub enum AdjustmentPlan {
    ProviderAdjusted { row_factors: Vec<Option<f64>> },
    SplitAdjusted,
}

impl AdjustmentPlan {
    pub const fn basis(&self) -> AdjustmentBasis {
        match self {
            Self::ProviderAdjusted { .. } => AdjustmentBasis::ProviderAdjusted,
            Self::SplitAdjusted => AdjustmentBasis::SplitAdjusted,
        }
    }

    pub fn factor_for_row(&self, i: usize, cum_split_after: &[f64]) -> Option<f64> {
        match self {
            Self::ProviderAdjusted { row_factors } => row_factors.get(i).copied().flatten(),
            Self::SplitAdjusted => split_adjustment_factor(i, cum_split_after),
        }
    }
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

pub fn provider_adjustment_factor(adjclose_i: Option<f64>, close_i: Option<f64>) -> Option<f64> {
    match (adjclose_i, close_i) {
        (Some(adj), Some(close)) if close != 0.0 => Some(adj / close),
        _ => None,
    }
}

fn split_adjustment_factor(i: usize, cum_split_after: &[f64]) -> Option<f64> {
    cum_split_after.get(i).map(|factor| 1.0 / factor.max(1e-12))
}
