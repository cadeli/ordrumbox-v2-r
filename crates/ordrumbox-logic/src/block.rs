//! A block of notes: what a zone drawn over the pattern grid holds, copied,
//! cut and pasted row by row, as in a spreadsheet. Tracks may have different
//! steps per beat, so a zone spans time in [`BEAT_UNITS`], not steps.

use ordrumbox_core::track::MAX_STEPS_PER_BEAT;
use ordrumbox_core::{Note, Pattern};

/// Units in a beat: a multiple of every steps per beat (1 to 8), so that
/// every step of every track starts on a whole unit.
pub const BEAT_UNITS: i64 = 840;

/// A step of a track: the corner of a zone, or where a block is pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepAt {
    pub track: usize,
    pub beat: i32,
    pub step: i32,
}

/// A rectangle of the grid: rows `first_track..=last_track`, and the time
/// from `start_units` (included) to `end_units` (excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    pub first_track: usize,
    pub last_track: usize,
    pub start_units: i64,
    pub end_units: i64,
}

impl Zone {
    /// The zone from the step `a` to the step `b`, both included, whichever
    /// corners they are; `None` when one of their tracks is not in `pattern`.
    #[must_use]
    pub fn between(pattern: &Pattern, a: StepAt, b: StepAt) -> Option<Self> {
        let span = |at: StepAt| {
            let track = pattern.tracks.get(at.track)?;
            let start = step_start_units(at.beat, at.step, track.steps_per_beat);
            Some((start, start + step_units(track.steps_per_beat)))
        };
        let (a_start, a_end) = span(a)?;
        let (b_start, b_end) = span(b)?;
        Some(Self {
            first_track: a.track.min(b.track),
            last_track: a.track.max(b.track),
            start_units: a_start.min(b_start),
            end_units: a_end.max(b_end),
        })
    }

    /// Whether a note starting at `units` on track `t` is inside.
    #[must_use]
    pub fn holds(&self, t: usize, units: i64) -> bool {
        (self.first_track..=self.last_track).contains(&t) && (self.start_units..self.end_units).contains(&units)
    }
}

/// Units in a step at `steps_per_beat` steps per beat.
#[must_use]
pub fn step_units(steps_per_beat: i32) -> i64 {
    BEAT_UNITS / i64::from(steps_per_beat.clamp(1, MAX_STEPS_PER_BEAT))
}

/// Where step `step` of beat `beat` starts, in units, at `steps_per_beat`.
#[must_use]
pub fn step_start_units(beat: i32, step: i32, steps_per_beat: i32) -> i64 {
    i64::from(beat) * BEAT_UNITS + i64::from(step) * step_units(steps_per_beat)
}

/// Where `note` starts, in units, at `steps_per_beat`.
#[must_use]
pub fn note_units(note: &Note, steps_per_beat: i32) -> i64 {
    step_start_units(note.beat, note.step, steps_per_beat)
}

/// The notes of a zone, row by row, each with its start from the zone's.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub rows: Vec<Vec<(i64, Note)>>,
    pub length_units: i64,
}

impl Block {
    /// The notes of `pattern` inside `zone`; a row for each of its tracks.
    #[must_use]
    pub fn copy(pattern: &Pattern, zone: &Zone) -> Self {
        let rows = (zone.first_track..=zone.last_track)
            .map(|t| {
                let Some(track) = pattern.tracks.get(t) else { return Vec::new() };
                track
                    .notes
                    .iter()
                    .filter_map(|n| {
                        let units = note_units(n, track.steps_per_beat);
                        zone.holds(t, units).then(|| (units - zone.start_units, n.clone()))
                    })
                    .collect()
            })
            .collect();
        Self { rows, length_units: zone.end_units - zone.start_units }
    }

    /// Notes in the block.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.rows.iter().map(Vec::len).sum()
    }
}

/// What a paste did: notes placed, and notes left out because their row or
/// their time fell past the pattern.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pasted {
    pub placed: usize,
    pub dropped: usize,
    /// The zone the block now covers (its rows that found a track).
    pub zone: Option<Zone>,
}

