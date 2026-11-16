//! Flushing tiny values to zero, so that a decaying state never sits on a
//! denormal (the render path's slowest numbers).

/// Level below which states and outputs are flushed to zero.
pub(crate) const FLUSH: f64 = 1e-15;

/// Zero `x` when it is tiny.
#[inline]
pub(crate) fn flush(x: &mut f64) {
    if x.abs() < FLUSH {
        *x = 0.0;
    }
}

/// Zero a `state` that is tiny or not a number: a filter never stays stuck
/// on a denormal, an infinity or a NaN.
///
/// A branch, not a selected value: it is almost never taken, and a select
/// would sit on the filter's feedback path (measured: 2 ns a frame).
#[inline]
pub(crate) fn flush_finite(state: &mut f64) {
    if state.abs() < FLUSH || !state.is_finite() {
        *state = 0.0;
    }
}
