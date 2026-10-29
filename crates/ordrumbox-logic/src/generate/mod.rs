//! Pattern generators (`src/logic/generators/`): a line for a track from a
//! variant of its instrument's table (kick, snare, hats, clap, cowbell,
//! percussion, bass, chords/melody), whole patterns from a genre template,
//! and the loop-by-loop regeneration of tracks in auto mode.
//!
//! The variant tables are the JS ones (`data/generators.json`, exported by
//! `tools/export_gen_fixtures.mjs`) and are read the way the JS reads them,
//! key by key with its defaults. Every random draw is one `Math.random()` of
//! the JS, in the same order, through the `rnd` source the caller gives: with
//! the same stream the notes are the JS notes (`tests/generators.rs`).

mod auto;
mod drums;
mod melodic;
mod structure;

use std::sync::OnceLock;

use ordrumbox_core::js;
use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Note, Scales, Track};

pub use auto::{
    AutoGenerator, Harmony, MelodicKind, Options, change_track, generate_pattern, generate_track, instrument_line,
    is_auto_on, plays_auto, random_track, run_loop, toggle_drums, toggle_melodic,
};
pub use structure::{SectionElement, Structure};

/// A random source with the `Math.random()` contract: uniform in `[0, 1)`.
pub type Random<'a> = &'a mut dyn FnMut() -> f64;

/// The tables of `data/generators.json` (parsed once).
pub(crate) fn tables() -> &'static Value {
    static TABLES: OnceLock<Value> = OnceLock::new();
    TABLES.get_or_init(|| json::parse(include_str!("../../data/generators.json")).expect("data/generators.json parses"))
}

/// Table `name` of `data/generators.json` (`kick`, `structure`…).
pub(crate) fn table(name: &str) -> &'static Object {
    static EMPTY: OnceLock<Object> = OnceLock::new();
    tables().get(name).and_then(Value::as_object).unwrap_or_else(|| EMPTY.get_or_init(Object::new))
}

// ─── JS-style reads ──────────────────────────────────────────────────────────

/// `value[key]` when it is a number.
pub(crate) fn num(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub(crate) fn list<'a>(value: &'a Value, key: &str) -> Option<&'a [Value]> {
    value.get(key).and_then(Value::as_array)
}

/// The numbers of `value[key]` (non-numbers as NaN, like an arithmetic on them).
pub(crate) fn nums(value: &Value, key: &str) -> Option<Vec<f64>> {
    list(value, key).map(|items| items.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
}

/// `list[i % list.length]`, `None` for an empty list (`undefined` in JS).
pub(crate) fn cycle<T: Copy>(items: &[T], i: usize) -> Option<T> {
    if items.is_empty() { None } else { Some(items[i % items.len()]) }
}

/// `Math.min(max, Math.max(min, value))` (NaN stays NaN).
pub(crate) fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value.is_nan() { value } else { value.max(min).min(max) }
}

/// `pickRandom(list)`: one draw, none for an empty list.
pub(crate) fn pick<'a, T>(rnd: &mut dyn FnMut() -> f64, items: &'a [T]) -> Option<&'a T> {
    if items.is_empty() {
        return None;
    }
    let i = (rnd() * items.len() as f64).floor() as usize;
    items.get(i.min(items.len() - 1))
}

/// `NoteCommands.addNote` then the generator's velocity: a default note at
/// `beat`/`beat_step` with its `stepPercent` on the track's current grid.
pub(crate) fn add_note(track: &mut Track, beat: i32, beat_step: i32, pitch: f64, velocity: f64) -> &mut Note {
    let step_percent = js::round(f64::from(beat_step) * 100.0 / f64::from(track.steps_per_beat));
    track.notes.push(Note { velocity, beat, beat_step, pitch, step_percent: Some(step_percent), ..Note::default() });
    let last = track.notes.len() - 1;
    &mut track.notes[last]
}