/// Remove the notes of `pattern` inside `zone`; how many.
pub fn delete_zone(pattern: &mut Pattern, zone: &Zone) -> usize {
    let mut removed = 0;
    for (t, track) in pattern.tracks.iter_mut().enumerate() {
        let spb = track.steps_per_beat;
        let count = track.notes.len();
        track.notes.retain(|n| !zone.holds(t, note_units(n, spb)));
        removed += count - track.notes.len();
    }
    removed
}

/// Paste `block` with its first row on track `at.track` and its start at
/// the start of step `at`: row `r` goes to track `at.track + r`, replacing
/// what that track had under the block. A note between two steps of its new
/// track goes to the nearest one.
pub fn paste_block(pattern: &mut Pattern, at: StepAt, block: &Block) -> Pasted {
    let Some(anchor) = pattern.tracks.get(at.track) else {
        return Pasted { dropped: block.note_count(), ..Pasted::default() };
    };
    let start = step_start_units(at.beat, at.step, anchor.steps_per_beat);
    let end = start + block.length_units;
    let mut pasted = Pasted::default();
    let mut last_track = None;
    for (r, row) in block.rows.iter().enumerate() {
        let Some(track) = pattern.tracks.get_mut(at.track + r) else {
            pasted.dropped += row.len();
            continue;
        };
        last_track = Some(at.track + r);
        let steps_per_beat = track.steps_per_beat;
        let units = step_units(steps_per_beat);
        let spb = BEAT_UNITS / units;
        let track_steps = i64::from(track.beat_count) * spb;
        track.notes.retain(|n| !(start..end).contains(&note_units(n, steps_per_beat)));
        for (offset, note) in row {
            // To the nearest step of this track (half a step rounds up).
            let step = (start + offset + units / 2).div_euclid(units);
            let place = (0..track_steps).contains(&step).then(|| step_place(step, spb)).flatten();
            let Some((beat, beat_step)) = place else {
                pasted.dropped += 1;
                continue;
            };
            let percent = crate::editor::step_percent(beat_step, steps_per_beat);
            track.notes.push(Note { beat, step: beat_step, step_percent: Some(percent), ..note.clone() });
            pasted.placed += 1;
        }
    }
    pasted.zone =
        last_track.map(|last_track| Zone { first_track: at.track, last_track, start_units: start, end_units: end });
    pasted
}

