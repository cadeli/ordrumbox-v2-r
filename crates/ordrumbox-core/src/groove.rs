//! A track's groove: how late each step of a beat plays and how hard it hits,
//! the swing being one groove among others.

use crate::json::{Object, Value};
use crate::record::JsonField;
use crate::steps::MAX_BEATS;

/// Longest table of a groove: one entry per step of a beat of the finest grid.
pub const MAX_GROOVE_STEPS: usize = 8;

/// What a step of the grid does to its notes, at full amount: its delay, in
/// beats, and the velocity it adds (negative: takes off).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GrooveStep {
    pub timing: f64,
    pub velocity: f64,
}

/// A groove: a table repeated over the steps of each beat, scaled by the
/// track's `grooveAmount`. An empty table is straight time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Groove {
    steps: Vec<GrooveStep>,
}

/// A named groove for the grid of 4 steps a beat (the shuffle, for 3).
struct Preset {
    name: &'static str,
    timing: &'static [f64],
    velocity: &'static [f64],
}

/// The grooves the editor offers, straight time first.
const PRESETS: [Preset; 6] = [
    Preset { name: "Straight", timing: &[], velocity: &[] },
    Preset { name: "Swing 16th", timing: &[0.0, 1.0], velocity: &[] },
    Preset { name: "Swing 8th", timing: &[0.0, 0.0, 1.0, 0.0], velocity: &[] },
    Preset { name: "Shuffle", timing: &[0.0, 1.0, 0.0], velocity: &[] },
    Preset { name: "Laid back", timing: &[0.0, 0.25, 0.5, 0.25], velocity: &[] },
    Preset { name: "Accent", timing: &[], velocity: &[0.5, -0.3, 0.2, -0.3] },
];

impl Groove {
    /// The groove `name` of [`Self::preset_names`], if there is one.
    #[must_use]
    pub fn preset(name: &str) -> Option<Self> {
        PRESETS.iter().find(|p| p.name == name).map(|p| Self::from_tables(p.timing, p.velocity))
    }

    /// The names of the grooves the editor offers, straight time first.
    pub fn preset_names() -> impl Iterator<Item = &'static str> {
        PRESETS.iter().map(|p| p.name)
    }

    /// The name of the preset this groove is, `None` for one of its own.
    #[must_use]
    pub fn preset_name(&self) -> Option<&'static str> {
        PRESETS.iter().find(|p| *self == Self::from_tables(p.timing, p.velocity)).map(|p| p.name)
    }

    /// The swing of old files: the second step of each group of `resolution`
    /// steps is late; straight for a group of 1 or less.
    #[must_use]
    pub fn swing(resolution: i32) -> Self {
        let count = usize::try_from(resolution.clamp(1, MAX_GROOVE_STEPS as i32)).unwrap_or(1);
        if count == 1 {
            return Self::default();
        }
        let timing: Vec<f64> = (0..count).map(|i| f64::from(u8::from(i == 1))).collect();
        Self::from_tables(&timing, &[])
    }

    /// The resolution of the swing this groove is ([`Self::swing`]; 1 when
    /// straight), `None` for any other groove.
    #[must_use]
    pub fn swing_resolution(&self) -> Option<i32> {
        if self.steps.is_empty() {
            return Some(1);
        }
        let resolution = i32::try_from(self.steps.len()).ok()?;
        (*self == Self::swing(resolution)).then_some(resolution)
    }

    /// A groove of the two tables, the shorter padded with zeros.
    fn from_tables(timing: &[f64], velocity: &[f64]) -> Self {
        let steps = (0..timing.len().max(velocity.len()).min(MAX_GROOVE_STEPS))
            .map(|i| GrooveStep {
                timing: timing.get(i).copied().unwrap_or(0.0),
                velocity: velocity.get(i).copied().unwrap_or(0.0),
            })
            .collect();
        Self { steps }
    }

    /// Whether every step plays on the grid, as hard as written.
    #[must_use]
    pub fn is_straight(&self) -> bool {
        self.steps.iter().all(|s| *s == GrooveStep::default())
    }

    /// The entry of step `step` of a beat; the table repeats when it is shorter than the beat.
    fn at(&self, step: i32) -> GrooveStep {
        let count = i32::try_from(self.steps.len()).unwrap_or(i32::MAX);
        if count == 0 { GrooveStep::default() } else { self.steps[step.rem_euclid(count) as usize] }
    }

    /// How late step `step` of a beat plays at `amount`, in beats.
    #[must_use]
    pub fn timing_beats(&self, step: i32, amount: f64) -> f64 {
        self.at(step).timing * amount
    }

    /// The velocity step `step` of a beat adds at `amount`.
    #[must_use]
    pub fn velocity_delta(&self, step: i32, amount: f64) -> f64 {
        self.at(step).velocity * amount
    }
}

