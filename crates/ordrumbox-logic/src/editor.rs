//! The library being edited and every edit the app makes to it
//! (`src/logic/commands/`: note, track, pattern and song commands of the
//! `Commander`), each one an undo step. The song (arrangement) commands are
//! in `arrangement.rs`.
//!
//! Patterns, tracks and notes are addressed by index (the JS commands take
//! object references). An edit that changes nothing records nothing.

use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::time::Instant;

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family, mono_from_track_name};
use ordrumbox_core::ids::{slugify, unique_id};
use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::note::{ACCENT_VELOCITY, ACCENTED_VELOCITY};
use ordrumbox_core::num::round2;
use ordrumbox_core::song::{DEFAULT_BPM, clamp_bpm};
use ordrumbox_core::style::{COMPLEXITY, ENERGY, MACRO_NEUTRAL};
use ordrumbox_core::track::{MAX_STEPS_PER_BEAT, MAX_TRACKS};
use ordrumbox_core::{Library, Note, Pattern, Scales, SetField, Styles, Track};

use crate::block::{self, Block, Pasted, StepAt, Zone};
use crate::generate::{self, AutoGenerator, MelodicKind, Options};
use crate::history::{Change, History, Target};
use crate::midi;
use crate::roll_block::{self, RollBlock, RollZone};

#[path = "arrangement.rs"]
mod arrangement;

/// Longest pattern, beats (`MAX_BEATS`).
pub const MAX_BEATS: i32 = 16;
/// Default length of a pattern (`PATTERN_DEFAULTS`).
const DEFAULT_BEAT_COUNT: i32 = 4;
/// Step of a velocity nudge (`VELOCITY_STEP`).
pub const VELOCITY_STEP: f64 = 0.05;
/// Lowest and highest MIDI note of the piano roll, and middle C
/// (`piano_roll_constants.js`): the pitch nudge stays in that range.
const MIDI_MIN: f64 = 12.0;
const MIDI_MAX: f64 = 108.0;
const MIDDLE_C: f64 = 60.0;

/// What a nudge changes: ↑/↓ pitch, ←/→ one step in time (the velocity nudge
/// stays for the note editor and the drag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    VelocityUp,
    VelocityDown,
    PitchUp,
    PitchDown,
    /// One step later in the pattern (it wraps at its length).
    Later,
    /// One step earlier in the pattern (it wraps at its length).
    Earlier,
}

/// `nudgeVelocity()`: ±0.05, rounded to 2 decimals, unchanged past 0–1.
pub fn nudge_velocity(velocity: f64, up: bool) -> f64 {
    let current = if velocity.is_finite() { velocity } else { 0.8 };
    let next = round2(current + if up { VELOCITY_STEP } else { -VELOCITY_STEP });
    if (0.0..=1.0).contains(&next) { next } else { current }
}

/// `nudgePitch()`: ±1 semitone (rounded first), unchanged past the piano
/// roll's range once the track transposition is added.
pub fn nudge_pitch(pitch: f64, up: bool, track_pitch: f64) -> f64 {
    let current = if pitch.is_finite() { pitch.round() } else { 0.0 };
    let next = current + if up { 1.0 } else { -1.0 };
    let (min, max) = (MIDI_MIN - MIDDLE_C - track_pitch, MIDI_MAX - MIDDLE_C - track_pitch);
    if next < min || next > max { current } else { next }
}

/// Step position in percent of the beat.
pub(crate) fn step_percent(beat_step: i32, steps_per_beat: i32) -> f64 {
    (f64::from(beat_step) * 100.0 / f64::from(steps_per_beat.max(1))).round()
}

fn pattern_name(pattern: &Pattern) -> &str {
    pattern.name.as_deref().unwrap_or_default()
}

/// Set track settings by JSON key: only track settings, numbers clamped to
/// their range, non-finite numbers and wrong types ignored.
fn apply_updates(track: &mut Track, updates: &[(&str, Value)]) {
    for (key, value) in updates {
        if !Track::is_setting(key) {
            continue;
        }
        let value = match (value, Track::value_range(key)) {
            (Value::Number(n), _) if !n.is_finite() => continue,
            (Value::Number(n), Some((min, max))) => Value::Number(n.max(min).min(max)),
            _ => value.clone(),
        };
        // A value of the wrong type leaves the setting as it is.
        let _: SetField = track.set_field(key, &value);
    }
}

/// The library and its undo history.
#[derive(Debug)]
pub struct Editor {
    library: Library,
    history: History,
    /// The arrangement the song panel edits.
    selected_song: usize,
    /// Bumped by every change (edits, undo, redo).
    revision: u64,
    started: Instant,
    /// Fixed time for tests, ms.
    clock: Option<f64>,
    /// Inside [`Editor::batch`]: edits are not recorded one by one.
    batching: bool,
}

