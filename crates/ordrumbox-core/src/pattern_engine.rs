//! The pattern engine (`src/patterns/pattern_engine.js`): one pass of a
//! pattern projected onto a flat timeline of ticks, with every repeat,
//! arpeggio note, euclidean fill and variation resolved.
//!
//! All randomness (probabilities, `random` arpeggios, variations) is drawn
//! from the [`Rng`] passed in: same seed, same pass. A pass whose notes are
//! all certain draws nothing.

use std::collections::{BTreeMap, HashSet};

use crate::arp;
use crate::euclidean;
use crate::js;
use crate::note::Note;
use crate::pattern::Pattern;
use crate::rng::Rng;
use crate::scales::Scales;
use crate::steps::{TICK, absolute_step, step_spacing, step_to_tick};
use crate::track::Track;
use crate::variation;

/// One note to play, as the player and the exporters read it.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatNote {
    /// Index of the track in `pattern.tracks`.
    pub track: usize,
    /// Index of the written note in `track.notes` that produced it; `None` for
    /// a note the variation layer added.
    pub source: Option<usize>,
    /// Grid position of that note (a repeat or an arpeggio note keeps the
    /// position of its source note: swing is decided by it).
    pub beat: i32,
    pub beat_step: i32,
    /// Semitones, arpeggio offset included (track pitch not included).
    pub pitch: f64,
    pub velocity: f64,
    pub pan: f64,
}

/// The flat notes of one pass, by tick (a beat is [`TICK`] ticks).
pub type FlatNotes = BTreeMap<u32, Vec<FlatNote>>;

/// Length of one pattern pass, in ticks.
pub fn pattern_ticks(pattern: &Pattern) -> i64 {
    i64::from(TICK) * i64::from(pattern.beat_count)
}

/// Length of one loop of a track, in ticks: `loopAtStep` when set, else the
/// track's own `beatCount` (`computeTickCountForLoop`).
pub fn track_loop_ticks(track: &Track) -> i64 {
    let spb = f64::from(track.steps_per_beat);
    let loop_steps = match track.loop_length {
        Some(steps) if steps > 0 => f64::from(steps),
        _ => f64::from(track.beat_count) * spb,
    };
    (loop_steps / spb * f64::from(TICK)).floor() as i64
}

/// Does a note with this `every`/`pos` fire on pass `pass`? It fires once every
/// `every` passes, on the pass where `(pass + pos) % every == 0`. `every` 0 never fires.
pub fn fires_on_pass(pos: i32, every: i32, pass: u32) -> bool {
    if every == 0 {
        return false;
    }
    let every = i64::from(every);
    (i64::from(pass) + i64::from(pos) % every) % every == 0
}

/// Compute pass `pass` (0-based, it selects the `every`/`pos` notes) of a
/// pattern (`recomputeFlatNotes`). Within a tick, notes come in track order,
/// then note order, then repeats, as in the JS app.
pub fn recompute_flat_notes(pattern: &Pattern, pass: u32, rng: &mut Rng, scales: &Scales) -> FlatNotes {
    let mut flat = FlatNotes::new();
    let pattern_ticks = pattern_ticks(pattern);
    for (track_idx, track) in pattern.tracks.iter().enumerate() {
        if track.steps_per_beat <= 0 {
            continue; // no grid: JS computes NaN ticks for every note
        }
        let loop_ticks = track_loop_ticks(track);
        let varied = variation::vary_notes(track, rng);
        let span_end = SpanResolver::new(track);
        // Probability is a manual-mode control: in auto mode only note.probability gates.
        let track_probability = if track.auto { 1.0 } else { track.probability };

        for (note_idx, source) in track.notes.iter().enumerate() {
            let note = varied.get(&note_idx).unwrap_or(source);
            if !fires_on_pass(note.every_offset, note.every, pass) || !rng.chance(track_probability * note.probability)
            {
                continue;
            }
            let base =
                step_to_tick(absolute_step(note.beat, note.beat_step, track.steps_per_beat), track.steps_per_beat);
            if base >= pattern_ticks {
                continue;
            }
            let ctx = NoteContext { track_idx, track, note, note_idx, pattern_ticks };
            for tick in loop_occurrences(base, loop_ticks, pattern_ticks) {
                ctx.add_sub_notes(&mut flat, tick, rng);
                ctx.add_euclidean_fill(&mut flat, tick, &span_end, rng);
            }
        }
        variation::apply(&mut flat, track_idx, track, loop_ticks, pattern_ticks, rng, scales);
    }
    flat
}

