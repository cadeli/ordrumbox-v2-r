//! The metrical weight of a step: how strong its place in the measure is.

/// Weight (0–1) of step `step` of a measure of `beat_count` beats of
/// `steps_per_beat` steps: the downbeat 1, the middle of the measure 0.85,
/// the other beats 0.7, then the subdivisions by how finely they cut the beat
/// (the half 0.5, a third 0.45, a quarter 0.3…).
pub fn metric_weight(step: usize, steps_per_beat: usize, beat_count: usize) -> f64 {
    let spb = steps_per_beat.max(1);
    let (beat, within) = (step / spb, step % spb);
    if within == 0 {
        return if beat == 0 {
            1.0
        } else if beat_count.is_multiple_of(2) && beat * 2 == beat_count {
            0.85
        } else {
            0.7
        };
    }
    match spb / gcd(within, spb) {
        2 => 0.5,
        3 => 0.45,
        4 => 0.3,
        6 => 0.25,
        8 => 0.15,
        _ => 0.2,
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The weights of every step of a measure.
#[cfg(test)]
pub fn measure_weights(steps_per_beat: usize, beat_count: usize) -> Vec<f64> {
    (0..steps_per_beat * beat_count).map(|s| metric_weight(s, steps_per_beat, beat_count)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_downbeat_leads_then_the_beats_then_the_subdivisions() {
        let w = measure_weights(4, 4);
        assert_eq!(w[0], 1.0);
        assert_eq!(w[8], 0.85);
        assert_eq!((w[4], w[12]), (0.7, 0.7));
        assert_eq!((w[2], w[1], w[3]), (0.5, 0.3, 0.3));
        let ternary = measure_weights(3, 4);
        assert_eq!((ternary[1], ternary[2]), (0.45, 0.45));
    }
}