/// An object `{"timing": [...], "velocity": [...]}` of up to
/// [`MAX_GROOVE_STEPS`] numbers each, within a measure's worth of beats.
impl JsonField for Groove {
    fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let table = |key: &str| -> Option<Vec<f64>> {
            let Some(value) = object.get(key) else { return Some(Vec::new()) };
            let numbers: Option<Vec<f64>> =
                value.as_array()?.iter().map(|n| n.as_f64().filter(|n| n.abs() <= f64::from(MAX_BEATS))).collect();
            numbers.filter(|numbers| numbers.len() <= MAX_GROOVE_STEPS)
        };
        Some(Self::from_tables(&table("timing")?, &table("velocity")?))
    }

    fn to_json(&self) -> Value {
        self.to_value()
    }
}

impl Groove {
    /// The groove as the object a file holds: the tables that are not all zeros.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let table =
            |pick: fn(&GrooveStep) -> f64| Value::Array(self.steps.iter().map(|s| Value::Number(pick(s))).collect());
        let mut object = Object::new();
        if self.steps.iter().any(|s| s.timing != 0.0) {
            object.insert("timing", table(|s| s.timing));
        }
        if self.steps.iter().any(|s| s.velocity != 0.0) {
            object.insert("velocity", table(|s| s.velocity));
        }
        Value::Object(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_delays_the_second_step_of_each_group() {
        let swung = Groove::swing(2);
        assert_eq!([0, 1, 2, 3].map(|step| swung.timing_beats(step, 0.33)), [0.0, 0.33, 0.0, 0.33]);
        let triplets = Groove::swing(3);
        assert_eq!([0, 1, 2, 3, 4].map(|step| triplets.timing_beats(step, 0.25)), [0.0, 0.25, 0.0, 0.0, 0.25]);
        assert!(Groove::swing(1).is_straight());
        assert_eq!([Groove::swing(3).swing_resolution(), Groove::default().swing_resolution()], [Some(3), Some(1)]);
        assert_eq!(Groove::preset("Swing 8th").unwrap().swing_resolution(), None);
    }

    #[test]
    fn a_step_takes_its_velocity_at_the_amount() {
        let accent = Groove::preset("Accent").unwrap();
        assert_eq!(accent.velocity_delta(0, 0.5), 0.25);
        assert_eq!(accent.timing_beats(0, 1.0), 0.0);
        assert_eq!(Groove::default().velocity_delta(3, 1.0), 0.0);
    }

    #[test]
    fn every_preset_is_found_by_its_table_and_its_name() {
        for name in Groove::preset_names() {
            assert_eq!(Groove::preset(name).and_then(|g| g.preset_name()), Some(name));
        }
        assert_eq!(Groove::swing(2).preset_name(), Some("Swing 16th"));
        assert_eq!(Groove::from_tables(&[0.0, 0.4], &[]).preset_name(), None);
    }

    #[test]
    fn it_round_trips_through_json() {
        for name in Groove::preset_names() {
            let groove = Groove::preset(name).unwrap();
            assert_eq!(Groove::from_json(&groove.to_json()), Some(groove), "{name}");
        }
        let both = Groove::from_tables(&[0.0, 0.5], &[0.1]);
        assert_eq!(both.to_json().to_json(), r#"{"timing":[0,0.5],"velocity":[0.1,0]}"#);
        assert_eq!(Groove::from_json(&both.to_json()), Some(both));
    }

    #[test]
    fn a_broken_table_is_refused() {
        let parse = |text: &str| Groove::from_json(&crate::json::parse(text).unwrap());
        assert_eq!(parse(r#"[0,1]"#), None);
        assert_eq!(parse(r#"{"timing":"late"}"#), None);
        assert_eq!(parse(r#"{"timing":[0,"x"]}"#), None);
        assert_eq!(parse(r#"{"timing":[0,0,0,0,0,0,0,0,0]}"#), None);
        assert_eq!(parse(r#"{}"#), Some(Groove::default()));
    }
}