/// Step `step` from a track's start as its beat and its step in the beat.
fn step_place(step: i64, steps_per_beat: i64) -> Option<(i32, i32)> {
    Some((i32::try_from(step.div_euclid(steps_per_beat)).ok()?, i32::try_from(step.rem_euclid(steps_per_beat)).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Track;

    /// Two tracks of 4 beats: KICK at 4 steps a beat, HAT at 3.
    fn pattern() -> Pattern {
        let mut pattern = Pattern::default();
        let mut kick = Track::new("KICK", 4, 4);
        let mut hat = Track::new("HAT", 4, 3);
        for (beat, step) in [(0, 0), (0, 2), (1, 0), (2, 3)] {
            kick.notes.push(Note { beat, step, ..Note::default() });
        }
        for (beat, step) in [(0, 1), (1, 2), (3, 0)] {
            hat.notes.push(Note { beat, step, pitch: 2.0, ..Note::default() });
        }
        pattern.tracks = vec![kick, hat];
        pattern
    }

    fn places(track: &Track) -> Vec<(i32, i32)> {
        let mut places: Vec<_> = track.notes.iter().map(|n| (n.beat, n.step)).collect();
        places.sort_unstable();
        places
    }

    fn at(track: usize, beat: i32, step: i32) -> StepAt {
        StepAt { track, beat, step }
    }

    #[test]
    fn a_zone_spans_its_two_corners_whichever_they_are() {
        let p = pattern();
        let zone = Zone::between(&p, at(1, 1, 2), at(0, 0, 1)).unwrap();
        assert_eq!(zone, Zone { first_track: 0, last_track: 1, start_units: 210, end_units: 2 * BEAT_UNITS });
        assert_eq!(Zone::between(&p, at(0, 0, 0), at(2, 0, 0)), None, "no third track");
    }

    #[test]
    fn copy_keeps_each_row_and_the_offsets_in_time() {
        let p = pattern();
        // Beats 1 and 2 of both tracks.
        let zone = Zone::between(&p, at(0, 0, 0), at(1, 1, 2)).unwrap();
        let block = Block::copy(&p, &zone);
        let offsets: Vec<Vec<i64>> = block.rows.iter().map(|r| r.iter().map(|(o, _)| *o).collect()).collect();
        assert_eq!(offsets, [vec![0, 420, 840], vec![280, 840 + 560]]);
        assert_eq!(block.length_units, 2 * BEAT_UNITS);
        assert_eq!(block.note_count(), 5);
    }

    #[test]
    fn delete_removes_only_what_is_inside() {
        let mut p = pattern();
        let zone = Zone::between(&p, at(0, 0, 2), at(0, 1, 0)).unwrap();
        assert_eq!(delete_zone(&mut p, &zone), 2);
        assert_eq!(places(&p.tracks[0]), [(0, 0), (2, 3)]);
        assert_eq!(p.tracks[1].notes.len(), 3, "the other row is untouched");
    }

    #[test]
    fn paste_goes_row_by_row_and_replaces_what_was_under_it() {
        let mut p = pattern();
        let zone = Zone::between(&p, at(0, 0, 0), at(1, 0, 2)).unwrap();
        let block = Block::copy(&p, &zone);
        let pasted = paste_block(&mut p, at(0, 2, 0), &block);
        assert_eq!((pasted.placed, pasted.dropped), (3, 0));
        // KICK: its beat 3 (2.3 included) replaced by beat 1.
        assert_eq!(places(&p.tracks[0]), [(0, 0), (0, 2), (1, 0), (2, 0), (2, 2)]);
        // HAT: beat 3 had nothing; beat 1's note (0, 1) lands at (2, 1).
        assert_eq!(places(&p.tracks[1]), [(0, 1), (1, 2), (2, 1), (3, 0)]);
        assert!(p.tracks[1].notes.iter().all(|n| n.pitch == 2.0), "the notes keep their values");
        assert_eq!(pasted.zone, Some(Zone { first_track: 0, last_track: 1, start_units: 1680, end_units: 2520 }));
    }

    #[test]
    fn paste_drops_what_falls_past_the_tracks_or_their_end() {
        let mut p = pattern();
        let zone = Zone::between(&p, at(0, 0, 0), at(1, 1, 2)).unwrap();
        let block = Block::copy(&p, &zone);
        // From HAT's beat 4: the KICK row lands on HAT, the HAT row on no track.
        let pasted = paste_block(&mut p, at(1, 3, 0), &block);
        assert_eq!(pasted.placed, 2, "the KICK notes of beat 1 fit in beat 4, not the one of beat 2");
        assert_eq!(pasted.dropped, 1 + 2);
        assert_eq!(pasted.zone.map(|z| (z.first_track, z.last_track)), Some((1, 1)));
    }

    #[test]
    fn a_note_between_two_steps_goes_to_the_nearest() {
        let mut p = pattern();
        // HAT's (0, 1) is at 280 units: on KICK (210 a step), nearer to step 1 than to step 2.
        let zone = Zone::between(&p, at(1, 0, 1), at(1, 0, 1)).unwrap();
        let block = Block::copy(&p, &zone);
        p.tracks[0].notes.clear();
        let pasted = paste_block(&mut p, at(0, 1, 0), &block);
        assert_eq!(pasted.placed, 1);
        assert_eq!(places(&p.tracks[0]), [(1, 0)], "its offset from the zone's start is 0");
        let zone = Zone::between(&p, at(1, 0, 0), at(1, 0, 2)).unwrap();
        let block = Block::copy(&p, &zone);
        p.tracks[0].notes.clear();
        paste_block(&mut p, at(0, 1, 0), &block);
        assert_eq!(places(&p.tracks[0]), [(1, 1)], "280 units: step 1 (210) rather than step 2 (420)");
    }
}
