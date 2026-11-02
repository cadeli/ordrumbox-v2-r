//! Weighted sampling without replacement (Efraimidis–Spirakis): each item
//! draws a key `u^(1/w)`, the largest keys win. An item of weight `w` is
//! taken before one of weight `w/2` as often as the weights say, and exactly
//! `count` items come out whenever that many have a weight.

use ordrumbox_core::Rng;

/// `count` indices of `weights` (fewer when fewer have a weight above 0), in
/// increasing order.
pub fn take(weights: &[f64], count: usize, rng: &mut Rng) -> Vec<usize> {
    let mut keyed: Vec<(f64, usize)> = weights
        .iter()
        .enumerate()
        .filter(|(_, w)| **w > 0.0 && w.is_finite())
        .map(|(i, &w)| (rng.next_f64().max(f64::MIN_POSITIVE).powf(1.0 / w), i))
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut chosen: Vec<usize> = keyed.into_iter().take(count).map(|(_, i)| i).collect();
    chosen.sort_unstable();
    chosen
}

/// One index of `weights`, with chances in proportion to them; `None` when
/// none has a weight.
pub fn pick(weights: &[f64], rng: &mut Rng) -> Option<usize> {
    let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return None;
    }
    let mut at = rng.next_f64() * total;
    let mut last = None;
    for (i, &w) in weights.iter().enumerate().filter(|(_, w)| **w > 0.0) {
        if at < w {
            return Some(i);
        }
        at -= w;
        last = Some(i);
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_count_items_with_a_weight() {
        let mut rng = Rng::new(1);
        let weights = [0.0, 0.5, 1.0, 0.1, 0.0, 0.9];
        for count in 0..=4 {
            let taken = take(&weights, count, &mut rng);
            assert_eq!(taken.len(), count);
            assert!(taken.iter().all(|&i| weights[i] > 0.0));
        }
        assert_eq!(take(&weights, 9, &mut rng), [1, 2, 3, 5]);
    }

    #[test]
    fn heavier_items_are_taken_more_often() {
        let mut rng = Rng::new(2);
        let weights = [1.0, 0.25];
        let first = (0..2000).filter(|_| take(&weights, 1, &mut rng) == [0]).count();
        // P(first) = 1 / (1 + 0.25) = 0.8.
        assert!((1500..1700).contains(&first), "{first}");
        let picked = (0..2000).filter(|_| pick(&weights, &mut rng) == Some(0)).count();
        assert!((1500..1700).contains(&picked), "{picked}");
        assert_eq!(pick(&[0.0, 0.0], &mut rng), None);
    }
}
