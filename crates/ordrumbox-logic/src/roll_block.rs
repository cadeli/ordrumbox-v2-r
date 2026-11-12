//! A block of the piano roll: what a zone drawn over one track's notes holds,
//! a span of time and a span of keys, copied, cut and pasted as a piece. Its
//! time is in [`BEAT_UNITS`](crate::block::BEAT_UNITS), so that it pastes on
//! a track of other steps per beat too.

use std::ops::RangeInclusive;

use ordrumbox_core::{Note, Pattern, Track};

use crate::block::{Pasted, note_units, place_note, step_units};

/// The key a note of `track` shows on: semitones from middle C, the track's
/// transposition included.
#[must_use]
pub fn note_key(note: &Note, track: &Track) -> i32 {
    (track.transpose + note.pitch).round() as i32
}

/// A rectangle of a track's piano roll: the time from `start_units`
/// (included) to `end_units` (excluded), the keys from `low_key` to
/// `high_key` (both included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollZone {
    pub track: usize,
    pub start_units: i64,
    pub end_units: i64,
    pub low_key: i32,
    pub high_key: i32,
}

impl RollZone {
    /// The zone of track `t` from the cell `a` to the cell `b` (each an
    /// absolute step and a key), both included, whichever corners they are;
    /// `None` when `pattern` has no track `t`.
    #[must_use]
    pub fn between(pattern: &Pattern, t: usize, a: (i64, i32), b: (i64, i32)) -> Option<Self> {
        let step_length = step_units(pattern.tracks.get(t)?.steps_per_beat);
        Some(Self {
            track: t,
            start_units: a.0.min(b.0) * step_length,
            end_units: (a.0.max(b.0) + 1) * step_length,
            low_key: a.1.min(b.1),
            high_key: a.1.max(b.1),
        })
    }

    /// Whether a note starting at `units` on key `key` is inside.
    #[must_use]
    pub fn holds(&self, units: i64, key: i32) -> bool {
        (self.start_units..self.end_units).contains(&units) && (self.low_key..=self.high_key).contains(&key)
    }
}

/// The notes of a roll zone, each with its start from the zone's, and its
/// pitch from the zone's top key (the track's transposition included).
#[derive(Debug, Clone, PartialEq)]
pub struct RollBlock {
    pub notes: Vec<(i64, Note)>,
    pub length_units: i64,
    pub key_count: i32,
}

impl RollBlock {
    /// The notes of `pattern` inside `zone`.
    #[must_use]
    pub fn copy(pattern: &Pattern, zone: &RollZone) -> Self {
        let notes = pattern.tracks.get(zone.track).map_or_else(Vec::new, |track| {
            track
                .notes
                .iter()
                .filter_map(|n| {
                    let units = note_units(n, track.steps_per_beat);
                    zone.holds(units, note_key(n, track)).then(|| {
                        let pitch = track.transpose + n.pitch - f64::from(zone.high_key);
                        (units - zone.start_units, Note { pitch, ..n.clone() })
                    })
                })
                .collect()
        });
        Self { notes, length_units: zone.end_units - zone.start_units, key_count: zone.high_key - zone.low_key + 1 }
    }

    /// Notes in the block.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }
}

/// Remove the notes of `pattern` inside `zone`; how many.
pub fn delete_roll_zone(pattern: &mut Pattern, zone: &RollZone) -> usize {
    let Some(track) = pattern.tracks.get_mut(zone.track) else { return 0 };
    let (spb, transpose) = (track.steps_per_beat, track.transpose);
    let count = track.notes.len();
    track.notes.retain(|n| !zone.holds(note_units(n, spb), (transpose + n.pitch).round() as i32));
    count - track.notes.len()
}