/// Ticks where a note at `base` sounds: repeated every loop when it sits
/// inside the loop, once otherwise (`expandLoopOccurrences`).
fn loop_occurrences(base: i64, loop_ticks: i64, pattern_ticks: i64) -> Vec<i64> {
    if base >= loop_ticks || loop_ticks <= 0 || loop_ticks >= pattern_ticks {
        return vec![base];
    }
    (0..).map(|i| base + i * loop_ticks).take_while(|&t| t < pattern_ticks).collect()
}

/// End (exclusive step) of the span a note's euclidean fill covers: the next
/// occupied step of the track, the loop point if it comes first, else the
/// track end (`step_resolver.js`). Built once per track and pass.
#[derive(Debug)]
pub struct SpanResolver {
    occupied: HashSet<i64>,
    last: i64,
    loop_at: Option<i64>,
    steps_per_beat: i32,
}

impl SpanResolver {
    pub fn new(track: &Track) -> Self {
        let spb = track.steps_per_beat;
        Self {
            occupied: track.notes.iter().map(|n| absolute_step(n.beat, n.beat_step, spb)).collect(),
            last: i64::from(spb) * i64::from(track.beat_count),
            loop_at: track.loop_length.map(i64::from),
            steps_per_beat: spb,
        }
    }

    pub fn end(&self, note: &Note) -> i64 {
        let first = absolute_step(note.beat, note.beat_step, self.steps_per_beat);
        let end = (first + 1..self.last).find(|step| self.occupied.contains(step)).unwrap_or(self.last);
        match self.loop_at {
            Some(loop_at) if loop_at > first && loop_at < end => loop_at,
            _ => end,
        }
    }
}

/// One written (or varied) note being expanded.
struct NoteContext<'a> {
    track_idx: usize,
    track: &'a Track,
    note: &'a Note,
    note_idx: usize,
    pattern_ticks: i64,
}

