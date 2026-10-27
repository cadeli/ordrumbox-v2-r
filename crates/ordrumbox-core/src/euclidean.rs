//! Euclidean rhythms (`src/core/euclidean.js`), shared by the engine and the
//! grid views so every view and the audio place the onsets on the same steps.

/// Bjorklund's algorithm: `pulses` onsets spread as evenly as possible over
/// `steps` positions. Always `steps` long, `min(pulses, steps)` onsets, onset 0
/// set, cyclic gaps differing by at most 1.
pub fn bjorklund(pulses: i32, steps: i32) -> Vec<bool> {
    let n = steps.max(0) as usize;
    let k = pulses.clamp(0, steps.max(0)) as usize;
    if k == 0 || k == n {
        return vec![k == n && n > 0; n];
    }
    let mut a: Vec<Vec<bool>> = vec![vec![true]; k];
    let mut b: Vec<Vec<bool>> = vec![vec![false]; n - k];
    while b.len() > 1 {
        let count = a.len().min(b.len());
        let rest_a = a.split_off(count);
        let rest_b = b.split_off(count);
        for (seq, tail) in a.iter_mut().zip(b) {
            seq.extend(tail);
        }
        b = if rest_a.is_empty() { rest_b } else { rest_a };
    }
    a.into_iter().chain(b).flatten().collect()
}

/// Steps of the euclidean onsets added after a base note at `base`.
///
/// `pulses` counts the base note as the first onset, over a span of `span`
/// steps (up to the next note): `pulses - 1` positions, or every step of the
/// span when `pulses >= span`. `rotation` shifts the added onsets, never onto
/// the base step. Ascending, base excluded.
pub fn fill_positions(base: i64, span: i64, pulses: i32, rotation: i32) -> Vec<i64> {
    if span <= 1 || pulses <= 1 {
        return Vec::new();
    }
    let span_i32 = span.min(i64::from(i32::MAX)) as i32;
    let mut pattern = bjorklund(pulses, span_i32);
    // The base note always anchors the rhythm as onset #1.
    if !pattern[0] {
        pattern[0] = true;
        if let Some(last) = pattern.iter().rposition(|&on| on).filter(|&i| i > 0) {
            pattern[last] = false;
        }
    }
    let slots = span - 1;
    let shift = i64::from(rotation).rem_euclid(slots);
    (1..span).filter(|&i| pattern[(1 + (i - 1 - shift).rem_euclid(slots)) as usize]).map(|i| base + i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn onsets(pattern: &[bool]) -> String {
        pattern.iter().map(|&on| if on { 'x' } else { '.' }).collect()
    }

    #[test]
    fn classic_rhythms() {
        assert_eq!(onsets(&bjorklund(3, 8)), "x..x..x.");
        assert_eq!(onsets(&bjorklund(5, 8)), "x.xx.xx.");
        assert_eq!(onsets(&bjorklund(4, 12)), "x..x..x..x..");
        assert_eq!(onsets(&bjorklund(0, 4)), "....");
        assert_eq!(onsets(&bjorklund(9, 4)), "xxxx");
        assert!(bjorklund(2, 0).is_empty());
    }

    #[test]
    fn fills_count_the_base_note() {
        assert_eq!(fill_positions(0, 8, 3, 0), [3, 6]);
        assert_eq!(fill_positions(10, 4, 9, 0), [11, 12, 13]); // full roll, no duplicate
        assert_eq!(fill_positions(0, 8, 1, 0), [] as [i64; 0]);
        assert_eq!(fill_positions(0, 1, 4, 0), [] as [i64; 0]);
    }

    #[test]
    fn rotation_never_lands_on_the_base() {
        assert_eq!(fill_positions(0, 8, 3, 1), [4, 7]);
        assert_eq!(fill_positions(0, 8, 3, -1), [2, 5]);
        assert_eq!(fill_positions(0, 8, 3, 7), fill_positions(0, 8, 3, 0));
    }
}