/// `applyNoteProperties`: the engine fields a config or phrase sets on a note.
pub(crate) fn apply_note_properties(note: &mut Note, source: &Value) {
    if let Some(n) = num(source, "retriggerCount") {
        note.retrigger_count = n as i32;
    }
    if let Some(n) = num(source, "rate") {
        note.retrigger_rate = n;
    }
    if let Some(n) = num(source, "euclideanFill") {
        note.euclidean_pulses = n as i32;
    }
    if let Some(arp) = source.get("arp").filter(|a| !a.is_null()) {
        note.arp = arp.clone();
    }
    if let Some(n) = num(source, "prob") {
        note.probability = n;
    }
    if let Some(n) = num(source, "arpTriggerProbability") {
        note.arp_trigger_probability = n;
    }
}

// ─── Velocity ────────────────────────────────────────────────────────────────

/// The accent of a note (`context.accent`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) enum Accent {
    #[default]
    Off,
    /// Adds the config's `accentOnBeat`.
    On,
    /// A direct offset (a variant's `accentPattern`).
    By(f64),
}

impl From<bool> for Accent {
    fn from(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

/// `computeVelocity`'s context.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VelocitySpec {
    pub accent: Accent,
    pub ghost: bool,
    pub variation: bool,
    /// Replaces the config's `base`.
    pub base: Option<f64>,
    /// Round to 2 decimals (`toFixed`, default on).
    pub fixed: bool,
    /// Multiplies the sum before the clamp.
    pub scale: f64,
}

impl Default for VelocitySpec {
    fn default() -> Self {
        Self { accent: Accent::Off, ghost: false, variation: false, base: None, fixed: true, scale: 1.0 }
    }
}

impl VelocitySpec {
    pub fn accent(accent: impl Into<Accent>, ghost: bool) -> Self {
        Self { accent: accent.into(), ghost, ..Self::default() }
    }
}

// ─── The base generator ──────────────────────────────────────────────────────

/// `BaseGenerator`: what every instrument generator shares — its variant
/// table, the scales, the random source.
pub(crate) struct Generator<'a> {
    pub configs: &'static Object,
    pub scales: &'a Scales,
    pub rnd: &'a mut dyn FnMut() -> f64,
    /// Tones above it are taken an octave down (`getRndTone`).
    pub tone_threshold: i32,
}

impl<'a> Generator<'a> {
    pub fn new(table_name: &str, scales: &'a Scales, rnd: &'a mut dyn FnMut() -> f64) -> Self {
        Self { configs: table(table_name), scales, rnd, tone_threshold: 6 }
    }

    pub fn random(&mut self) -> f64 {
        (self.rnd)()
    }

