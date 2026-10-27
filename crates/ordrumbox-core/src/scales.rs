//! Musical scales (`assets/data/scales.json`): `{"dorian": {"scaleSteps": [0, 2, 3, …]}}`.

use std::collections::HashMap;

use crate::json::Value;

/// Degrees used when a track names no scale, or one that is not loaded.
pub const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scales {
    steps: HashMap<String, Vec<i32>>,
}

impl Scales {
    /// Read `scales.json`; entries without an integer `scaleSteps` list are skipped.
    pub fn from_json(value: &Value) -> Self {
        let steps = value
            .as_object()
            .into_iter()
            .flat_map(|object| object.iter())
            .filter_map(|(name, scale)| {
                let steps = scale.get("scaleSteps")?.as_array()?;
                let steps: Option<Vec<i32>> =
                    steps.iter().map(|s| s.as_f64().filter(|n| n.fract() == 0.0).map(|n| n as i32)).collect();
                Some((name.to_owned(), steps?))
            })
            .collect();
        Self { steps }
    }

    /// Degrees (semitones from the root, within an octave) of `name`, or the
    /// major scale.
    pub fn steps_or_major(&self, name: Option<&str>) -> &[i32] {
        name.and_then(|name| self.steps.get(name)).map_or(&MAJOR, Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn lookup_with_major_fallback() {
        let scales = Scales::from_json(
            &json::parse(r#"{"dorian":{"scaleSteps":[0,2,3,5,7,9,10]},"bad":{"scaleSteps":[0.5]},"x":1}"#).unwrap(),
        );
        assert_eq!(scales.steps_or_major(Some("dorian")), [0, 2, 3, 5, 7, 9, 10]);
        assert_eq!(scales.steps_or_major(Some("bad")), MAJOR);
        assert_eq!(scales.steps_or_major(None), MAJOR);
    }
}
