//! How far apart a note's repeats (retriggers, arpeggio notes) fall, and the
//! `retriggerRate` code the files write it as.

use crate::json::Value;
use crate::record::{JsonField, MAX_NUMBER};

/// The first code of [`RetriggerSpacing::Steps`]: codes below it are eighths
/// of a step, from it up whole steps (8 is one step, 9 two steps…).
const FIRST_STEPS_CODE: f64 = 8.0;

/// The spacing of a note's repeats: a number of eighths of a step (tight
/// rolls), or a number of steps. A file writes it as one `retriggerRate`
/// code: the eighths as they are (1–7), the steps plus 7 (8 and up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetriggerSpacing {
    /// Eighths of a step, below 8; 1, the default, is the tightest.
    Eighths(f64),
    /// Whole steps, 1 and up.
    Steps(f64),
}

impl Default for RetriggerSpacing {
    fn default() -> Self {
        Self::Eighths(1.0)
    }
}

impl RetriggerSpacing {
    /// The spacing a `retriggerRate` code gives.
    #[must_use]
    pub fn from_code(code: f64) -> Self {
        if code < FIRST_STEPS_CODE { Self::Eighths(code) } else { Self::Steps(code - (FIRST_STEPS_CODE - 1.0)) }
    }

    /// The `retriggerRate` code a file writes.
    #[must_use]
    pub fn code(self) -> f64 {
        match self {
            Self::Eighths(eighths) => eighths,
            Self::Steps(steps) => steps + (FIRST_STEPS_CODE - 1.0),
        }
    }

    /// The spacing in steps, which may be a fraction of a step.
    #[must_use]
    pub fn steps(self) -> f64 {
        match self {
            Self::Eighths(eighths) => eighths / 8.0,
            Self::Steps(steps) => steps,
        }
    }
}

/// A number within [`MAX_NUMBER`], read as a `retriggerRate` code.
impl JsonField for RetriggerSpacing {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_f64().filter(|n| n.abs() <= MAX_NUMBER).map(Self::from_code)
    }
    fn to_json(&self) -> Value {
        Value::Number(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_below_8_are_eighths_from_8_up_steps() {
        assert_eq!(RetriggerSpacing::from_code(1.0).steps(), 0.125);
        assert_eq!(RetriggerSpacing::from_code(4.0).steps(), 0.5);
        assert_eq!(RetriggerSpacing::from_code(8.0), RetriggerSpacing::Steps(1.0));
        assert_eq!(RetriggerSpacing::from_code(12.0).steps(), 5.0);
        for code in [0.0, 1.0, 2.5, 7.0, 8.0, 9.0, 16.0] {
            assert_eq!(RetriggerSpacing::from_code(code).code(), code);
        }
    }
}