impl NoteContext<'_> {
    fn push(&self, flat: &mut FlatNotes, tick: i64, pitch_offset: f64) {
        let Ok(tick) = u32::try_from(tick) else { return }; // before the pattern start
        flat.entry(tick).or_default().push(FlatNote {
            track: self.track_idx,
            source: Some(self.note_idx),
            beat: self.note.beat,
            beat_step: self.note.beat_step,
            pitch: self.note.pitch + pitch_offset,
            velocity: self.note.velocity,
            pan: self.note.pan,
        });
    }

    /// Ticks between repeats / arpeggio notes for the note's `rate`.
    fn repeat_spacing(&self) -> i64 {
        let ticks_per_step = f64::from(TICK) / f64::from(self.track.steps_per_beat);
        js::round(ticks_per_step * step_spacing(self.note.retrigger_rate)) as i64
    }

    /// The note and its repeats, or its arpeggio (`generateSubNotes`). Every
    /// arpeggio note, the first included, and every repeat after the first
    /// pass `arpTriggerProbability`.
    fn add_sub_notes(&self, flat: &mut FlatNotes, base: i64, rng: &mut Rng) {
        let note = self.note;
        let spacing = self.repeat_spacing();
        let trigger = note.arp_trigger_probability;
        if let Some(sequence) = arp::sequence(&note.arp, rng) {
            for i in 0..arp::note_count(note.retrigger_count) {
                if rng.chance(trigger) {
                    let tick = base + i as i64 * spacing;
                    if tick < self.pattern_ticks {
                        self.push(flat, tick, sequence[i % sequence.len()]);
                    }
                }
            }
        } else {
            self.push(flat, base, 0.0);
            for i in 1..i64::from(note.retrigger_count.max(1)) {
                let tick = base + i * spacing;
                if tick < self.pattern_ticks && rng.chance(trigger) {
                    self.push(flat, tick, 0.0);
                }
            }
        }
    }

    /// Euclidean onsets over the span to the next note; with an arpeggio they
    /// continue its sequence after the arpeggio notes.
    fn add_euclidean_fill(&self, flat: &mut FlatNotes, base: i64, span_end: &SpanResolver, rng: &mut Rng) {
        let note = self.note;
        if note.euclidean_pulses <= 0 {
            return;
        }
        let sequence = arp::sequence(&note.arp, rng);
        let spb = self.track.steps_per_beat;
        let start = absolute_step(note.beat, note.beat_step, spb);
        let ticks_per_step = f64::from(TICK) / f64::from(spb);
        let positions = euclidean::fill_positions(
            start,
            span_end.end(note) - start,
            note.euclidean_pulses,
            note.euclidean_rotation,
        );
        for (i, step) in positions.into_iter().enumerate() {
            let tick = base + js::round((step - start) as f64 * ticks_per_step) as i64;
            if tick >= self.pattern_ticks || !rng.chance(note.arp_trigger_probability) {
                continue;
            }
            let offset = sequence.as_ref().map_or(0.0, |seq| {
                let index = arp::note_count(note.retrigger_count) + i;
                seq[index % seq.len()]
            });
            self.push(flat, tick, offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;
    use crate::library::Library;

    fn pattern(text: &str) -> Pattern {
        let (library, report) = Library::parse(&format!(r#"{{"patterns":[{text}]}}"#)).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        library.patterns.into_iter().next().unwrap()
    }

    fn ticks(pattern: &Pattern, pass: u32) -> Vec<(u32, f64)> {
        let flat = recompute_flat_notes(pattern, pass, &mut Rng::new(0), &Scales::default());
        flat.iter().flat_map(|(&tick, notes)| notes.iter().map(move |n| (tick, n.pitch))).collect()
    }

    #[test]
    fn every_and_pos_select_passes() {
        assert!(fires_on_pass(0, 1, 7));
        assert!(fires_on_pass(0, 2, 2) && !fires_on_pass(0, 2, 1));
        assert!(fires_on_pass(1, 2, 1) && !fires_on_pass(1, 2, 2));
        assert!(fires_on_pass(1, 4, 3) && fires_on_pass(1, 4, 7));
        assert!(!fires_on_pass(0, 0, 0));
    }

    #[test]
    fn retriggers_follow_the_rate_code() {
        let p = pattern(r#"{"tracks":[{"notes":[{"retriggerCount":4,"rate":8}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (8, 0.0), (16, 0.0), (24, 0.0)]);
        let p = pattern(r#"{"tracks":[{"notes":[{"retriggerCount":3,"rate":4}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (4, 0.0), (8, 0.0)]);
    }

    #[test]
    fn notes_inside_the_loop_repeat() {
        let p = pattern(r#"{"tracks":[{"loopAtStep":4,"notes":[{"beatStep":1},{"beat":2}]}]}"#);
        assert_eq!(ticks(&p, 0), [(8, 0.0), (40, 0.0), (64, 0.0), (72, 0.0), (104, 0.0)]);
    }

    #[test]
    fn arpeggio_offsets_pitch() {
        let p = pattern(r#"{"tracks":[{"notes":[{"pitch":-12,"arp":[0,4,7],"retriggerCount":4,"rate":8}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, -12.0), (8, -8.0), (16, -5.0), (24, -12.0)]);
    }

    #[test]
    fn auto_mode_ignores_the_track_probability() {
        let manual = pattern(r#"{"tracks":[{"probability":0,"notes":[{"beat":1}]}]}"#);
        assert!(ticks(&manual, 0).is_empty());
        let auto = pattern(
            r#"{"tracks":[{"probability":0,"auto":true,"auto_variation":0,"auto_variation2":0,"notes":[{"beat":1}]}]}"#,
        );
        assert_eq!(ticks(&auto, 0), [(32, 0.0)]);
    }

    #[test]
    fn certain_patterns_draw_no_randomness() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/song.json")).unwrap();
        let (library, _) = Library::from_json(&json::parse(&text).unwrap());
        let rock = library.pattern("rock").unwrap();
        let mut rng = Rng::new(9);
        let before = rng.clone().next_u64();
        recompute_flat_notes(rock, 0, &mut rng, &Scales::default());
        assert_eq!(rng.next_u64(), before);
    }
}
