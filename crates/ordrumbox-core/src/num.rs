//! Decimal rounding shared by the crates.

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

/// `x` to 2 decimals: the precision of velocities, probabilities and the
/// other settings saved rounded.
pub fn round2(x: f64) -> f64 {
    round_to(x, 2)
}

/// `x` as the UI shows it: its shortest form (`140`, `0.5`), zero without a
/// sign (a value rounded from just under 0 is `-0.0`).
pub fn text(x: f64) -> String {
    if x == 0.0 { "0".to_owned() } else { x.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn text_is_shortest_and_zero_unsigned() {
        assert_eq!(text(140.0), "140");
        assert_eq!(text(0.5), "0.5");
        assert_eq!(text(-0.0), "0");
        assert_eq!(text((-0.3f64).round()), "0");
        assert_eq!(text(-2.0), "-2");
        assert_eq!(text(0.1 + 0.2), "0.30000000000000004");
    }
}