    pub fn pick<'t, T>(&mut self, items: &'t [T]) -> Option<&'t T> {
        pick(&mut *self.rnd, items)
    }

    /// The config of variant `name`.
    pub fn config(&self, name: &str) -> Option<&'static Value> {
        self.configs.get(name)
    }

    /// `resolveVariantName`: `name` when the table has it, else a random one.
    pub fn resolve_variant(&mut self, name: Option<&str>) -> String {
        match name {
            Some(name) if !name.is_empty() && self.configs.contains_key(name) => name.to_owned(),
            _ => self.random_variant(&[]),
        }
    }

    /// `getRndVariantName`: a random variant, except `excluded` ones.
    pub fn random_variant(&mut self, excluded: &[&str]) -> String {
        let names: Vec<&str> = self.configs.iter().map(|(k, _)| k).filter(|k| !excluded.contains(k)).collect();
        self.pick(&names).map_or_else(|| "basic".to_owned(), |n| (*n).to_owned())
    }

    /// `computeVelocity`: base + accent + ghost + variation boost + a random
    /// spread (always drawn), scaled, clamped, rounded to 2 decimals.
    pub fn velocity(&mut self, config: Option<&Value>, context: VelocitySpec) -> f64 {
        let empty = Value::Object(Object::new());
        let vc = config.unwrap_or(&empty);
        let base = context.base.or_else(|| num(vc, "base")).unwrap_or(0.75);
        let accent = match context.accent {
            Accent::By(n) => n,
            Accent::On => num(vc, "accentOnBeat").unwrap_or(0.0),
            Accent::Off => 0.0,
        };
        let ghost = if context.ghost { num(vc, "ghost").unwrap_or(0.0) } else { 0.0 };
        let boost = if context.variation { num(vc, "variationBoost").unwrap_or(0.0) } else { 0.0 };
        let spread = num(vc, "randomSpread").unwrap_or(0.0);
        let offset = (self.random() * 2.0 - 1.0) * spread;
        let min = num(vc, "clampMin").unwrap_or(0.25);
        let max = num(vc, "clampMax").unwrap_or(1.0);
        let result = clamp((base + accent + ghost + boost + offset) * context.scale, min, max);
        if context.fixed { js::to_fixed(result, 2) } else { result }
    }

    /// `getScaleSteps`: the scale's degrees, or the major scale.
    pub fn scale_steps(&self, name: Option<&str>) -> Vec<f64> {
        let steps = name.and_then(|n| self.scales.get(n)).unwrap_or(&[0, 2, 4, 5, 7, 9, 11]);
        steps.iter().map(|&s| f64::from(s)).collect()
    }

    /// `getRndTone`: a random degree, an octave down above the threshold.
    pub fn random_tone(&mut self, tones: &[f64]) -> f64 {
        let tone = self.pick(tones).copied().unwrap_or(0.0);
        if tone > f64::from(self.tone_threshold) { tone - 12.0 } else { tone }
    }

    /// `resolvePhrasePitch`: the phrase's own pitch, else its source degree
    /// (`alternateSource` when its draw hits; `reuse` of an earlier pitch;
    /// `approach` from `approach_notes`), else a random tone; plus `bias`.
    pub fn phrase_pitch(
        &mut self,
        phrase: &Value,
        tones: &[f64],
        cached: &[f64],
        bias: f64,
        approach_notes: Option<&[f64]>,
    ) -> f64 {
        if let Some(pitch) = num(phrase, "pitch") {
            return pitch + bias;
        }
        let alternate = text(phrase, "alternateSource").filter(|s| !s.is_empty());
        let source = match alternate {
            Some(alt) if self.random() < num(phrase, "alternateChance").unwrap_or(0.35) => Some(alt),
            _ => text(phrase, "source"),
        };
        if source == Some("reuse")
            && let Some(index) = num(phrase, "reuseIndex")
        {
            let found = if index >= 0.0 && index.fract() == 0.0 { cached.get(index as usize) } else { None };
            return found.copied().unwrap_or(bias);
        }
        let offset = match source {
            Some("approach") => {
                let pool = approach_notes.filter(|p| !p.is_empty()).unwrap_or(&[-1.0, -2.0]);
                Some(self.pick(pool).copied().unwrap_or(0.0) + bias)
            }
            Some("root") => Some(bias),
            Some("third") => Some(4.0 + bias),
            Some("fifth") => Some(7.0 + bias),
            Some("seventh") => Some(11.0 + bias),
            Some("octave") => Some(12.0 + bias),
            _ => None,
        };
        offset.unwrap_or_else(|| self.random_tone(tones) + bias)
    }

    /// `getStepAdvance`: `average` steps, give or take `jitter` (never below 1).
    pub fn step_advance(&mut self, average: i32, jitter: i32) -> i32 {
        if jitter <= 0 {
            return average;
        }
        let mut choices = vec![average];
        if average - jitter >= 1 {
            choices.push(average - jitter);
        }
        choices.push(average + jitter);
        self.pick(&choices).copied().unwrap_or(average)
    }

    /// `generateGridVariant`: each step of the loop sounds with its column's
    /// probability (required steps always).
    pub fn grid(&mut self, track: &mut Track, config: &Value, density: f64, mut opts: Grid<'_>) {
        let loop_point = loop_point(track, config, opts.default_loop_beats);
        let spb = track.steps_per_beat;
        let required = list(config, "requiredSteps");
        let probabilities = nums(config, "probabilities");
        for beat in 0..track.beat_count {
            for step in 0..spb {
                if beat * spb + step >= loop_point {
                    continue;
                }
                let is_required = required.is_some_and(|r| is_required_step(beat, step, r));
                let probability = probabilities.as_deref().and_then(|p| cycle(p, step as usize)).unwrap_or(0.0);
                if !is_required && self.random() >= probability * density {
                    continue;
                }
                let accent = opts.accent.map_or(is_required || step == 0, |f| f(beat, step));
                let ghost = opts.ghost.map_or(!is_required && step != 0, |f| f(beat, step));
                let pitch = match opts.pitch.as_mut() {
                    Some(pitch) => pitch(self),
                    None => num(config, "pitch").unwrap_or(0.0),
                };
                let velocity = self.velocity(config.get("velocity"), VelocitySpec::accent(accent, ghost));
                apply_note_properties(add_note(track, beat, step, pitch, velocity), config);
            }
        }
    }

    /// `generatePhraseVariant`: the notes of a phrase skeleton (one of
    /// `phraseSets`, or `phrases`), each kept with `density` and its own
    /// `chance`; `step: "random"` takes a free step of its beat.
    pub fn phrases(&mut self, track: &mut Track, config: &Value, density: f64, opts: Phrases<'_>) {
        let loop_point = loop_point(track, config, opts.default_loop_beats);
        let spb = track.steps_per_beat;
        let phrases: &[Value] = match list(config, "phraseSets") {
            Some(sets) => self.pick(sets).and_then(Value::as_array).unwrap_or_default(),
            None => list(config, "phrases").unwrap_or_default(),
        };
        let mut occupied: Vec<(f64, Vec<i32>)> = Vec::new();
        let mut cached: Vec<f64> = Vec::new();
        let mut pitch_of = opts.pitch;
        for phrase in phrases {
            if density < 1.0 && self.random() >= density {
                continue;
            }
            if let Some(chance) = num(phrase, "chance")
                && self.random() >= chance
            {
                continue;
            }
            let beat_value = num(phrase, "beat").unwrap_or(f64::NAN);
            let step = if text(phrase, "step") == Some("random") {
                let taken: &[i32] = occupied.iter().find(|(b, _)| *b == beat_value).map_or(&[], |(_, s)| s);
                let free: Vec<i32> = (0..spb).filter(|s| !taken.contains(s)).collect();
                match self.pick(&free) {
                    Some(&s) => s,
                    None => continue,
                }
            } else {
                num(phrase, "step").unwrap_or(f64::NAN) as i32
            };
            let beat = beat_value as i32;
            if !opts.stacking && beat * spb + step >= loop_point {
                continue;
            }
            let pitch = match pitch_of.as_mut() {
                Some(pitch) => pitch(self, phrase, &cached),
                None => num(config, "pitch").unwrap_or(0.0),
            };
            let accent = opts.accent.map_or(phrase.get("accent") == Some(&Value::Bool(true)), |f| f(phrase, step));
            let ghost = opts.ghost.map_or(phrase.get("ghost") == Some(&Value::Bool(true)), |f| f(phrase, step));
            let velocity = self.velocity(config.get("velocity"), VelocitySpec::accent(accent, ghost));
            apply_note_properties(add_note(track, beat, step, pitch, velocity), phrase);
            cached.push(pitch);
            if !opts.stacking {
                match occupied.iter_mut().find(|(b, _)| *b == beat_value) {
                    Some((_, steps)) => steps.push(step),
                    None => occupied.push((beat_value, vec![step])),
                }
            }
        }
    }

    /// `buildContour`: `length` degrees of `scale` walked up, down, or up and down.
    pub fn contour(scale: &[f64], length: usize, contour: &str, start: f64, default: &[f64]) -> Vec<f64> {
        let scale = if scale.is_empty() { default } else { scale };
        let n = scale.len() as i64;
        let mut index = clamp(start, 0.0, (n - 1) as f64) as i64;
        let mut direction = 1;
        let mut sequence = Vec::with_capacity(length);
        for _ in 0..length {
            sequence.push(scale[index as usize]);
            match contour {
                "up" => index = (index + 1) % n,
                "down" => index = (index - 1 + n) % n,
                _ if n == 1 => {}
                _ => {
                    let next = index + direction;
                    if next >= n || next < 0 {
                        direction = -direction;
                        index += direction;
                    } else {
                        index = next;
                    }
                }
            }
        }
        sequence
    }
}

