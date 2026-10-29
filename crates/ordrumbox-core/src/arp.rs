//! Arpeggios (`normalizeArp()` in pattern_engine.js): what a note's `arp`
//! field means, as a sequence of semitone offsets.

use crate::js;
use crate::json::Value;
use crate::rng::Rng;

/// Intervals used by the mode keywords (`"up"`, `"random"`…): a major scale.
const MAJOR_SCALE: [f64; 8] = [0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0, 12.0];

/// The semitone offsets an arpeggio steps through, or `None` when `arp` does
/// not describe one. Accepted forms:
/// - `[0, 4, 7]`: intervals, ascending;
/// - `"0,4,7"`: the same as a string;
/// - `"up"`, `"down"`, `"updown"`, `"random"`: the major scale in that order;
/// - `{"intervals": [0, 3, 7], "direction": "updown"}`.
///
/// Intervals are coerced like JS `Number()` (`"4"` is 4), non-numbers dropped,
/// and a root (0) is added when missing. `updown` goes up then back down
/// without repeating the ends (`0 4 7 4`). `random` shuffles on every call, so
/// it draws from `rng`; every other form draws nothing.
pub fn sequence(arp: &Value, rng: &mut Rng) -> Option<Vec<f64>> {
    let (intervals, mode): (Vec<f64>, String) = match arp {
        Value::Array(items) => (items.iter().map(js::to_number).collect(), "up".into()),
        Value::String(text) => {
            let text = text.trim().to_lowercase();
            match text.as_str() {
                "up" | "down" | "updown" | "random" => (MAJOR_SCALE.to_vec(), text),
                _ if !text.contains(|c: char| c.is_ascii_digit()) => return None,
                _ => (text.split(',').map(|part| js::to_number(&Value::from(part.trim()))).collect(), "up".into()),
            }
        }
        Value::Object(object) => {
            let intervals = object.get("intervals").and_then(Value::as_array).unwrap_or_default();
            let mode = object.get("direction").filter(|m| !m.is_null()).map_or_else(|| "up".to_owned(), js::to_string);
            (intervals.iter().map(js::to_number).collect(), mode.to_lowercase())
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
