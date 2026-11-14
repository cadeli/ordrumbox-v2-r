//! Quantizing notes: each one moved towards the nearest line of a grid, all
//! the way or part of it (a strength), whether it sits off its step by a
//! shift (a note recorded off the grid) or on a step off the grid chosen.

use ordrumbox_core::note::MAX_SHIFT_STEPS;
use ordrumbox_core::num::round2;
use ordrumbox_core::{Note, Pattern};

use crate::block::{Zone, note_units};

/// Which notes of a pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scope {
    /// The notes starting inside a zone.
    Zone(Zone),
    /// Every note of a track.
    Track(usize),
    /// Every note of the pattern.
    Pattern,
}

impl Scope {
    fn holds(self, track: usize, note: &Note, steps_per_beat: i32) -> bool {
        match self {
            Self::Zone(zone) => zone.holds(track, note_units(note, steps_per_beat)),
            Self::Track(t) => t == track,
            Self::Pattern => true,
        }
    }
}

/// How to quantize.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantize {
    /// The grid: lines to a beat (4: sixteenth notes). A grid finer than a
    /// track's steps is kept to what the track's step and its shift can hold.
    pub lines_per_beat: u32,
    /// How far each note goes to its line, 0 (not at all) to 1 (on it).
    pub strength: f64,
}

/// Quantize the notes of `pattern` in `scope`; how many moved.
pub fn quantize(pattern: &mut Pattern, scope: Scope, how: Quantize) -> usize {
    let strength = how.strength.clamp(0.0, 1.0);
    let lines = how.lines_per_beat.max(1);
    let beat_count = pattern.beat_count.max(1);
    let mut moved = 0;
    for (t, track) in pattern.tracks.iter_mut().enumerate() {
        let spb = track.steps_per_beat.max(1);
        let (spb_f, total) = (f64::from(spb), i64::from(beat_count) * i64::from(spb));
        // A line is at least a quarter of a step: closer, a shift cannot tell them apart.
        let grid = (spb_f / f64::from(lines)).max(0.25);
        for note in track.notes.iter_mut().filter(|n| scope.holds(t, n, spb)) {
            let at = f64::from(note.beat) * spb_f + f64::from(note.step) + note.late_steps();
            let line = (at / grid).round() * grid;
            let to = at + (line - at) * strength;
            // The step it falls on, and what is left of it as a shift.
            let nearest = to.round();
            let shift = round2((to - nearest).clamp(-MAX_SHIFT_STEPS, MAX_SHIFT_STEPS));
            let nearest = (nearest as i64).rem_euclid(total);
            let (beat, step) = (nearest.div_euclid(i64::from(spb)) as i32, nearest.rem_euclid(i64::from(spb)) as i32);
            if (beat, step) != (note.beat, note.step) || shift != note.shift_steps {
                note.beat = beat;
                note.step = step;
                note.shift_steps = shift;
                note.step_percent = Some((f64::from(step) * 100.0 / spb_f).round());
                moved += 1;
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Track;

    fn pattern(notes: &[(i32, i32, f64)]) -> Pattern {
        let notes = notes.iter().map(|&(beat, step, shift)| Note { beat, step, shift_steps: shift, ..Note::default() });
        Pattern {
            tracks: vec![Track { steps_per_beat: 4, notes: notes.collect(), ..Track::default() }],
            ..Pattern::default()
        }
    }

    fn places(p: &Pattern) -> Vec<(i32, i32, f64)> {
        p.tracks[0].notes.iter().map(|n| (n.beat, n.step, n.shift_steps)).collect()
    }

    const FULL: Quantize = Quantize { lines_per_beat: 4, strength: 1.0 };

    #[test]
    fn a_shift_is_taken_off_and_a_note_already_on_its_line_is_left_alone() {
        let mut p = pattern(&[(0, 1, 0.3), (1, 2, -0.4), (2, 0, 0.0)]);
        assert_eq!(quantize(&mut p, Scope::Pattern, FULL), 2);
        assert_eq!(places(&p), [(0, 1, 0.0), (1, 2, 0.0), (2, 0, 0.0)]);
    }

    #[test]
    fn a_coarser_grid_moves_the_steps_to_its_nearest_line() {
        // Eighth notes on 4 steps a beat: a line every 2 steps. 1.3 and 2.4 are nearest 2; 3 is halfway: the later line.
        let mut p = pattern(&[(0, 1, 0.3), (0, 2, 0.4), (0, 3, 0.0)]);
        quantize(&mut p, Scope::Pattern, Quantize { lines_per_beat: 2, strength: 1.0 });
        assert_eq!(places(&p), [(0, 2, 0.0), (0, 2, 0.0), (1, 0, 0.0)]);
        // A quarter-note grid: step 3 of beat 0 is nearer the next beat (4) than the beat's start.
        let mut p = pattern(&[(0, 3, 0.0), (0, 1, 0.0)]);
        quantize(&mut p, Scope::Pattern, Quantize { lines_per_beat: 1, strength: 1.0 });
        assert_eq!(places(&p), [(1, 0, 0.0), (0, 0, 0.0)]);
    }

    #[test]
    fn a_strength_goes_part_of_the_way_and_keeps_what_is_left_as_a_shift() {
        let mut p = pattern(&[(0, 1, 0.4), (0, 4 - 1, -0.25)]);
        // Shift 0.4 from step 1: 0.4 late; half the way back is 0.2 late.
        quantize(&mut p, Scope::Pattern, Quantize { lines_per_beat: 4, strength: 0.5 });
        assert_eq!(places(&p), [(0, 1, 0.2), (0, 3, -0.13)]);
    }

    #[test]
    fn a_note_pulled_past_the_end_or_the_start_wraps_round_the_pattern() {
        let mut p = pattern(&[(0, 0, -0.4), (3, 3, 0.0)]);
        quantize(&mut p, Scope::Pattern, Quantize { lines_per_beat: 1, strength: 1.0 });
        assert_eq!(
            places(&p),
            [(0, 0, 0.0), (0, 0, 0.0)],
            "the last step of beat 3 is nearer the next beat: the first"
        );
    }

    #[test]
    fn only_the_scope_is_quantized() {
        let mut p = pattern(&[(0, 1, 0.3), (2, 1, 0.3)]);
        p.tracks.push(p.tracks[0].clone());
        let zone = Zone { first_track: 0, last_track: 0, start_units: 0, end_units: crate::block::BEAT_UNITS };
        assert_eq!(quantize(&mut p, Scope::Zone(zone), FULL), 1);
        assert_eq!(places(&p), [(0, 1, 0.0), (2, 1, 0.3)]);
        assert_eq!(quantize(&mut p, Scope::Track(1), FULL), 2);
        assert_eq!(p.tracks[1].notes.iter().map(|n| n.shift_steps).collect::<Vec<_>>(), [0.0, 0.0]);
        assert_eq!(p.tracks[0].notes[1].shift_steps, 0.3, "track 0 is not in the scope");
    }
}
