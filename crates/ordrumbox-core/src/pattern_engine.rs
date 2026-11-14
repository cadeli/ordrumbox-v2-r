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
use crate::note::{MIN_LENGTH_STEPS, Note};
use crate::pattern::Pattern;
use crate::rng::Rng;
use crate::scales::Scales;
use crate::steps::{TICK, absolute_step, position_to_tick, step_spacing, step_to_tick};
use crate::style::Macros;
use crate::track::Track;
use crate::variation;

/// One note to play, as the player and the exporters read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Index of the track in `pattern.tracks`.
    pub track: usize,
    /// Index of the written note in `track.notes` that produced it; `None` for
    /// a note the variation layer added.
    pub source: Option<usize>,
    /// Grid position of that note (a repeat or an arpeggio note keeps the
    /// position of its source note: swing is decided by it).
    pub beat: i32,
    pub step: i32,
    /// Semitones, arpeggio offset included (track pitch not included).
    pub pitch: f64,
    pub velocity: f64,
    pub pan: f64,
    /// How long it is held, in steps of its track ([`Note::held_steps`]).
    pub length_steps: f64,
}

/// The hits of one pass (the notes expanded into what plays), by tick (a beat is [`TICK`] ticks).
pub type Hits = BTreeMap<u32, Vec<Hit>>;

/// Length of one pattern pass, in ticks.
pub fn pattern_ticks(pattern: &Pattern) -> i64 {
    i64::from(TICK) * i64::from(pattern.beat_count)
}

/// Length of one loop of a track, in steps: `loopAtStep` when set, else the
/// track's own `beatCount`.
pub fn track_loop_steps(track: &Track) -> i64 {
    match track.loop_length {
        Some(steps) if steps > 0 => i64::from(steps),
        _ => i64::from(track.beat_count) * i64::from(track.steps_per_beat),
    }
}

/// Does a note with this `cycle`/`cycle_offset` fire on pass `pass`? It fires
/// once every `cycle` passes, on the pass where `(pass + offset) % cycle == 0`.
/// A `cycle` of 0 never fires.
pub fn fires_on_pass(offset: i32, cycle: i32, pass: u32) -> bool {
    if cycle == 0 {
        return false;
    }
    let cycle = i64::from(cycle);
    (i64::from(pass) + i64::from(offset) % cycle) % cycle == 0
}

/// Compute pass `pass` (0-based, it selects the notes by `cycle`/`cycle_offset`) of a
/// pattern. Within a tick, notes come in track order,
/// then note order, then repeats, as in the JS app.
pub fn expand_hits(pattern: &Pattern, pass: u32, rng: &mut Rng, scales: &Scales) -> Hits {
    let mut flat = Hits::new();
    let pattern_ticks = pattern_ticks(pattern);
    let macro_factor = Macros::of(pattern).variation_factor();
    for (track_index, track) in pattern.tracks.iter().enumerate() {
        if track.steps_per_beat <= 0 {
            continue; // no grid: JS computes NaN ticks for every note
        }
        let loop_steps = track_loop_steps(track);
        let varied = variation::vary_notes(track, macro_factor, rng);
        let span_end = SpanResolver::new(track);
        // Probability is a manual-mode control: in auto mode only note.probability gates.
        let track_probability = if track.auto_mode { 1.0 } else { track.probability };

        for (note_index, source) in track.notes.iter().enumerate() {
            let note = varied.get(&note_index).unwrap_or(source);
            if !fires_on_pass(note.cycle_offset, note.cycle, pass) || !rng.chance(track_probability * note.probability)
            {
                continue;
            }
            let step = absolute_step(note.beat, note.step, track.steps_per_beat);
            if step_to_tick(step, track.steps_per_beat) >= pattern_ticks {
                continue;
            }
            let late_ticks = position_to_tick(note.late_steps(), track.steps_per_beat);
            let ctx = NoteContext { track_index, track, note, note_index, pattern_ticks, late_ticks };
            for at in loop_occurrences(step, loop_steps, track.steps_per_beat, pattern_ticks) {
                ctx.add_sub_notes(&mut flat, at, rng);
                ctx.add_euclidean_fill(&mut flat, at, &span_end, rng);
            }
        }
        variation::apply(&mut flat, track_index, track, (loop_steps, pattern_ticks), macro_factor, rng, scales);
    }
    for (track_index, track) in pattern.tracks.iter().enumerate().filter(|(_, t)| t.legato && t.steps_per_beat > 0) {
        extend_to_next_note(&mut flat, track_index, track.steps_per_beat, pattern_ticks);
    }
    flat
}

