//! The scale the UI is drawn at, in quarters: lengths of the layout are given
//! in scale units (one pixel at ×1) and multiplied by it, rounded to the
//! pixel. Whole scales give exactly the lengths times the scale; ×1.25,
//! ×1.5 and ×1.75 sit between them.

use std::fmt;
use std::ops::Mul;

/// A UI scale, a whole number of quarters of a scale unit (at least one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scale(i32);

/// Quarters in a whole scale unit.
const QUARTERS: i32 = 4;

impl Scale {
    /// ×1: one pixel per scale unit.
    pub const ONE: Self = Self(QUARTERS);

    /// The largest scale the UI is drawn at.
    pub const MAX: Self = Self::whole(4);

    /// `n` whole scale units (at least ×¼).
    #[must_use]
    pub const fn whole(n: i32) -> Self {
        Self::from_quarters(n.saturating_mul(QUARTERS))
    }

    /// `quarters` quarters of a scale unit (at least one).
    #[must_use]
    pub const fn from_quarters(quarters: i32) -> Self {
        Self(if quarters < 1 { 1 } else { quarters })
    }

    /// A display's scale factor, to the nearest quarter, at least ×1.
    #[must_use]
    pub fn from_factor(factor: f64) -> Self {
        // `as` saturates: a factor that is not a number counts as none.
        let quarters = (factor * f64::from(QUARTERS)).round() as i32;
        Self(quarters.max(QUARTERS))
    }

    /// The scale in quarters of a unit.
    #[must_use]
    pub const fn quarters(self) -> i32 {
        self.0
    }

    /// This scale `n` times over: text twice the regular size is `times(2)`.
    #[must_use]
    pub const fn times(self, n: i32) -> Self {
        Self::from_quarters(self.0.saturating_mul(n))
    }

    /// `quarters` quarters of this scale, to the nearest quarter: the small
    /// font is `part(3)`.
    #[must_use]
    pub const fn part(self, quarters: i32) -> Self {
        Self::from_quarters(round_quarters(self.0.saturating_mul(quarters)))
    }

    /// The scale of a title: a quarter bigger than the text, still within
    /// a row of text (12 units high).
    #[must_use]
    pub const fn title(self) -> Self {
        self.part(5)
    }

    /// One scale unit in pixels, rounded, at least 1: the thickness of a line.
    #[must_use]
    pub const fn px(self) -> i32 {
        let px = round_quarters(self.0);
        if px < 1 { 1 } else { px }
    }

    /// This scale `steps` quarters bigger (or smaller), within `min..=max`.
    #[must_use]
    pub fn step(self, steps: i32, min: Self, max: Self) -> Self {
        Self(self.0.saturating_add(steps)).clamp(min, max)
    }
}

/// `n` quarters as a whole number, the half rounded up.
const fn round_quarters(n: i32) -> i32 {
    n.saturating_add(QUARTERS / 2).div_euclid(QUARTERS)
}

impl Mul<Scale> for i32 {
    type Output = i32;

    /// `self` scale units in pixels, rounded.
    fn mul(self, scale: Scale) -> i32 {
        round_quarters(self.saturating_mul(scale.0))
    }
}

impl Mul<i32> for Scale {
    type Output = i32;

    /// `n` scale units in pixels, rounded.
    fn mul(self, n: i32) -> i32 {
        n * self
    }
}

impl fmt::Display for Scale {
    /// `1`, `1.25`, `1.5`, `1.75`, `2`…
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (whole, rest) = (self.0 / QUARTERS, self.0 % QUARTERS);
        match rest {
            0 => write!(f, "{whole}"),
            2 => write!(f, "{whole}.5"),
            _ => write!(f, "{whole}.{}", rest * 25),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_scales_multiply_exactly_and_quarters_round() {
        assert_eq!(12 * Scale::whole(2), 24);
        assert_eq!(Scale::whole(3) * -5, -15);
        assert_eq!(6 * Scale::from_quarters(5), 8, "7.5 rounds up");
        assert_eq!(7 * Scale::from_quarters(6), 11, "10.5 rounds up");
        assert_eq!(-6 * Scale::from_quarters(5), -7, "-7.5 rounds up too");
        assert_eq!(Scale::ONE.part(3), Scale::from_quarters(3));
        assert_eq!(Scale::from_quarters(5).part(3), Scale::from_quarters(4), "3.75 quarters");
    }

    #[test]
    fn a_display_factor_goes_to_the_nearest_quarter() {
        assert_eq!(Scale::from_factor(1.0), Scale::ONE);
        assert_eq!(Scale::from_factor(1.5), Scale::from_quarters(6));
        assert_eq!(Scale::from_factor(1.1), Scale::ONE);
        assert_eq!(Scale::from_factor(0.5), Scale::ONE, "never under ×1");
        assert_eq!(Scale::from_factor(f64::NAN), Scale::ONE);
    }

    #[test]
    fn it_reads_as_a_decimal() {
        let shown: Vec<String> = (4..=9).map(|q| Scale::from_quarters(q).to_string()).collect();
        assert_eq!(shown, ["1", "1.25", "1.5", "1.75", "2", "2.25"]);
    }
}
