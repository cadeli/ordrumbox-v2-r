//! Humanizing notes: each one played a little early or late (its shift) and
//! a little softer or louder (its velocity), by a random amount within a
//! range, as a drummer's hits never land quite on the grid nor at one force.

use ordrumbox_core::note::MAX_SHIFT_STEPS;
use ordrumbox_core::num::round2;
use ordrumbox_core::{Pattern, Rng};

use crate::quantize::Scope;

/// How much to humanize.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Humanize {
    /// The furthest a note moves either way, in steps of its track (0.1: a
    /// tenth of a step). Its shift stays within [`MAX_SHIFT_STEPS`].
    pub timing_steps: f64,
    /// The most a note's velocity changes either way, as a share of it
    /// (0.1: ±10 %), so a ghost note stays a ghost and nothing goes silent.
    pub velocity: f64,
}

/// Humanize the notes of `pattern` in `scope`, drawing from `rng`; how many
/// changed. The steps stay: only the shift and the velocity move.
pub fn humanize(pattern: &mut Pattern, scope: Scope, how: Humanize, rng: &mut Rng) -> usize {
    let timing = how.timing_steps.clamp(0.0, MAX_SHIFT_STEPS);
    let velocity = how.velocity.clamp(0.0, 1.0);
    let mut changed = 0;
    for (t, track) in pattern.tracks.iter_mut().enumerate() {
        let spb = track.steps_per_beat.max(1);
        for note in track.notes.iter_mut().filter(|n| scope.holds(t, n, spb)) {
            let shift = round2((note.late_steps() + timing * spread(rng)).clamp(-MAX_SHIFT_STEPS, MAX_SHIFT_STEPS));
            let played = if note.velocity.is_finite() { note.velocity } else { 0.8 };
            let gain = round2((played * (1.0 + velocity * spread(rng))).clamp(0.0, 1.0));
            if shift != note.shift_steps || gain != note.velocity {
                note.shift_steps = shift;
                note.velocity = gain;
                changed += 1;
            }
        }
    }
    changed
}

/// Uniform in `[-1, 1)`.
fn spread(rng: &mut Rng) -> f64 {
    rng.next_f64() * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::{Note, Track};

    fn pattern(notes: &[(i32, f64)]) -> Pattern {
        let notes = notes.iter().map(|&(step, velocity)| Note { step, velocity, ..Note::default() });
        Pattern {
            tracks: vec![Track { steps_per_beat: 4, notes: notes.collect(), ..Track::default() }],
            ..Pattern::default()
        }
    }

    const HOW: Humanize = Humanize { timing_steps: 0.1, velocity: 0.2 };

    #[test]
    fn notes_move_within_the_ranges_and_keep_their_steps() {
        let mut p = pattern(&[(0, 0.8), (1, 0.3), (2, 1.0), (3, 0.5)]);
        assert_eq!(humanize(&mut p, Scope::Pattern, HOW, &mut Rng::new(7)), 4);
        let before = [0.8, 0.3, 1.0, 0.5];
        for (n, (step, velocity)) in p.tracks[0].notes.iter().zip((0..).zip(before)) {
            assert_eq!(n.step, step);
            assert!(n.shift_steps.abs() <= 0.1, "{}", n.shift_steps);
            assert!((n.velocity - velocity).abs() <= velocity * 0.2 + 0.005, "{} from {velocity}", n.velocity);
            assert!(n.velocity <= 1.0);
        }
    }

    #[test]
    fn nothing_moves_at_zero_and_the_same_seed_gives_the_same_notes() {
        let mut p = pattern(&[(0, 0.8), (2, 0.4)]);
        let none = Humanize { timing_steps: 0.0, velocity: 0.0 };
        assert_eq!(humanize(&mut p, Scope::Pattern, none, &mut Rng::new(1)), 0);
        let mut q = p.clone();
        humanize(&mut p, Scope::Pattern, HOW, &mut Rng::new(3));
        humanize(&mut q, Scope::Pattern, HOW, &mut Rng::new(3));
        assert_eq!(p.tracks[0].notes, q.tracks[0].notes);
    }

    #[test]
    fn a_shift_near_the_edge_stays_within_half_a_step() {
        let mut p = pattern(&[(0, 0.8)]);
        p.tracks[0].notes[0].shift_steps = 0.48;
        for seed in 0..20 {
            humanize(&mut p, Scope::Pattern, Humanize { timing_steps: 0.5, velocity: 0.0 }, &mut Rng::new(seed));
            assert!(p.tracks[0].notes[0].shift_steps.abs() <= MAX_SHIFT_STEPS);
        }
    }

    #[test]
    fn only_the_scope_is_humanized() {
        let mut p = pattern(&[(0, 0.8)]);
        p.tracks.push(p.tracks[0].clone());
        assert_eq!(humanize(&mut p, Scope::Track(1), HOW, &mut Rng::new(5)), 1);
        assert_eq!(p.tracks[0].notes[0], Note { step: 0, velocity: 0.8, ..Note::default() });
    }
}
