//! The second filter as a block of a note runs it: its coefficients while
//! nothing moves them, its routing, and how the two filters are mixed in parallel.

use super::clamp;
use super::filter::cutoff_coefficient;
use super::patch::{Filter2, Filter2Routing};
use crate::svf::tap_weights;

/// The second filter of a block: its taps, its cutoff, the coefficient and
/// damping while nothing moves it, and in parallel the gains of the two filters.
#[derive(Debug, Clone, Copy)]
pub(super) struct Filter2Stage {
    pub(super) weights: [f64; 3],
    pub(super) freq: f64,
    pub(super) g: f64,
    pub(super) k: f64,
    pub(super) routing: Filter2Routing,
    pub(super) balance: f64,
    /// The gains of the first and the second filter at [`Self::balance`].
    pub(super) gains: [f64; 2],
}

impl Filter2Stage {
    pub(super) fn new(filter: &Filter2, sample_rate: f64) -> Self {
        Self {
            weights: tap_weights(filter.mode),
            freq: filter.freq,
            g: cutoff_coefficient(filter.freq, sample_rate),
            k: 1.0 / clamp(filter.q, 0.1, 20.0),
            routing: filter.routing,
            balance: filter.balance,
            gains: balance_gains(filter.balance),
        }
    }
}

/// The gains of the first and the second filter in parallel at `balance`
/// (0–1, clamped): each at full level from the middle to its own side, then
/// down to silence at the other end. In the middle the two simply add up.
#[inline]
#[must_use]
pub fn balance_gains(balance: f64) -> [f64; 2] {
    let balance = clamp(balance, 0.0, 1.0);
    [(2.0 * (1.0 - balance)).min(1.0), (2.0 * balance).min(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_balance_fades_one_filter_out_on_each_side_of_the_middle() {
        assert_eq!(balance_gains(0.0), [1.0, 0.0]);
        assert_eq!(balance_gains(0.25), [1.0, 0.5]);
        assert_eq!(balance_gains(0.5), [1.0, 1.0]);
        assert_eq!(balance_gains(1.0), [0.0, 1.0]);
        assert_eq!(balance_gains(-3.0), [1.0, 0.0]);
        assert_eq!(balance_gains(9.0), [0.0, 1.0]);
    }
}
