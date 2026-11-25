//! Decimal rounding, wrapping and decibels shared by the crates.

/// `x` rounded to `decimals` decimal places, halves away from zero (as
/// [`f64::round`]). `x` is scaled first, so a value stored just under a half
/// (`1.005` is 1.00499…) can round either way: this is a display and file
/// precision, not an exact decimal rounding. A value that rounds to zero is
/// `0`, never `-0`.
pub fn round_to(x: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    let rounded = (x * scale).round() / scale;
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// `x` inside `min`–`max`, NaN kept. Unlike [`f64::clamp`], `min > max` (bounds
/// read from a table or a file) gives `max` instead of a panic.
#[must_use]
pub fn clamp(x: f64, min: f64, max: f64) -> f64 {
    if x.is_nan() { x } else { x.max(min).min(max) }
}

/// `x` to 2 decimals: the precision of velocities, probabilities and the
/// other settings saved rounded.
pub fn round2(x: f64) -> f64 {
    round_to(x, 2)
}

/// `x` wrapped into `[0, period)` ([`f64::rem_euclid`]); a tiny negative `x`,
/// which `rem_euclid` rounds up to `period`, is `0`. NaN stays NaN.
pub fn wrap(x: f64, period: f64) -> f64 {
    let r = x.rem_euclid(period);
    if r == period { 0.0 } else { r }
}

/// Largest whole number an `f64` holds exactly (2^53 − 1).
const MAX_EXACT_WHOLE: f64 = 9_007_199_254_740_991.0;

/// `x` as a whole number, if it is one: no fraction, from 0 to 2^53 − 1
/// (past it an `f64` no longer holds every whole number). For a count or
/// an index read from a file or given by a client: narrow it with `try_from`.
#[must_use]
pub fn whole(x: f64) -> Option<u64> {
    // Whole and in range: the conversion loses nothing.
    (x.fract() == 0.0 && (0.0..=MAX_EXACT_WHOLE).contains(&x)).then_some(x as u64)
}

/// `x` rounded down to a `u32`, saturating: below 0 (and NaN) is 0, past
/// `u32::MAX` is `u32::MAX`. For a position already checked, or one where
/// the nearest step is what is wanted.
#[must_use]
pub fn floor_u32(x: f64) -> u32 {
    // `as` from a float saturates and maps NaN to 0: the wanted rounding.
    x.floor() as u32
}

/// `x` as the UI shows it: its shortest form (`140`, `0.5`), zero without a
/// sign (a value rounded from just under 0 is `-0.0`).
pub fn text(x: f64) -> String {
    if x == 0.0 { "0".to_owned() } else { x.to_string() }
}

/// The linear gain (amplitude ratio) of `db` decibels: 0 dB is 1.
#[must_use]
#[inline]
pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The decibels of the linear gain (amplitude ratio) `gain`: 1 is 0 dB, 0 is
/// minus infinity, a negative gain NaN. The caller guards what it cannot take.
#[must_use]
#[inline]
pub fn gain_to_db(gain: f64) -> f64 {
    20.0 * gain.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_and_gains_convert_both_ways() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert_eq!(gain_to_db(1.0), 0.0);
        assert!((db_to_gain(-6.0) - 0.501_187).abs() < 1e-6);
        assert!((gain_to_db(db_to_gain(-23.5)) + 23.5).abs() < 1e-12);
        assert_eq!(gain_to_db(0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn rounds_halves_away_from_zero() {
        assert_eq!(round_to(0.81234, 2), 0.81);
        assert_eq!(round_to(0.876, 2), 0.88);
        assert_eq!(round_to(1234.567, 2), 1234.57);
        assert_eq!(round_to(0.125, 2), 0.13);
        assert_eq!(round_to(-0.125, 2), -0.13);
        assert_eq!(round_to(1.0, 2), 1.0);
        assert_eq!(round_to(2.5, 0), 3.0);
        assert_eq!(round_to(-2.5, 0), -3.0);
        assert_eq!(round_to(9.9996, 3), 10.0);
        assert!(round_to(f64::NAN, 2).is_nan());
        assert!(round_to(-0.004, 2).is_sign_positive());
    }

    #[test]
    fn wraps_into_the_period() {
        assert_eq!(wrap(14.0, 12.0), 2.0);
        assert_eq!(wrap(-1.0, 12.0), 11.0);
        assert_eq!(wrap(-12.0, 12.0), 0.0);
        assert_eq!(wrap(-1e-20, 12.0), 0.0);
        assert_eq!(wrap(0.5, 12.0), 0.5);
        assert!(wrap(f64::NAN, 12.0).is_nan());
    }

    #[test]
    fn text_is_shortest_and_zero_unsigned() {
        assert_eq!(text(140.0), "140");
        assert_eq!(text(0.5), "0.5");
        assert_eq!(text(-0.0), "0");
        assert_eq!(text((-0.3f64).round()), "0");
        assert_eq!(text(-2.0), "-2");
        assert_eq!(text(0.1 + 0.2), "0.30000000000000004");
    }

    #[test]
    fn whole_numbers_are_whole_from_0_and_exact() {
        assert_eq!(whole(0.0), Some(0));
        assert_eq!(whole(8080.0), Some(8080));
        assert_eq!(whole(MAX_EXACT_WHOLE), Some(9_007_199_254_740_991));
        for not_whole in [8080.5, -1.0, MAX_EXACT_WHOLE + 2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(whole(not_whole), None, "{not_whole}");
        }
    }

    #[test]
    fn floor_u32_rounds_down_and_saturates() {
        assert_eq!(floor_u32(3.9), 3);
        assert_eq!(floor_u32(-0.5), 0);
        assert_eq!(floor_u32(f64::NAN), 0);
        assert_eq!(floor_u32(1e12), u32::MAX);
    }
}