impl Editor {
    pub fn new(library: Library) -> Self {
        Self {
            library,
            history: History::default(),
            selected_song: 0,
            revision: 0,
            started: Instant::now(),
            clock: None,
            batching: false,
        }
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Changes so far: compare to know whether to redraw or save.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Use `ms` as the time of the next edits (coalescing), `None` for the real clock.
    pub fn set_clock(&mut self, ms: Option<f64>) {
        self.clock = ms;
    }

    fn now(&self) -> f64 {
        self.clock.unwrap_or_else(|| self.started.elapsed().as_secs_f64() * 1000.0)
    }

    /// Record `change` as an undo step, unless a batch is under way (it
    /// records one step for all its edits at its end).
    fn record(&mut self, desc: String, change: Change, coalesce: Option<String>) {
        if !self.batching {
            let now = self.now();
            self.history.record(desc, change, coalesce, now);
        }
    }

    /// Run `edits` as one undo step named `desc`, whatever edits it makes;
    /// none when they changed nothing. A batch inside a batch is part of it.
    pub fn batch<R>(&mut self, desc: &str, edits: impl FnOnce(&mut Self) -> R) -> R {
        if self.batching {
            return edits(self);
        }
        let before = Box::new(self.library.clone());
        let selected_before = self.selected_song;
        self.batching = true;
        let result = edits(self);
        self.batching = false;
        if *before != self.library {
            let change = Change::Library {
                before,
                after: Box::new(self.library.clone()),
                select: (selected_before, self.selected_song),
            };
            self.record(desc.to_owned(), change, None);
        }
        result
    }

    /// Undo the last step; what it did.
    pub fn undo(&mut self) -> Option<String> {
        let desc = self.history.undo(Target { library: &mut self.library, selected_song: &mut self.selected_song })?;
        self.revision += 1;
        Some(desc)
    }

    /// Redo the last undone step; what it does.
    pub fn redo(&mut self) -> Option<String> {
        let desc = self.history.redo(Target { library: &mut self.library, selected_song: &mut self.selected_song })?;
        self.revision += 1;
        Some(desc)
    }

    /// Run `edit` on pattern `p` and record it when it changed something.
    /// `None` when there is no such pattern.
    fn edit<R>(
        &mut self,
        p: usize,
        desc: impl FnOnce(&Pattern) -> String,
        coalesce: Option<String>,
        edit: impl FnOnce(&mut Pattern) -> R,
    ) -> Option<R> {
        let before = self.library.patterns.get(p)?.clone();
        let result = edit(&mut self.library.patterns[p]);
        let after = &self.library.patterns[p];
        if *after != before {
            let change = Change::Pattern { index: p, before: Box::new(before), after: Box::new(after.clone()) };
            let desc = desc(after);
            self.record(desc, change, coalesce);
            self.revision += 1;
        }
        Some(result)
    }

    /// Run `edit` on track `t` of pattern `p` (see [`Self::edit`]).
    fn edit_track<R>(
        &mut self,
        p: usize,
        t: usize,
        desc: impl FnOnce(&Track) -> String,
        coalesce: Option<String>,
        edit: impl FnOnce(&mut Track) -> R,
    ) -> Option<R> {
        self.library.patterns.get(p)?.tracks.get(t)?;
        self.edit(p, |pattern| desc(&pattern.tracks[t]), coalesce, |pattern| edit(&mut pattern.tracks[t]))
    }

    /// Run `edit` on the pattern list and record it when it changed something.
    fn edit_patterns<R>(&mut self, desc: String, edit: impl FnOnce(&mut Vec<Pattern>) -> R) -> R {
        let before = self.library.patterns.clone();
        let result = edit(&mut self.library.patterns);
        if self.library.patterns != before {
            let change = Change::Patterns { before, after: self.library.patterns.clone() };
            self.record(desc, change, None);
            self.revision += 1;
        }
        result
    }

    /// Run `edit` on the synth presets saved with the library (made when
    /// there were none) and record it as step `desc` when it changed
    /// something; edits with the same `coalesce` key close together are one
    /// step (a knob turned). Whether it changed something.
    pub fn edit_synths(&mut self, desc: String, coalesce: Option<String>, edit: impl FnOnce(&mut Object)) -> bool {
        let before = self.library.synths.clone();
        edit(self.library.synths.get_or_insert_default());
        if before.is_none() && self.library.synths.as_ref().is_some_and(Object::is_empty) {
            self.library.synths = None;
        }
        if self.library.synths == before {
            return false;
        }
        let change = Change::Synths { before, after: self.library.synths.clone() };
        self.record(desc, change, coalesce);
        self.revision += 1;
        true
    }
}

/// `cleanTrack`'s change.
fn clean(track: &mut Track) {
    track.notes.clear();
    track.loop_length = Some(track.beat_count * track.steps_per_beat);
}

mod generation;
mod notes;
mod patterns;
#[cfg(test)]
mod tests;
mod tracks;
