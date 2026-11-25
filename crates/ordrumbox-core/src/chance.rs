//! A chance, 0–1 like every probability of the model, that a file holds as a percentage.
//!
//! The generator's per-kind chances (`autoPitchChance`…) are written 0–100
//! in the files, while a note's or a track's `probability` is written 0–1.
//! In the model both are fractions: [`Chance`] converts at the file's edge
//! only, so the code never mixes the two scales.

use crate::json::Value;
use crate::record::{JsonField, MAX_NUMBER};

/// Decimals a percentage keeps when written: enough for any value a file
/// gives, few enough to drop the noise of the `/ 100`, `* 100` round trip.
const WRITTEN_DECIMALS: f64 = 1e9;

/// A chance, 0–1, written to files as a percentage, 0–100.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Chance(f64);

impl Chance {
    /// The chance of `fraction`, 0–1.
    #[must_use]
    pub const fn new(fraction: f64) -> Self {
        Self(fraction)
    }

    /// The chance as a fraction, 0–1.
    #[must_use]
    pub const fn fraction(self) -> f64 {
        self.0
    }

    /// The chance as a percentage, 0–100, as files and sliders show it.
    #[must_use]
    pub fn percent(self) -> f64 {
        (self.0 * 100.0 * WRITTEN_DECIMALS).round() / WRITTEN_DECIMALS
    }
}

/// A number within [`MAX_NUMBER`], read as a percentage.
impl JsonField for Chance {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_f64().filter(|n| n.abs() <= MAX_NUMBER).map(|percent| Self(percent / 100.0))
    }
    fn to_json(&self) -> Value {
        Value::Number(self.percent())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percentage_comes_back_as_written() {
        for percent in [0.0, 7.0, 12.5, 33.0, 50.0, 57.0, 99.9, 100.0, 12.345_678_9] {
            let chance = Chance::from_json(&Value::Number(percent)).unwrap();
            assert_eq!(chance.to_json(), Value::Number(percent), "{percent}");
        }
        assert_eq!(Chance::from_json(&Value::Number(25.0)), Some(Chance::new(0.25)));
        assert_eq!(Chance::from_json(&Value::from("25")), None);
    }
}