/// Paste `block` on track `t` with its start at the start of the track's
/// (absolute) step `step` and its top on key `top_key`, replacing what the
/// track had under it. A note between two steps of the track goes to the
/// nearest one; a note past the track's end or off `keys` is left out.
pub fn paste_roll_block(
    pattern: &mut Pattern,
    t: usize,
    step: i64,
    top_key: i32,
    block: &RollBlock,
    keys: RangeInclusive<i32>,
) -> Pasted<RollZone> {
    let Some(track) = pattern.tracks.get_mut(t) else {
        return Pasted { dropped: block.note_count(), ..Pasted::default() };
    };
    let (spb, transpose) = (track.steps_per_beat, track.transpose);
    let start = step * step_units(spb);
    let zone = RollZone {
        track: t,
        start_units: start,
        end_units: start + block.length_units,
        low_key: top_key - block.key_count + 1,
        high_key: top_key,
    };
    track.notes.retain(|n| !zone.holds(note_units(n, spb), (transpose + n.pitch).round() as i32));
    let mut pasted = Pasted::default();
    for (offset, note) in &block.notes {
        let moved = Note { pitch: f64::from(top_key) + note.pitch - transpose, ..note.clone() };
        let placed = place_note(&moved, start + offset, spb, track.beat_count)
            .filter(|n| keys.contains(&((transpose + n.pitch).round() as i32)));
        let Some(placed) = placed else {
            pasted.dropped += 1;
            continue;
        };
        track.notes.push(placed);
        pasted.placed += 1;
    }
    let shown = RollZone { low_key: zone.low_key.max(*keys.start()), high_key: zone.high_key.min(*keys.end()), ..zone };
    pasted.zone = (shown.low_key <= shown.high_key).then_some(shown);
    pasted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BEAT_UNITS;

    const KEYS: RangeInclusive<i32> = -48..=48;

    /// BASS, 4 beats at 4 steps a beat, an octave down: C, E, G on beat 1,
    /// D on beat 2. LEAD, 4 beats at 3 steps a beat.
    fn pattern() -> Pattern {
        let mut pattern = Pattern::default();
        let mut bass = Track::new("BASS", 4, 4);
        bass.transpose = -12.0;
        for (beat, step, pitch) in [(0, 0, 0.0), (0, 0, 4.0), (0, 2, 7.0), (1, 0, 2.0)] {
            bass.notes.push(Note { beat, step, pitch, ..Note::default() });
        }
        pattern.tracks = vec![bass, Track::new("LEAD", 4, 3)];
        pattern
    }

    /// Beat, step and key of each note of track `t`.
    fn places(p: &Pattern, t: usize) -> Vec<(i32, i32, i32)> {
        let track = &p.tracks[t];
        let mut places: Vec<_> = track.notes.iter().map(|n| (n.beat, n.step, note_key(n, track))).collect();
        places.sort_unstable();
        places
    }

    #[test]
    fn a_zone_takes_its_steps_and_its_keys() {
        let p = pattern();
        // Steps 0 to 2, keys E to G (an octave down): two notes, not C nor D.
        let zone = RollZone::between(&p, 0, (2, -5), (0, -8)).unwrap();
        assert_eq!(zone, RollZone { track: 0, start_units: 0, end_units: 630, low_key: -8, high_key: -5 });
        let block = RollBlock::copy(&p, &zone);
        let notes: Vec<(i64, f64)> = block.notes.iter().map(|(o, n)| (*o, n.pitch)).collect();
        assert_eq!(notes, [(0, -3.0), (420, 0.0)], "pitches from the top key");
        assert_eq!((block.length_units, block.key_count), (630, 4));
        assert_eq!(RollZone::between(&p, 2, (0, 0), (0, 0)), None);
    }

    #[test]
    fn delete_takes_only_what_is_inside() {
        let mut p = pattern();
        let zone = RollZone::between(&p, 0, (0, -12), (4, -8)).unwrap();
        assert_eq!(delete_roll_zone(&mut p, &zone), 3, "C, E and D; G is above");
        assert_eq!(places(&p, 0), [(0, 2, -5)]);
    }

    #[test]
    fn paste_moves_the_notes_in_time_and_in_key_and_replaces_what_was_under() {
        let mut p = pattern();
        // The chord C-E-G of beat 1, pasted a fourth up on beat 3.
        let zone = RollZone::between(&p, 0, (0, -12), (3, -5)).unwrap();
        let block = RollBlock::copy(&p, &zone);
        p.tracks[0].notes.push(Note { beat: 2, step: 1, pitch: 10.0, ..Note::default() });
        let pasted = paste_roll_block(&mut p, 0, 8, 0, &block, KEYS);
        assert_eq!((pasted.placed, pasted.dropped), (3, 0));
        let beat_3: Vec<_> = places(&p, 0).into_iter().filter(|n| n.0 == 2).collect();
        assert_eq!(beat_3, [(2, 0, -7), (2, 0, -3), (2, 2, 0)], "the note at key -2 was under the block");
        assert_eq!(
            pasted.zone,
            Some(RollZone {
                track: 0,
                start_units: 2 * BEAT_UNITS,
                end_units: 3 * BEAT_UNITS,
                low_key: -7,
                high_key: 0
            })
        );
    }

    #[test]
    fn paste_on_another_track_keeps_the_keys_shown_and_drops_what_falls_out() {
        let mut p = pattern();
        let zone = RollZone::between(&p, 0, (0, -12), (7, -5)).unwrap();
        let block = RollBlock::copy(&p, &zone);
        // LEAD is not transposed: the top key -5 is pitch -5 there. From
        // beat 4: beat 1's notes fit, beat 2's D does not.
        let pasted = paste_roll_block(&mut p, 1, 9, -5, &block, KEYS);
        assert_eq!((pasted.placed, pasted.dropped), (3, 1));
        // Step 2 of BASS (420 units) is halfway between LEAD's steps 1 and 2: half rounds up.
        assert_eq!(places(&p, 1), [(3, 0, -12), (3, 0, -8), (3, 2, -5)]);
        // Keys off the keyboard are left out, and the zone stops at its edge.
        let pasted = paste_roll_block(&mut p, 1, 0, -45, &block, KEYS);
        assert_eq!((pasted.placed, pasted.dropped), (2, 2), "C and D fall under the keyboard");
        assert_eq!(pasted.zone.map(|z| (z.low_key, z.high_key)), Some((-48, -45)));
    }
}