/// `getLoopPointAbsolute`: the loop length in steps (`loopBeats` beats).
pub(crate) fn loop_point(track: &Track, config: &Value, default_beats: f64) -> i32 {
    (num(config, "loopBeats").unwrap_or(default_beats) * f64::from(track.steps_per_beat)) as i32
}

/// `applyLoopPoint`: the track loops after `loopBeats` beats (its length by default).
pub(crate) fn apply_loop_point(track: &mut Track, config: &Value) {
    let beats = num(config, "loopBeats").unwrap_or(f64::from(track.beat_count));
    track.loop_length = Some((beats * f64::from(track.steps_per_beat)) as i32);
}

/// `#isRequiredStep`: `step` of a beat matching a `{beatModulo, step}` rule.
fn is_required_step(beat: i32, step: i32, rules: &[Value]) -> bool {
    rules.iter().any(|rule| {
        // `beatModulo === undefined || beat % beatModulo === beatModulo - 1`
        let beat_matches = match rule.get("beatModulo") {
            None => true,
            Some(m) => m.as_f64().is_some_and(|m| f64::from(beat) % m == m - 1.0),
        };
        beat_matches && num(rule, "step") == Some(f64::from(step))
    })
}

/// A test on a step (`beat`, `step`).
pub(crate) type StepTest<'f> = &'f dyn Fn(i32, i32) -> bool;
/// A test on a phrase and the step it lands on.
pub(crate) type PhraseTest<'f> = &'f dyn Fn(&Value, i32) -> bool;
/// The pitch of a grid step.
pub(crate) type StepPitch<'f> = &'f mut dyn FnMut(&mut Generator<'_>) -> f64;
/// The pitch of a phrase, given the pitches so far.
pub(crate) type PhrasePitch<'f> = &'f mut dyn FnMut(&mut Generator<'_>, &Value, &[f64]) -> f64;

/// Options of [`Generator::grid`].
#[derive(Default)]
pub(crate) struct Grid<'f> {
    pub default_loop_beats: f64,
    pub accent: Option<StepTest<'f>>,
    pub ghost: Option<StepTest<'f>>,
    pub pitch: Option<StepPitch<'f>>,
}

/// Options of [`Generator::phrases`].
#[derive(Default)]
pub(crate) struct Phrases<'f> {
    pub default_loop_beats: f64,
    pub pitch: Option<PhrasePitch<'f>>,
    pub accent: Option<PhraseTest<'f>>,
    pub ghost: Option<PhraseTest<'f>>,
    /// Notes may share a step and pass the loop point (chords).
    pub stacking: bool,
}

impl Grid<'_> {
    pub fn beats(default_loop_beats: f64) -> Self {
        Self { default_loop_beats, ..Self::default() }
    }
}

/// `withTemporaryStepsPerBeat`: run `f` with the track's grid at `steps`
/// clamped to 1–8 (so 16 is 8), then put the grid back.
pub(crate) fn with_steps_per_beat<R>(track: &mut Track, steps: i32, f: impl FnOnce(&mut Track) -> R) -> R {
    let saved = track.steps_per_beat;
    track.steps_per_beat = steps.clamp(1, 8);
    let result = f(track);
    track.steps_per_beat = saved;
    result
}
