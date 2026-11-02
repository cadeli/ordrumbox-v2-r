//! Measures of a line: how syncopated it is, how typical of its style.

/// Syncopation of the onsets `onsets` (steps of a cycle of `metric.len()`
/// steps, increasing), after Longuet-Higgins and Lee: an onset followed by a
/// rest on a stronger step than itself is syncopated by the difference. The
/// mean over the onsets, 0 for none.
pub fn syncopation(onsets: &[usize], metric: &[f64]) -> f64 {
    let n = metric.len();
    if onsets.is_empty() || n == 0 {
        return 0.0;
    }
    let mut total = 0.0;
    for (i, &onset) in onsets.iter().enumerate() {
        let next = if i + 1 < onsets.len() { onsets[i + 1] } else { onsets[0] + n };
        let strongest_rest = (onset + 1..next).map(|s| metric[s % n]).fold(0.0, f64::max);
        total += (strongest_rest - metric[onset % n]).max(0.0);
    }
    total / onsets.len() as f64
}

/// Mean log weight of the onsets: how typical of the weights a line is.
pub fn typicality(onsets: &[usize], weights: &[f64]) -> f64 {
    if onsets.is_empty() {
        return 0.0;
    }
    onsets.iter().map(|&s| weights.get(s).copied().unwrap_or(0.0).max(1e-6).ln()).sum::<f64>() / onsets.len() as f64
}

/// The candidate whose syncopation sits at `target` (0–1) of the range the
/// candidates span; between equals, the most typical.
pub fn closest(syncopations: &[f64], typicalities: &[f64], target: f64) -> usize {
    let low = syncopations.iter().copied().fold(f64::INFINITY, f64::min);
    let high = syncopations.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let goal = low + target.clamp(0.0, 1.0) * (high - low);
    let mut best = 0;
    for i in 1..syncopations.len() {
        let (d, d_best) = ((syncopations[i] - goal).abs(), (syncopations[best] - goal).abs());
        if d < d_best - 1e-9 || ((d - d_best).abs() <= 1e-9 && typicalities[i] > typicalities[best]) {
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::meter::measure_weights;

    #[test]
    fn on_the_beat_is_not_syncopated_off_it_is() {
        let metric = measure_weights(4, 4);
        assert_eq!(syncopation(&[0, 4, 8, 12], &metric), 0.0);
        // An onset on the last 16th before the beat, the beat itself silent.
        assert!(syncopation(&[0, 3, 7, 11], &metric) > 0.3);
        assert_eq!(syncopation(&[], &metric), 0.0);
    }

    #[test]
    fn the_closest_candidate_to_the_target() {
        let s = [0.1, 0.5, 0.3, 0.3];
        let t = [0.0, 0.0, -1.0, -0.5];
        assert_eq!(closest(&s, &t, 0.0), 0);
        assert_eq!(closest(&s, &t, 1.0), 1);
        assert_eq!(closest(&s, &t, 0.5), 3);
    }
}
