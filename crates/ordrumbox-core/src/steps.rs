//! Grid arithmetic (`src/core/notes.js`): steps, beats and ticks.
//!
//! A beat is [`TICK`] ticks whatever the grid; a track divides its beat into
//! `stepsPerBeat` steps, so a step is `TICK / stepsPerBeat` ticks, rounded per
//! step position.

use crate::js;

/// Ticks per **beat** (not per step).
pub const TICK: u32 = 32;

/// Absolute step of a note in its track: `beat * stepsPerBeat + beatStep`.
pub fn absolute_step(beat: i32, beat_step: i32, steps_per_beat: i32) -> i64 {
    i64::from(beat) * i64::from(steps_per_beat) + i64::from(beat_step)
}

/// `(beat, beatStep)` of an absolute step, as `stepToBeat()`: the beat is
/// floored, the step is JS `%` (it keeps the sign of `step`). `steps_per_beat > 0`.
pub fn step_to_beat(step: i64, steps_per_beat: i32) -> (i64, i64) {
    let spb = i64::from(steps_per_beat);
    (step.div_euclid(spb), step % spb)
}

/// Tick of an absolute step: `beat * TICK + round(beatStep * TICK / stepsPerBeat)`.
pub fn step_to_tick(step: i64, steps_per_beat: i32) -> i64 {
    let (beat, beat_step) = step_to_beat(step, steps_per_beat);
    beat * i64::from(TICK) + js::round(beat_step as f64 * f64::from(TICK) / f64::from(steps_per_beat)) as i64
}

/// Spacing, in steps, of a `rate` code: below 8 it is `rate / 8` of a step,
/// from 8 up `rate - 7` steps.
pub fn step_spacing(rate: f64) -> f64 {
    if rate < 8.0 { rate / 8.0 } else { rate - 7.0 }
}

/// Playback-rate ratio of a transposition in semitones.
pub fn semitone_ratio(semitones: f64) -> f64 {
    (semitones / 12.0).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_to_tick_rounds_per_step() {
        assert_eq!(step_to_tick(0, 4), 0);
        assert_eq!(step_to_tick(5, 4), 40);
        assert_eq!(step_to_tick(1, 3), 11); // 10.67
        assert_eq!(step_to_tick(2, 3), 21); // 21.33
        assert_eq!(step_to_tick(4, 3), 43);
    }

    #[test]
    fn spacing_codes() {
        assert_eq!(step_spacing(1.0), 0.125);
        assert_eq!(step_spacing(4.0), 0.5);
        assert_eq!(step_spacing(8.0), 1.0);
        assert_eq!(step_spacing(12.0), 5.0);
    }
}
