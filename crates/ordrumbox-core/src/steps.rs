//! Grid arithmetic (`src/core/notes.js`): steps, beats and ticks.
//!
//! A beat is [`TICK`] ticks whatever the grid; a track divides its beat into
//! `stepsPerBeat` steps, so a step is `TICK / stepsPerBeat` ticks, rounded per
//! step position. 96 is divided by every usual grid, triplets included (1, 2,
//! 3, 4, 6, 8, 12, 16, 24, 32): only odd grids such as 5 or 7 round.

/// Ticks per **beat** (not per step). The JS app counted 32, which rounded
/// triplets; MIDI files are written at the same 96 PPQN.
pub const TICK: u32 = 96;
/// Beats of the longest pattern or track a file may hold (an edit stops well
/// before): what plays a pattern works in memory with its length.
pub const MAX_LOADED_BEATS: i32 = 1024;
/// Steps a beat of a file may be divided into: every tick of it.
pub const MAX_LOADED_STEPS_PER_BEAT: i32 = TICK as i32;
/// Steps of the longest track a file may hold.
pub const MAX_LOADED_STEPS: i32 = MAX_LOADED_BEATS * MAX_LOADED_STEPS_PER_BEAT;

/// Absolute step of a note in its track: `beat * stepsPerBeat + beatStep`.
pub fn absolute_step(beat: i32, beat_step: i32, steps_per_beat: i32) -> i64 {
    i64::from(beat) * i64::from(steps_per_beat) + i64::from(beat_step)
}

/// `(beat, beatStep)` of an absolute step, as `stepToBeat()`: the beat is
/// floored, the step is `%` (it keeps the sign of `step`). `steps_per_beat > 0`.
pub fn step_to_beat(step: i64, steps_per_beat: i32) -> (i64, i64) {
    let spb = i64::from(steps_per_beat);
    (step.div_euclid(spb), step % spb)
}

/// Tick of an absolute step: `beat * TICK + round(beatStep * TICK / stepsPerBeat)`.
pub fn step_to_tick(step: i64, steps_per_beat: i32) -> i64 {
    let (beat, beat_step) = step_to_beat(step, steps_per_beat);
    beat * i64::from(TICK) + (beat_step as f64 * f64::from(TICK) / f64::from(steps_per_beat)).round() as i64
}

/// Tick nearest a position counted in steps, which may fall between steps (a
/// repeat at a fraction of a step). For a whole step `>= 0` it is
/// [`step_to_tick`].
pub fn position_to_tick(steps: f64, steps_per_beat: i32) -> i64 {
    (steps * f64::from(TICK) / f64::from(steps_per_beat)).round() as i64
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
        assert_eq!(step_to_tick(5, 4), 120);
        assert_eq!(step_to_tick(1, 5), 19); // 19.2
        assert_eq!(step_to_tick(3, 5), 58); // 57.6
        assert_eq!(step_to_tick(7, 5), 134); // 96 + 38.4
    }

    #[test]
    fn the_usual_grids_fall_on_whole_ticks() {
        for steps_per_beat in [1, 2, 3, 4, 6, 8, 12, 16, 24, 32] {
            assert_eq!(TICK % steps_per_beat, 0, "{steps_per_beat} steps a beat");
        }
        assert_eq!(step_to_tick(1, 3), 32);
        assert_eq!(step_to_tick(4, 6), 64);
    }

    #[test]
    fn spacing_codes() {
        assert_eq!(step_spacing(1.0), 0.125);
        assert_eq!(step_spacing(4.0), 0.5);
        assert_eq!(step_spacing(8.0), 1.0);
        assert_eq!(step_spacing(12.0), 5.0);
    }
}
