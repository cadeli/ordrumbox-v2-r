//! Where a note's sub-events fall (`patterns/note_positions.js`): its
//! retriggers or arpeggio steps, then its euclidean fills — what the piano
//! roll draws as ghosts and lights while playing.

use crate::euclidean;
use crate::js::round;
use crate::pattern_engine::SpanResolver;
use crate::rng::Rng;
use crate::steps::{absolute_step, step_spacing};
use crate::{Note, Track, arp};

/// What a sub-event comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubKind {
    /// A retrigger, or an arpeggio step.
    Retrigger,
    /// A euclidean fill.
    Euclidean,
}

/// One sub-event of a note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubPosition {
    /// Absolute step in the track.
    pub step: i64,
    pub kind: SubKind,
    /// The arpeggio's offset from the note, semitones (0 without one).
    pub pitch_offset: f64,
}

/// The sub-events of `note` before `total_steps` (`getNoteSubPositions`).
/// `span` is the track's (built once per pass); a `random` arpeggio draws
/// its order from `rng`.
pub fn sub_positions(
    note: &Note,
    track: &Track,
    total_steps: i64,
    span: &SpanResolver,
    rng: &mut Rng,
) -> Vec<SubPosition> {
    let base = absolute_step(note.beat, note.beat_step, track.steps_per_beat);
    let sequence = arp::sequence(&note.arp, rng);
    // The engine clamps the arpeggio's note count: the ghosts stay on the same steps.
    let count = match &sequence {
        Some(_) => arp::note_count(note.retrigger_count) as i64,
        None => i64::from(note.retrigger_count),
    };
    if sequence.is_none() && count <= 1 && note.euclidean_pulses <= 0 {
        return Vec::new();
    }
    let offset = |i: i64| sequence.as_ref().map_or(0.0, |s| s[(i as usize) % s.len()]);
    let spacing = step_spacing(note.retrigger_rate);
    let mut positions: Vec<SubPosition> = (1..count)
        .map(|i| SubPosition {
            step: round(base as f64 + i as f64 * spacing) as i64,
            kind: SubKind::Retrigger,
            pitch_offset: offset(i),
        })
        .filter(|p| p.step < total_steps)
        .collect();
    if note.euclidean_pulses > 0 {
        let fills =
            euclidean::fill_positions(base, span.end(note) - base, note.euclidean_pulses, note.euclidean_rotation);
        for (k, pos) in fills.into_iter().enumerate() {
            if pos < total_steps {
                positions.push(SubPosition {
                    step: pos,
                    kind: SubKind::Euclidean,
                    pitch_offset: offset(count + k as i64),
                });
            }
        }
    }
    positions
}
