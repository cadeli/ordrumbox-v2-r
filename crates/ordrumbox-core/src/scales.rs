//! Musical scales (`assets/data/scales.json`): `{"dorian": {"scaleSteps": [0, 2, 3, …]}}`.

use std::collections::HashMap;

use crate::json::Value;
use crate::record::{Issue, JsonField};

/// Degrees used when a track names no scale, or one that is not loaded.
pub const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scales {
    steps: HashMap<String, Vec<i32>>,
}

impl Scales {
    /// Read `scales.json` ([`ScaleList::from_json`]); a scale it cannot read
    /// is not loaded.
    pub fn from_json(value: &Value) -> (Self, Vec<Issue>) {
        let (list, issues) = ScaleList::from_json(value);
        (Self::from(&list), issues)
    }

    /// Degrees (semitones from the root, within an octave) of `name`, if loaded.
    pub fn get(&self, name: &str) -> Option<&[i32]> {
        self.steps.get(name).map(Vec::as_slice)
    }

    /// Degrees (semitones from the root, within an octave) of `name`, or the
    /// major scale.
    pub fn steps_or_major(&self, name: Option<&str>) -> &[i32] {
        name.and_then(|name| self.steps.get(name)).map_or(&MAJOR, Vec::as_slice)
    }
}

impl From<&ScaleList> for Scales {
    /// The scales of `list` that have degrees.
    fn from(list: &ScaleList) -> Self {
        let named = list.names.iter().zip(&list.steps);
        Self { steps: named.filter_map(|(name, steps)| Some((name.clone(), steps.clone()?))).collect() }
    }
}

/// Scale names (`scales.json` keys, in file order) and their degrees.
#[derive(Debug, Clone, Default)]
pub struct ScaleList {
    pub names: Vec<String>,
    pub steps: Vec<Option<Vec<i32>>>,
}

impl ScaleList {
    /// Read `scales.json`: every scale named, with its degrees when its
    /// `scaleSteps` is a list of whole numbers, else none (and an issue).
    pub fn from_json(value: &Value) -> (Self, Vec<Issue>) {
        let mut list = Self::default();
        let mut issues = Vec::new();
        for (name, scale) in value.as_object().into_iter().flat_map(|o| o.iter()) {
            list.names.push(name.to_owned());
            list.steps.push(scale_steps(scale, name, &mut issues));
        }
        (list, issues)
    }

    fn steps_of(&self, name: &str) -> Option<&[i32]> {
        let i = self.names.iter().position(|n| n == name)?;
        self.steps[i].as_deref()
    }

    /// The intervals of scale `name`: `range` notes up the scale.
    pub fn intervals(&self, name: &str, range: usize) -> Vec<i32> {
        let Some(steps) = self.steps_of(name) else { return vec![0] };
        let n = steps.len();
        (0..range).map(|i| steps[i % n] + (i / n) as i32 * 12).collect()
    }
}

/// The degrees of `scale` (its `scaleSteps`, whole numbers), or `None`,
/// said at `name`, when they cannot be read.
fn scale_steps(scale: &Value, name: &str, issues: &mut Vec<Issue>) -> Option<Vec<i32>> {
    let Some(raw) = scale.get("scaleSteps") else {
        issues.push(Issue::new(name, "no scaleSteps, scale not used"));
        return None;
    };
    let at = format!("{name}.scaleSteps");
    let Some(items) = raw.as_array().filter(|items| !items.is_empty()) else {
        issues.push(Issue::new(at, format!("not a list of degrees ({}), scale not used", raw.to_json())));
        return None;
    };
    let steps: Option<Vec<i32>> = items.iter().map(i32::from_json).collect();
    if steps.is_none() {
        issues.push(Issue::new(at, format!("not whole numbers ({}), scale not used", raw.to_json())));
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn lookup_with_major_fallback() {
        let (scales, issues) = Scales::from_json(
            &json::parse(r#"{"dorian":{"scaleSteps":[0,2,3,5,7,9,10]},"bad":{"scaleSteps":[0.5]},"x":1}"#).unwrap(),
        );
        assert_eq!(scales.steps_or_major(Some("dorian")), [0, 2, 3, 5, 7, 9, 10]);
        assert_eq!(scales.steps_or_major(Some("bad")), MAJOR);
        assert_eq!(scales.steps_or_major(None), MAJOR);
        let at: Vec<&str> = issues.iter().map(|i| i.at.as_str()).collect();
        assert_eq!(at, ["bad.scaleSteps", "x"]);
    }

    #[test]
    fn the_list_and_the_lookup_read_a_scale_alike() {
        let (list, _) = ScaleList::from_json(&json::parse(r#"{"a":{"scaleSteps":[0,"x",4]},"b":{}}"#).unwrap());
        assert_eq!(list.names, ["a", "b"]);
        assert_eq!(list.steps, [None, None]);
        assert_eq!(list.intervals("a", 3), [0]);
        assert_eq!(Scales::from(&list).get("a"), None);
    }
}