/// Make every hit of `track_index` last until the next tick where that track
/// plays (the last ones, until the pass end): a legato line.
fn extend_to_next_note(flat: &mut Hits, track_index: usize, steps_per_beat: i32, pattern_ticks: i64) {
    let step_ticks = f64::from(TICK) / f64::from(steps_per_beat);
    let mut next_tick = pattern_ticks;
    for (&tick, hits) in flat.iter_mut().rev() {
        let mut played = false;
        for hit in hits.iter_mut().filter(|h| h.track == track_index) {
            hit.length_steps = ((next_tick - i64::from(tick)) as f64 / step_ticks).max(MIN_LENGTH_STEPS);
            played = true;
        }
        if played {
            next_tick = i64::from(tick);
        }
    }
}

/// Absolute steps where a note at `step` sounds: repeated every loop when it
/// sits inside the loop, once otherwise. Each one is turned into a tick from
/// its own step, so a loop of a fractional tick length (4 steps at 5 a beat,
/// 76.8 ticks) does not drift.
fn loop_occurrences(step: i64, loop_steps: i64, steps_per_beat: i32, pattern_ticks: i64) -> Vec<i64> {
    let tick = |step: i64| step_to_tick(step, steps_per_beat);
    if step >= loop_steps || loop_steps <= 0 || tick(loop_steps) >= pattern_ticks {
        return vec![step];
    }
    (0..).map(|i| step + i * loop_steps).take_while(|&s| tick(s) < pattern_ticks).collect()
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
            occupied: track.notes.iter().map(|n| absolute_step(n.beat, n.step, spb)).collect(),
            last: i64::from(spb) * i64::from(track.beat_count),
            loop_at: track.loop_length.map(i64::from),
            steps_per_beat: spb,
        }
    }

    pub fn end(&self, note: &Note) -> i64 {
        let first = absolute_step(note.beat, note.step, self.steps_per_beat);
        let end = (first + 1..self.last).find(|step| self.occupied.contains(step)).unwrap_or(self.last);
        match self.loop_at {
            Some(loop_at) if loop_at > first && loop_at < end => loop_at,
            _ => end,
        }
    }
}

/// One written (or varied) note being expanded.
struct NoteContext<'a> {
    track_index: usize,
    track: &'a Track,
    note: &'a Note,
    note_index: usize,
    pattern_ticks: i64,
    /// How late every hit of the note is, in ticks (its `shiftSteps`).
    late_ticks: i64,
}

