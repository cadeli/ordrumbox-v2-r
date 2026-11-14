//! Arpeggios (`normalizeArp()` in pattern_engine.js): what a note's `arp`
//! field means, as a sequence of semitone offsets.

use crate::json::Value;
use crate::rng::Rng;

/// Intervals used by the mode keywords (`"up"`, `"random"`…): a major scale.
const MAJOR_SCALE: [f64; 8] = [0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0, 12.0];

/// The most octaves an arpeggio spans.
pub const MAX_OCTAVES: usize = 4;

/// The semitone offsets an arpeggio steps through, or `None` when `arp` does
/// not describe one. Accepted forms:
/// - `[0, 4, 7]`: intervals, ascending;
/// - `"0,4,7"`: the same as a string;
/// - `"up"`, `"down"`, `"updown"`, `"random"`: the major scale in that order;
/// - `{"intervals": [0, 3, 7], "direction": "updown", "octaves": 2}`.
///
/// `octaves` (1–[`MAX_OCTAVES`], default 1) repeats the intervals that many
/// octaves up (`0 4 7 12 16 19` for two), before the direction is applied.
///
/// Intervals are numbers or numeric strings (`"4"` is 4), anything else dropped,
/// and a root (0) is added when missing. `updown` goes up then back down
/// without repeating the ends (`0 4 7 4`). `random` shuffles on every call, so
/// it draws from `rng`; every other form draws nothing.
pub fn sequence(arp: &Value, rng: &mut Rng) -> Option<Vec<f64>> {
    let (intervals, mode, octaves): (Vec<f64>, String, usize) = match arp {
        Value::Array(items) => (items.iter().filter_map(Value::to_number).collect(), "up".into(), 1),
        Value::String(text) => {
            let text = text.trim().to_lowercase();
            match text.as_str() {
                "up" | "down" | "updown" | "random" => (MAJOR_SCALE.to_vec(), text, 1),
                _ if !text.contains(|c: char| c.is_ascii_digit()) => return None,
                _ => (text.split(',').filter_map(|part| Value::from(part).to_number()).collect(), "up".into(), 1),
            }
        }
        Value::Object(object) => {
            let intervals = object.get("intervals").and_then(Value::as_array).unwrap_or_default();
            let mode = object.get("direction").and_then(Value::to_text).unwrap_or_else(|| "up".to_owned());
            let octaves = object.get("octaves").and_then(Value::to_number).map_or(1, octave_count);
            (intervals.iter().filter_map(Value::to_number).collect(), mode.to_lowercase(), octaves)
        }
        _ => return None,
    };

    let mut steps: Vec<f64> = intervals.into_iter().filter(|n| n.is_finite()).collect();
    if steps.is_empty() {
        return None;
    }
    if !steps.contains(&0.0) {
        steps.insert(0, 0.0);
    }
    add_octaves(&mut steps, octaves);
    match mode.as_str() {
        "random" => rng.shuffle(&mut steps),
        "down" => steps.sort_by(|a, b| b.total_cmp(a)),
        "updown" => {
            steps.sort_by(f64::total_cmp);
            let inner = steps.get(1..steps.len() - 1).unwrap_or_default();
            let back: Vec<f64> = inner.iter().rev().copied().collect();
            steps.extend(back);
        }
        _ => steps.sort_by(f64::total_cmp),
    }
    Some(steps)
}

/// An `octaves` value as a count, 1–[`MAX_OCTAVES`].
fn octave_count(octaves: f64) -> usize {
    octaves.round().clamp(1.0, MAX_OCTAVES as f64) as usize
}

/// `steps` followed by themselves 12 semitones higher, `octaves - 1` times
/// (a note already there, like the 12 of a scale, is not repeated).
fn add_octaves(steps: &mut Vec<f64>, octaves: usize) {
    let base = steps.clone();
    for octave in 1..octaves {
        let shift = 12.0 * octave as f64;
        for step in &base {
            let shifted = step + shift;
            if !steps.contains(&shifted) {
                steps.push(shifted);
            }
        }
    }
}

/// Notes an arpeggio plays: the note's `retriggerCount`, 1–16.
pub fn note_count(retrigger_count: i32) -> usize {
    retrigger_count.clamp(1, 16) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn seq(text: &str) -> Option<Vec<f64>> {
        sequence(&json::parse(text).unwrap(), &mut Rng::new(1))
    }

    #[test]
    fn forms() {
        assert_eq!(seq("[7, 4]"), Some(vec![0.0, 4.0, 7.0]));
        assert_eq!(seq(r#""0, 5,9""#), Some(vec![0.0, 5.0, 9.0]));
        assert_eq!(seq(r#""down""#), Some(vec![12.0, 11.0, 9.0, 7.0, 5.0, 4.0, 2.0, 0.0]));
        assert_eq!(seq(r#"{"intervals":[0,4,7,12],"direction":"updown"}"#), Some(vec![0.0, 4.0, 7.0, 12.0, 7.0, 4.0]));
        assert_eq!(seq(r#"{"intervals":["4","7","x"]}"#), Some(vec![0.0, 4.0, 7.0]));
        assert_eq!(seq(r#"{"intervals":[3],"direction":"UP"}"#), Some(vec![0.0, 3.0]));
    }

    #[test]
    fn octaves_repeat_the_intervals_higher() {
        assert_eq!(seq(r#"{"intervals":[0,4,7],"octaves":2}"#), Some(vec![0.0, 4.0, 7.0, 12.0, 16.0, 19.0]));
        // The octave of a scale is not played twice.
        assert_eq!(seq(r#"{"intervals":[0,7,12],"octaves":2}"#), Some(vec![0.0, 7.0, 12.0, 19.0, 24.0]));
        assert_eq!(
            seq(r#"{"intervals":[0,4,7],"direction":"updown","octaves":2}"#),
            Some(vec![0.0, 4.0, 7.0, 12.0, 16.0, 19.0, 16.0, 12.0, 7.0, 4.0])
        );
        assert_eq!(seq(r#"{"intervals":[0,4],"octaves":9}"#).map(|s| s.len()), Some(2 * MAX_OCTAVES));
        assert_eq!(seq(r#"{"intervals":[0,4],"octaves":0}"#), Some(vec![0.0, 4.0]));
    }

    #[test]
    fn not_an_arpeggio() {
        for text in ["null", "5", "true", "[]", r#""nothing""#, r#"{"direction":"up"}"#, r#"["x"]"#] {
            assert_eq!(seq(text), None, "{text}");
        }
    }

    #[test]
    fn random_is_a_permutation_and_reproducible() {
        let arp = json::parse(r#""random""#).unwrap();
        let a = sequence(&arp, &mut Rng::new(5)).unwrap();
        let mut sorted = a.clone();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(sorted, MAJOR_SCALE);
        assert_eq!(sequence(&arp, &mut Rng::new(5)).unwrap(), a);
    }

    #[test]
    fn counts() {
        assert_eq!(note_count(0), 1);
        assert_eq!(note_count(20), 16);
    }
}