impl NoteContext<'_> {
    fn push(&self, flat: &mut Hits, tick: i64, pitch_offset: f64) {
        self.push_at(flat, tick, pitch_offset, self.note.velocity);
    }

    /// A hit at `tick` and the note's shift: one shifted before the pattern
    /// start plays on its first tick, one shifted past its end is left out.
    fn push_at(&self, flat: &mut Hits, tick: i64, pitch_offset: f64, velocity: f64) {
        let Ok(tick) = u32::try_from(tick) else { return }; // before the pattern start
        let shifted = (i64::from(tick) + self.late_ticks).max(0);
        if shifted >= self.pattern_ticks {
            return;
        }
        let Ok(tick) = u32::try_from(shifted) else { return };
        flat.entry(tick).or_default().push(Hit {
            track: self.track_index,
            source: Some(self.note_index),
            beat: self.note.beat,
            step: self.note.step,
            pitch: self.note.pitch + pitch_offset,
            velocity,
            pan: self.note.pan,
            length_steps: self.note.held_steps(),
        });
    }

    /// Tick of the `i`-th repeat / arpeggio note of the note sounding at
    /// absolute step `at`: rounded from its exact place, so the repeats keep
    /// their spacing where a step is a fractional number of ticks.
    fn repeat_tick(&self, at: i64, i: usize) -> i64 {
        let offset = i as f64 * step_spacing(self.note.retrigger_rate);
        position_to_tick(at as f64 + offset, self.track.steps_per_beat)
    }

    /// The note sounding at absolute step `at` and its repeats, or its
    /// arpeggio. Every arpeggio note, the first included, and every repeat
    /// after the first pass `arpTriggerProbability`; the repeats fade by
    /// `retriggerFade`.
    fn add_sub_notes(&self, flat: &mut Hits, at: i64, rng: &mut Rng) {
        let note = self.note;
        let trigger = note.retrigger_probability;
        if let Some(sequence) = arp::sequence(&note.arp, rng) {
            for i in 0..arp::note_count(note.retrigger_count) {
                if rng.chance(trigger) {
                    let tick = self.repeat_tick(at, i);
                    if tick < self.pattern_ticks {
                        self.push(flat, tick, sequence[i % sequence.len()]);
                    }
                }
            }
        } else {
            self.push(flat, step_to_tick(at, self.track.steps_per_beat), 0.0);
            for i in 1..note.retrigger_count.max(1) as usize {
                let tick = self.repeat_tick(at, i);
                if tick < self.pattern_ticks && rng.chance(trigger) {
                    self.push_at(flat, tick, 0.0, note.velocity * note.retrigger_gain(i));
                }
            }
        }
    }

    /// Euclidean onsets over the span to the next note, for the note sounding
    /// at absolute step `at`; with an arpeggio they continue its sequence
    /// after the arpeggio notes.
    fn add_euclidean_fill(&self, flat: &mut Hits, at: i64, span_end: &SpanResolver, rng: &mut Rng) {
        let note = self.note;
        if note.euclidean_pulses <= 0 {
            return;
        }
        let sequence = arp::sequence(&note.arp, rng);
        let spb = self.track.steps_per_beat;
        let start = absolute_step(note.beat, note.step, spb);
        let positions = euclidean::fill_positions(
            start,
            span_end.end(note) - start,
            note.euclidean_pulses,
            note.euclidean_rotation,
        );
        for (i, step) in positions.into_iter().enumerate() {
            // From the onset's own step, as a note written there would be.
            let tick = step_to_tick(at + step - start, spb);
            if tick >= self.pattern_ticks || !rng.chance(note.retrigger_probability) {
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
        let flat = expand_hits(pattern, pass, &mut Rng::new(0), &Scales::default());
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
        assert_eq!(ticks(&p, 0), [(0, 0.0), (24, 0.0), (48, 0.0), (72, 0.0)]);
        let p = pattern(r#"{"tracks":[{"notes":[{"retriggerCount":3,"rate":4}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (12, 0.0), (24, 0.0)]);
    }

    #[test]
    fn repeats_fade_towards_silence() {
        let velocities = |text: &str| {
            let p = pattern(text);
            let flat = expand_hits(&p, 0, &mut Rng::new(0), &Scales::default());
            flat.values().flatten().map(|hit| (hit.velocity * 100.0).round() / 100.0).collect::<Vec<_>>()
        };
        let notes = r#"{"tracks":[{"notes":[{"velocity":1,"retriggerCount":4,"rate":8"#;
        assert_eq!(velocities(&format!("{notes}}}]}}]}}")), [1.0, 0.85, 0.7, 0.55]);
        assert_eq!(velocities(&format!(r#"{notes},"retriggerFade":0}}]}}]}}"#)), [1.0; 4]);
        assert_eq!(velocities(&format!(r#"{notes},"retriggerFade":1}}]}}]}}"#)), [1.0, 0.75, 0.5, 0.25]);
        // An arpeggio does not fade.
        assert_eq!(velocities(&format!(r#"{notes},"arp":[0,4]}}]}}]}}"#)), [1.0; 4]);
    }

    #[test]
    fn notes_inside_the_loop_repeat() {
        let p = pattern(r#"{"tracks":[{"loopAtStep":4,"notes":[{"beatStep":1},{"beat":2}]}]}"#);
        assert_eq!(ticks(&p, 0), [(24, 0.0), (120, 0.0), (192, 0.0), (216, 0.0), (312, 0.0)]);
    }

    #[test]
    fn triplets_fall_on_whole_ticks() {
        let p = pattern(r#"{"tracks":[{"stepsPerBeat":3,"notes":[{"retriggerCount":4,"rate":8}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (32, 0.0), (64, 0.0), (96, 0.0)]);
        let p = pattern(r#"{"tracks":[{"stepsPerBeat":6,"notes":[{"step":5},{"beat":1,"step":1}]}]}"#);
        assert_eq!(ticks(&p, 0), [(80, 0.0), (112, 0.0)]);
    }

    #[test]
    fn a_loop_of_a_fractional_tick_length_does_not_drift() {
        // 4 steps at 5 a beat last 76.8 ticks: each repeat is rounded from
        // its own step (0, 77, 154, 230, 307), not from a floored loop
        // (0, 76, 152, 228, 304).
        let p = pattern(r#"{"tracks":[{"beatCount":4,"stepsPerBeat":5,"loopAtStep":4,"notes":[{}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (77, 0.0), (154, 0.0), (230, 0.0), (307, 0.0)]);
    }

    #[test]
    fn repeats_of_a_fractional_tick_spacing_do_not_drift() {
        // A step at 5 a beat is 19.2 ticks: each repeat is rounded from its
        // own place (0, 19, 38, 58), not from a rounded spacing (0, 19, 38, 57).
        let p = pattern(r#"{"tracks":[{"stepsPerBeat":5,"notes":[{"retriggerCount":4,"rate":8}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (19, 0.0), (38, 0.0), (58, 0.0)]);
    }

    #[test]
    fn a_euclidean_onset_lands_where_a_written_note_would() {
        // 2 pulses over the 2 steps to the next note, at 5 a beat: the onset
        // at step 3 is tick 58, as a note written there, not 38 + 19.
        let p = pattern(r#"{"tracks":[{"stepsPerBeat":5,"notes":[{"step":2,"euclideanPulses":2},{"step":4}]}]}"#);
        assert_eq!(ticks(&p, 0), [(38, 0.0), (58, 0.0), (77, 0.0)]);
    }

    #[test]
    fn a_shift_moves_the_note_and_its_repeats_off_the_grid() {
        // A step of 24 ticks: a quarter of a step late is 6 ticks, early -6.
        let p = pattern(r#"{"tracks":[{"notes":[{"beat":1,"shiftSteps":0.25,"retriggerCount":2,"rate":8}]}]}"#);
        assert_eq!(ticks(&p, 0), [(102, 0.0), (126, 0.0)]);
        let p = pattern(r#"{"tracks":[{"notes":[{"beat":1,"shiftSteps":-0.25}]}]}"#);
        assert_eq!(ticks(&p, 0), [(90, 0.0)]);
        // Before the pattern start: its first tick. Past half a step: half a step.
        let p = pattern(r#"{"tracks":[{"notes":[{"shiftSteps":-0.25},{"beat":2,"shiftSteps":3}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, 0.0), (204, 0.0)]);
    }

    #[test]
    fn a_hit_is_held_as_long_as_its_note() {
        let p = pattern(r#"{"tracks":[{"notes":[{},{"beat":1,"lengthSteps":2.5},{"beat":2,"lengthSteps":0}]}]}"#);
        let flat = expand_hits(&p, 0, &mut Rng::new(0), &Scales::default());
        let lengths: Vec<f64> = flat.values().flatten().map(|hit| hit.length_steps).collect();
        assert_eq!(lengths, [1.0, 2.5, crate::note::MIN_LENGTH_STEPS]);
    }

    #[test]
    fn a_legato_track_holds_each_note_until_the_next() {
        let p = pattern(r#"{"tracks":[{"legato":true,"notes":[{},{"step":2},{"beat":2,"lengthSteps":0.5}]}]}"#);
        let flat = expand_hits(&p, 0, &mut Rng::new(0), &Scales::default());
        let lengths: Vec<f64> = flat.values().flatten().map(|hit| hit.length_steps).collect();
        // 4 steps a beat, a 4-beat pattern: the last note runs to step 16.
        assert_eq!(lengths, [2.0, 6.0, 8.0]);
    }

    #[test]
    fn arpeggio_offsets_pitch() {
        let p = pattern(r#"{"tracks":[{"notes":[{"pitch":-12,"arp":[0,4,7],"retriggerCount":4,"rate":8}]}]}"#);
        assert_eq!(ticks(&p, 0), [(0, -12.0), (24, -8.0), (48, -5.0), (72, -12.0)]);
    }

    #[test]
    fn auto_mode_ignores_the_track_probability() {
        let manual = pattern(r#"{"tracks":[{"probability":0,"notes":[{"beat":1}]}]}"#);
        assert!(ticks(&manual, 0).is_empty());
        let auto = pattern(
            r#"{"tracks":[{"probability":0,"auto":true,"auto_variation":0,"auto_variation2":0,"notes":[{"beat":1}]}]}"#,
        );
        assert_eq!(ticks(&auto, 0), [(TICK, 0.0)]);
    }

    #[test]
    fn certain_patterns_draw_no_randomness() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/song.json")).unwrap();
        let (library, _) = Library::from_json(&json::parse(&text).unwrap());
        let rock = library.pattern("rock").unwrap();
        let mut rng = Rng::new(9);
        let before = rng.clone().next_u64();
        expand_hits(rock, 0, &mut rng, &Scales::default());
        assert_eq!(rng.next_u64(), before);
    }
}
