//! The library being edited and every edit the app makes to it
//! (`src/logic/commands/`: note, track, pattern and song commands of the
//! `Commander`), each one an undo step. The song (arrangement) commands are
//! in `arrangement.rs`.
//!
//! Patterns, tracks and notes are addressed by index (the JS commands take
//! object references). An edit that changes nothing records nothing.

use std::collections::HashSet;
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

use crate::generate::{self, AutoGenerator, MelodicKind, Options};
use crate::history::{Change, History, Target};
use crate::midi;

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

/// What a nudge changes: ↑/↓ velocity, ←/→ pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    VelocityUp,
    VelocityDown,
    PitchUp,
    PitchDown,
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
fn step_percent(beat_step: i32, steps_per_beat: i32) -> f64 {
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

    // ─── Notes ───────────────────────────────────────────────────────────────

    /// Add a note (defaults) at `beat`/`beat_step`; its index in the track.
    pub fn add_note(&mut self, p: usize, t: usize, beat: i32, beat_step: i32, pitch: f64) -> Option<usize> {
        let pattern = pattern_name(self.library.patterns.get(p)?).to_owned();
        self.edit_track(
            p,
            t,
            |track| format!("Add note on {} in \"{pattern}\"", track.name),
            None,
            |track| {
                let percent = step_percent(beat_step, track.steps_per_beat);
                track.notes.push(Note { beat, step: beat_step, pitch, step_percent: Some(percent), ..Note::default() });
                track.notes.len() - 1
            },
        )
    }

    /// Delete the last note at `beat`/`beat_step` with that pitch.
    pub fn delete_note(&mut self, p: usize, t: usize, beat: i32, beat_step: i32, pitch: f64) -> bool {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else { return false };
        self.edit_track(
            p,
            t,
            |track| format!("Delete note on {} in \"{pattern}\"", track.name),
            None,
            |track| {
                let found = track.notes.iter().rposition(|n| n.beat == beat && n.step == beat_step && n.pitch == pitch);
                found.map(|i| track.notes.remove(i)).is_some()
            },
        )
        .unwrap_or(false)
    }

    /// Delete every note at `beat`/`beat_step` (one step; the JS key handler
    /// records one per note).
    pub fn delete_step(&mut self, p: usize, t: usize, beat: i32, beat_step: i32) -> bool {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else { return false };
        self.edit_track(
            p,
            t,
            |track| format!("Delete step on {} in \"{pattern}\"", track.name),
            None,
            |track| {
                let count = track.notes.len();
                track.notes.retain(|n| !(n.beat == beat && n.step == beat_step));
                track.notes.len() != count
            },
        )
        .unwrap_or(false)
    }

    /// Set note fields by JSON key; values of the wrong type
    /// are ignored. With `coalesce`, repeated edits of the same fields of the
    /// same note are one undo step.
    pub fn update_note(
        &mut self,
        p: usize,
        t: usize,
        n: usize,
        updates: &[(&str, Value)],
        desc: Option<&str>,
        coalesce: bool,
    ) -> bool {
        let mut keys: Vec<&str> = updates.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let coalesce = coalesce.then(|| format!("note:{p}:{t}:{n}:{}", keys.join(",")));
        self.edit_track(
            p,
            t,
            |track| desc.map_or_else(|| format!("Edit note on {}", track.name), str::to_owned),
            coalesce,
            |track| {
                let Some(note) = track.notes.get_mut(n) else { return false };
                let before = note.clone();
                for (key, value) in updates {
                    note.set_field(key, value);
                }
                // A note moved by hand has a new place: an older position in its beat must not come back.
                if (note.beat, note.step) != (before.beat, before.step) {
                    note.step_percent = None;
                }
                *note != before
            },
        )
        .unwrap_or(false)
    }

    /// Accent the notes at a step, or, when they all are, take the accent
    /// off (back to the default velocity), as one undo step. The new state:
    /// `Some(true)` accented, `None` when the step has no note.
    pub fn toggle_accent(&mut self, p: usize, t: usize, beat: i32, beat_step: i32) -> Option<bool> {
        let track = self.library.patterns.get(p)?.tracks.get(t)?;
        let at = |n: &Note| n.beat == beat && n.step == beat_step;
        let notes: Vec<&Note> = track.notes.iter().filter(|n| at(n)).collect();
        if notes.is_empty() {
            return None;
        }
        let accent = notes.iter().any(|n| n.velocity < ACCENT_VELOCITY);
        let velocity = if accent { ACCENTED_VELOCITY } else { Note::default().velocity };
        let desc = |track: &Track| format!("{} on {}", if accent { "Accent" } else { "Remove accent" }, track.name);
        self.edit_track(p, t, desc, None, |track| {
            track.notes.iter_mut().filter(|n| at(n)).for_each(|n| n.velocity = velocity);
            true
        })?;
        Some(accent)
    }

    /// Nudge note `n` (`applyNoteNudge`: velocity ±0.05, pitch ±1).
    pub fn nudge_note(&mut self, p: usize, t: usize, n: usize, nudge: Nudge) -> bool {
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        let Some(note) = track.notes.get(n) else { return false };
        let (key, value) = match nudge {
            Nudge::VelocityUp | Nudge::VelocityDown => {
                ("velocity", nudge_velocity(note.velocity, nudge == Nudge::VelocityUp))
            }
            Nudge::PitchUp | Nudge::PitchDown => {
                ("pitch", nudge_pitch(note.pitch, nudge == Nudge::PitchUp, track.transpose))
            }
        };
        let desc = format!("Edit note {key} on {}", track.name);
        self.update_note(p, t, n, &[(key, Value::Number(value))], Some(&desc), true)
    }

    /// Replace the notes at a step by copies of `notes`.
    pub fn paste_step_notes(&mut self, p: usize, t: usize, beat: i32, beat_step: i32, notes: &[Note]) -> bool {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else { return false };
        self.edit_track(
            p,
            t,
            |track| format!("Paste step on {} in \"{pattern}\"", track.name),
            None,
            |track| {
                track.notes.retain(|n| !(n.beat == beat && n.step == beat_step));
                let percent = step_percent(beat_step, track.steps_per_beat);
                track.notes.extend(notes.iter().map(|n| Note {
                    beat,
                    step: beat_step,
                    step_percent: Some(percent),
                    ..n.clone()
                }));
            },
        )
        .is_some()
    }

    // ─── Tracks ──────────────────────────────────────────────────────────────

    /// Whether pattern `p` can take one more track ([`MAX_TRACKS`]).
    pub fn has_track_room(&self, p: usize) -> bool {
        self.library.patterns.get(p).is_some_and(|pattern| pattern.tracks.len() < MAX_TRACKS)
    }

    /// Append a new track; its index. `None` when the pattern is full.
    pub fn add_track(&mut self, p: usize, name: &str, steps_per_beat: i32) -> Option<usize> {
        if !self.has_track_room(p) {
            return None;
        }
        let desc = format!("Add track {name}");
        self.edit(
            p,
            |_| desc,
            None,
            |pattern| {
                pattern.tracks.push(Track::new(name, pattern.beat_count, steps_per_beat));
                pattern.tracks.len() - 1
            },
        )
    }

    pub fn remove_track(&mut self, p: usize, t: usize) -> bool {
        let Some(name) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)).map(|t| t.name.clone()) else {
            return false;
        };
        self.edit(p, |_| format!("Remove track {name}"), None, |pattern| pattern.tracks.remove(t)).is_some()
    }

    /// Move track `from` so that it ends at index `to` (clamped), the others
    /// keeping their order; whether it moved.
    pub fn move_track(&mut self, p: usize, from: usize, to: usize) -> bool {
        let Some(pattern) = self.library.patterns.get(p) else { return false };
        let Some(name) = pattern.tracks.get(from).map(|t| t.name.clone()) else { return false };
        let to = to.min(pattern.tracks.len() - 1);
        if to == from {
            return false;
        }
        self.edit(
            p,
            |_| format!("Move track {name}"),
            None,
            |pattern| {
                let track = pattern.tracks.remove(from);
                pattern.tracks.insert(to, track);
            },
        )
        .is_some()
    }

    /// Insert a copy of `source` at `index` (clamped), named "`<name>` copy"
    /// (then "copy 2"…); its index. `None` when the pattern is full.
    pub fn paste_track(&mut self, p: usize, index: usize, source: &Track) -> Option<usize> {
        if !self.has_track_room(p) {
            return None;
        }
        let taken: HashSet<&str> = self.library.patterns.get(p)?.tracks.iter().map(|t| t.name.as_str()).collect();
        let mut name = format!("{} copy", source.name);
        let mut n = 2;
        while taken.contains(name.as_str()) {
            name = format!("{} copy {n}", source.name);
            n += 1;
        }
        let desc = format!("Paste track {name}");
        self.edit(
            p,
            |_| desc,
            None,
            |pattern| {
                let index = index.min(pattern.tracks.len());
                pattern.tracks.insert(index, Track { name, ..source.clone() });
                index
            },
        )
    }

    /// Set track settings by JSON key: only track settings,
    /// numbers clamped to their range, non-finite numbers and wrong types
    /// ignored. With `coalesce`, repeated edits of the same settings of the
    /// same track are one undo step.
    pub fn update_track(
        &mut self,
        p: usize,
        t: usize,
        updates: &[(&str, Value)],
        desc: Option<&str>,
        coalesce: bool,
    ) -> bool {
        let mut keys: Vec<&str> = updates.iter().map(|(k, _)| *k).filter(|k| Track::is_setting(k)).collect();
        keys.sort_unstable();
        let coalesce = coalesce.then(|| format!("track:{p}:{t}:{}", keys.join(",")));
        self.edit_track(
            p,
            t,
            |track| desc.map_or_else(|| format!("Update {}", track.name), str::to_owned),
            coalesce,
            |track| apply_updates(track, updates),
        )
        .is_some()
    }

    /// [`Self::update_track`] on every track of pattern `p`, `updates(t, track)`
    /// for each, as one undo step (the JS `recordTransaction`).
    pub fn update_tracks(
        &mut self,
        p: usize,
        desc: &str,
        mut updates: impl FnMut(usize, &Track) -> Vec<(&'static str, Value)>,
    ) -> bool {
        self.edit(
            p,
            |_| desc.to_owned(),
            None,
            |pattern| {
                for (t, track) in pattern.tracks.iter_mut().enumerate() {
                    let updates = updates(t, track);
                    apply_updates(track, &updates);
                }
            },
        )
        .is_some()
    }

    /// Flip a boolean setting (`mute`, `solo`…).
    pub fn toggle_track(&mut self, p: usize, t: usize, key: &str) -> bool {
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        let on = track.field(key).and_then(|v| v.as_bool()) != Some(true);
        let desc = format!("{key} on {}", track.name);
        self.update_track(p, t, &[(key, Value::Bool(on))], Some(&desc), false)
    }

    /// `setStepsPerBeat`: clamped to 1–8; notes keep their position in the
    /// beat (by percent), the loop point stays inside the track.
    pub fn set_steps_per_beat(&mut self, p: usize, t: usize, value: f64, coalesce: bool) -> bool {
        let target = value.clamp(1.0, MAX_STEPS_PER_BEAT as f64).round() as i32;
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        if !value.is_finite() || target == track.steps_per_beat {
            return false;
        }
        let coalesce = coalesce.then(|| format!("track:{p}:{t}:stepsPerBeat"));
        self.edit_track(
            p,
            t,
            |track| format!("Steps per beat on {}", track.name),
            coalesce,
            |track| {
                let old = track.steps_per_beat;
                track.steps_per_beat = target;
                for note in &mut track.notes {
                    let percent = note.step_percent(old);
                    note.step_percent = Some(percent);
                    note.step = ((percent / 100.0 * f64::from(target)).round() as i32).min(target - 1);
                }
                let max_steps = track.beat_count * target;
                if track.loop_length.is_some_and(|l| l > max_steps) {
                    track.loop_length = Some(max_steps);
                }
            },
        )
        .is_some()
    }

    /// Next grid: 1 → 2 … 8 → 1.
    pub fn incr_steps_per_beat(&mut self, p: usize, t: usize) -> bool {
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        let next = if track.steps_per_beat >= 8 { 1 } else { track.steps_per_beat + 1 };
        self.set_steps_per_beat(p, t, f64::from(next), false)
    }

    /// The loop one step shorter, wrapping to the track end.
    pub fn shorten_loop(&mut self, p: usize, t: usize) -> bool {
        self.edit_track(
            p,
            t,
            |track| format!("Shorten loop on {}", track.name),
            None,
            |track| {
                let full = track.steps_per_beat * track.beat_count;
                let next = track.loop_length.unwrap_or(full) - 1;
                track.loop_length = Some(if next < 1 { full } else { next });
            },
        )
        .is_some()
    }

    /// No notes, loop over the whole track.
    /// Shorten track `t` to its shortest loop, when its notes repeat one
    ///; whether it changed.
    pub fn compact_track(&mut self, p: usize, t: usize) -> bool {
        self.edit_track(p, t, |track| format!("Compact {}", track.name), None, Track::compact_loop).unwrap_or(false)
    }

    /// Pattern > Compact tracks (the tools panel's button): every track of
    /// pattern `p`, one undo step each; how many changed.
    pub fn compact_pattern(&mut self, p: usize) -> usize {
        let tracks = self.library.patterns.get(p).map_or(0, |pattern| pattern.tracks.len());
        (0..tracks).filter(|&t| self.compact_track(p, t)).count()
    }

    pub fn clean_track(&mut self, p: usize, t: usize) -> bool {
        self.edit_track(p, t, |track| format!("Clean {}", track.name), None, clean).is_some()
    }

    /// Every track cleaned, as one step (the JS records one per track).
    pub fn clean_pattern(&mut self, p: usize) -> bool {
        self.edit(
            p,
            |pattern| format!("Clean \"{}\"", pattern_name(pattern)),
            None,
            |pattern| pattern.tracks.iter_mut().for_each(clean),
        )
        .is_some()
    }

    /// Play sample `sample_id`: auto-assign and synth off.
    pub fn change_track_sound(&mut self, p: usize, t: usize, sample_id: &str) -> bool {
        self.edit_track(
            p,
            t,
            |track| format!("Sound on {}", track.name),
            None,
            |track| {
                track.sample_path = sample_id.to_owned();
                track.auto_assign = false;
                track.use_synth = false;
            },
        )
        .is_some()
    }

    /// Rename track `t`; a track renamed to a bass becomes mono.
    pub fn change_track_name(&mut self, p: usize, t: usize, name: &str) -> bool {
        self.edit_track(
            p,
            t,
            |_| format!("Rename track → {name}"),
            None,
            |track| {
                track.name = name.to_owned();
                track.mono |= mono_from_track_name(name);
            },
        )
        .is_some()
    }

    // ─── Patterns ────────────────────────────────────────────────────────────

    /// A new empty pattern (`createPattern`; default name `NewPat_<count>`)
    /// with an id no pattern of the library has.
    fn blank_pattern(&self, name: Option<&str>) -> Pattern {
        let name = name.map_or_else(|| format!("NewPat_{}", self.library.patterns.len()), str::to_owned);
        let mut taken: HashSet<String> = self.library.patterns.iter().map(|p| p.id.clone()).collect();
        Pattern {
            id: unique_id(&slugify(&name), &mut taken),
            name: Some(name),
            beat_count: DEFAULT_BEAT_COUNT,
            ..Pattern::default()
        }
    }

    /// Append an empty pattern (`addPattern`; default name `NewPat_<count>`)
    /// with a unique id; its index.
    pub fn add_pattern(&mut self, name: Option<&str>) -> usize {
        let pattern = self.blank_pattern(name);
        let desc = format!("Add pattern \"{}\"", pattern_name(&pattern));
        self.edit_patterns(desc, |patterns| {
            patterns.push(pattern);
            patterns.len() - 1
        })
    }

    /// Append a copy of pattern `p` named "`<name>` copy", with an id made
    /// from that name; its index.
    pub fn duplicate_pattern(&mut self, p: usize) -> Option<usize> {
        let source = self.library.patterns.get(p)?;
        let name = format!("{} copy", source.name.as_deref().unwrap_or("Pattern"));
        let mut taken: HashSet<String> = self.library.patterns.iter().map(|p| p.id.clone()).collect();
        let copy = Pattern { id: unique_id(&slugify(&name), &mut taken), name: Some(name.clone()), ..source.clone() };
        Some(self.edit_patterns(format!("Duplicate pattern → \"{name}\""), |patterns| {
            patterns.push(copy);
            patterns.len() - 1
        }))
    }

    /// Remove pattern `index`, unless it is the last one.
    pub fn remove_pattern(&mut self, index: usize) -> bool {
        if self.library.patterns.len() <= 1 || index >= self.library.patterns.len() {
            return false;
        }
        let name = pattern_name(&self.library.patterns[index]).to_owned();
        self.edit_patterns(format!("Remove pattern \"{name}\""), |patterns| patterns.remove(index));
        true
    }

    /// Move pattern `from` so that it ends at index `to` (clamped), the others
    /// keeping their order; whether it moved. Songs name patterns by id, so
    /// only the order of the list and of the arrangement rows changes.
    pub fn move_pattern(&mut self, from: usize, to: usize) -> bool {
        let Some(pattern) = self.library.patterns.get(from) else { return false };
        let to = to.min(self.library.patterns.len() - 1);
        if to == from {
            return false;
        }
        let desc = format!("Move pattern \"{}\"", pattern_name(pattern));
        self.edit_patterns(desc, |patterns| {
            let pattern = patterns.remove(from);
            patterns.insert(to, pattern);
        });
        true
    }

    /// Rename (trimmed; an empty name keeps the old one). The id stays.
    pub fn rename_pattern(&mut self, p: usize, name: &str) -> bool {
        let name = name.trim();
        self.edit(
            p,
            |pattern| format!("Rename pattern → \"{}\"", pattern_name(pattern)),
            None,
            |pattern| {
                if !name.is_empty() {
                    pattern.name = Some(name.to_owned());
                }
            },
        )
        .is_some()
    }

    /// The library's tempo, every pattern's (inside the tempo bounds; 0 or
    /// not a number sets the default 120). With `coalesce`, a run of changes
    /// is one step. Whether it changed.
    pub fn set_bpm(&mut self, bpm: f64, coalesce: bool) -> bool {
        let bpm = if bpm.is_finite() && bpm != 0.0 { clamp_bpm(bpm) } else { DEFAULT_BPM };
        let before = self.library.bpm;
        if bpm == before {
            return false;
        }
        self.library.bpm = bpm;
        let coalesce = coalesce.then(|| "bpm".to_owned());
        self.record(format!("Set BPM → {bpm}"), Change::Tempo { before, after: bpm }, coalesce);
        self.revision += 1;
        true
    }

    /// Length in beats, 1–16 (out of range: 4); every track follows and its
    /// loop point stays inside.
    pub fn set_pattern_beat_count(&mut self, p: usize, beats: f64) -> bool {
        let requested = beats.round();
        let applied = if requested.is_finite() && (1.0..=f64::from(MAX_BEATS)).contains(&requested) {
            requested as i32
        } else {
            DEFAULT_BEAT_COUNT
        };
        self.edit(
            p,
            |_| format!("Set pattern length → {applied} beats"),
            None,
            |pattern| {
                for track in &mut pattern.tracks {
                    let max_steps = applied * track.steps_per_beat;
                    if track.loop_length.is_some_and(|l| l > max_steps) {
                        track.loop_length = Some(max_steps);
                    }
                    track.beat_count = applied;
                }
                pattern.beat_count = applied;
            },
        )
        .is_some()
    }

    /// Tags (trimmed, empty ones left out).
    pub fn set_pattern_tags(&mut self, p: usize, tags: &[String]) -> bool {
        let tags: Vec<String> = tags.iter().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect();
        self.edit(
            p,
            |pattern| format!("Set tags on \"{}\"", pattern_name(pattern)),
            None,
            |pattern| pattern.tags = tags,
        )
        .is_some()
    }

    pub fn set_pattern_description(&mut self, p: usize, description: &str) -> bool {
        self.edit(
            p,
            |pattern| format!("Set description on \"{}\"", pattern_name(pattern)),
            None,
            |pattern| pattern.description = description.to_owned(),
        )
        .is_some()
    }
}

// ─── Generation ──────────────────────────────────────────────────────────────

impl Editor {
    /// Shift+B: a new pattern made in a style (style `style`, else a drawn
    /// one): its tracks, meter and lines, the library at the style's tempo,
    /// one undo step; its index.
    pub fn generate_pattern(
        &mut self,
        scales: &Scales,
        styles: &Styles,
        style: Option<&str>,
        rnd: &mut dyn FnMut() -> f64,
    ) -> usize {
        let mut pattern = self.blank_pattern(None);
        let options = Options { genre: style.map(str::to_owned), ..Options::default() };
        let mut g = AutoGenerator::new(scales, styles, rnd);
        generate::restyle_pattern(&mut g, &mut pattern, &options);
        let tempo = g.tempo;
        let desc = format!("Generate pattern \"{}\"", pattern_name(&pattern));
        self.batch(&desc.clone(), |e| {
            e.set_style_tempo(tempo);
            e.edit_patterns(desc, |patterns| {
                patterns.push(pattern);
                patterns.len() - 1
            })
        })
    }

    /// The library at the tempo a style drew, when it drew one.
    fn set_style_tempo(&mut self, tempo: Option<f64>) {
        if let Some(bpm) = tempo {
            self.set_bpm(bpm, false);
        }
    }

    /// ⟳ (B): pattern `p` made again in style `style` (else its own, else a
    /// drawn one), locked tracks kept, the library at the style's tempo; one
    /// undo step.
    pub fn regenerate_pattern(
        &mut self,
        p: usize,
        scales: &Scales,
        styles: &Styles,
        style: Option<&str>,
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let options = Options { genre: style.map(str::to_owned), ..Options::default() };
        let Some(pattern) = self.library.patterns.get(p) else { return false };
        let desc = format!("Generate \"{}\"", pattern_name(pattern));
        self.batch(&desc, |e| {
            let mut g = AutoGenerator::new(scales, styles, rnd);
            e.edit(
                p,
                |pattern| format!("Generate \"{}\"", pattern_name(pattern)),
                None,
                |pattern| {
                    generate::restyle_pattern(&mut g, pattern, &options);
                },
            );
            let tempo = g.tempo;
            e.set_style_tempo(tempo);
        });
        true
    }

    /// A generation macro of pattern `p` (`composeEnergy`…, 0–100) set to
    /// `value`, its lines made again from its seed; a drag is one undo step.
    pub fn set_pattern_macro(
        &mut self,
        p: usize,
        key: &'static str,
        value: f64,
        (scales, styles): (&Scales, &Styles),
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let value = if value.is_finite() { value.clamp(0.0, 100.0).round() } else { MACRO_NEUTRAL };
        let coalesce = Some(format!("pattern:{p}:{key}"));
        let label = match key {
            ENERGY => "energy",
            COMPLEXITY => "complexity",
            _ => "variation",
        };
        self.edit(
            p,
            |_| format!("Set {label} → {value}"),
            coalesce,
            |pattern| {
                pattern.extra.insert(key, Value::Number(value));
                generate::recompose(&mut AutoGenerator::new(scales, styles, rnd), pattern);
            },
        )
        .is_some()
    }

    /// AUTO: every unlocked track of pattern `p` in auto mode, or all out of
    /// it; whether auto mode is now on.
    pub fn toggle_pattern_auto(&mut self, p: usize) -> Option<bool> {
        let desc = |pattern: &Pattern| {
            let on = pattern.tracks.iter().any(|t| t.auto_mode);
            if on { "Auto on" } else { "Auto off" }.to_owned()
        };
        self.edit(p, desc, None, generate::toggle_auto)
    }

    /// ↻ Drum on pattern `p` (in style `style`, else the pattern's): whether
    /// drum generation is now on.
    pub fn toggle_drums(
        &mut self,
        p: usize,
        scales: &Scales,
        styles: &Styles,
        style: Option<&str>,
        rnd: &mut dyn FnMut() -> f64,
    ) -> Option<bool> {
        let desc = |pattern: &Pattern| {
            let on = pattern.tracks.iter().any(|t| instrument_family(&t.name).is_drum() && t.auto_mode);
            if on { "Generate drums" } else { "Drums auto off" }.to_owned()
        };
        let options = Options { genre: style.map(str::to_owned), ..Options::default() };
        self.edit(p, desc, None, |pattern| {
            generate::toggle_drums(&mut AutoGenerator::new(scales, styles, rnd), pattern, &options)
        })
    }

    /// ↻ on track `t` of pattern `p`: a new line in the pattern's style, one undo step.
    pub fn regenerate_track(
        &mut self,
        p: usize,
        t: usize,
        scales: &Scales,
        styles: &Styles,
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let name = match self.library.patterns.get(p).and_then(|pattern| pattern.tracks.get(t)) {
            Some(track) => track.name.clone(),
            None => return false,
        };
        self.edit(
            p,
            |_| format!("Generate {name}"),
            None,
            |pattern| generate::regenerate_track(&mut AutoGenerator::new(scales, styles, rnd), pattern, t),
        )
        .is_some()
    }

    /// ↻ Bass / ↻ Chords on pattern `p`: whether that
    /// generation is now on.
    pub fn toggle_melodic(
        &mut self,
        p: usize,
        kind: MelodicKind,
        scales: &Scales,
        styles: &Styles,
        rnd: &mut dyn FnMut() -> f64,
    ) -> Option<bool> {
        let (part, wanted) = match kind {
            MelodicKind::Bass => ("bass", InstrumentFamily::Bass),
            MelodicKind::Chords => ("chords", InstrumentFamily::Piano),
        };
        let desc = |pattern: &Pattern| {
            let on = pattern.tracks.iter().any(|t| instrument_family(&t.name) == wanted && t.auto_mode);
            if on { format!("Generate {part}") } else { format!("{part} auto off") }
        };
        self.edit(p, desc, None, |pattern| {
            generate::toggle_melodic(&mut AutoGenerator::new(scales, styles, rnd), pattern, kind)
        })
    }

    /// Patterns from the MIDI file `bytes` named `file_name`
    /// (`MidiImportService.importFile`), one undo step; `roots` are the
    /// samples' root notes. The index of the last new pattern and the
    /// message, or why nothing was imported.
    pub fn import_midi(
        &mut self,
        file_name: &str,
        bytes: &[u8],
        roots: &[(String, f64)],
    ) -> Result<(usize, String), String> {
        let imported = midi::import(file_name, bytes, roots)?;
        let index = self.edit_patterns("Import MIDI file".to_owned(), |patterns| {
            let mut taken: HashSet<String> = patterns.iter().map(|p| p.id.clone()).collect();
            for mut pattern in imported.patterns {
                pattern.id = unique_id(&slugify(pattern_name(&pattern)), &mut taken);
                patterns.push(pattern);
            }
            patterns.len() - 1
        });
        Ok((index, imported.message))
    }

    /// Random notes on track `t`.
    pub fn randomize_track(&mut self, p: usize, t: usize, rnd: &mut dyn FnMut() -> f64) -> bool {
        self.edit_track(
            p,
            t,
            |track| format!("Randomize {}", track.name),
            None,
            |track| generate::random_track(track, rnd),
        )
        .is_some()
    }

    /// The loop start of pattern `p` (loop `index` of the playback): its
    /// tracks in auto mode get new lines. Not an undo step — the lines change
    /// every loop, as in the JS app — but a change to save.
    pub fn auto_loop(
        &mut self,
        p: usize,
        index: u64,
        scales: &Scales,
        styles: &Styles,
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let Some(pattern) = self.library.patterns.get_mut(p) else { return false };
        if !generate::plays_auto(pattern) {
            return false;
        }
        let changed = generate::run_loop(&mut AutoGenerator::new(scales, styles, rnd), pattern, index);
        if changed {
            self.revision += 1;
        }
        changed
    }

    /// Exchange the tracks of pattern `p` with `tracks`: the lines of a loop
    /// generated ahead going in or out. Not an undo step, like [`Self::auto_loop`].
    pub fn swap_tracks(&mut self, p: usize, tracks: &mut Vec<Track>) {
        if let Some(pattern) = self.library.patterns.get_mut(p) {
            std::mem::swap(&mut pattern.tracks, tracks);
            self.revision += 1;
        }
    }
}

/// `cleanTrack`'s change.
fn clean(track: &mut Track) {
    track.notes.clear();
    track.loop_length = Some(track.beat_count * track.steps_per_beat);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_accent_toggles_on_the_notes_of_a_step_in_one_undo_step() {
        let mut e = editor();
        let velocity = |e: &Editor| e.library().patterns[0].tracks[0].notes[0].velocity;
        assert_eq!(e.toggle_accent(0, 0, 0, 0), Some(true));
        assert_eq!(velocity(&e), ACCENTED_VELOCITY);
        assert_eq!(e.toggle_accent(0, 0, 0, 0), Some(false));
        assert_eq!(velocity(&e), Note::default().velocity);
        assert_eq!(e.toggle_accent(0, 0, 1, 0), None, "no note on that step");
        assert_eq!(e.toggle_accent(0, 0, 0, 0), Some(true));
        assert_eq!(e.undo().as_deref(), Some("Accent on KICK"));
        assert_eq!(velocity(&e), Note::default().velocity);
    }

    #[test]
    fn a_track_added_or_renamed_as_a_bass_is_mono() {
        let mut e = editor();
        let t = e.add_track(0, "BASS", 4).unwrap();
        assert!(pattern(&e).tracks[t].mono);
        assert!(!pattern(&e).tracks[1].mono);
        assert!(e.change_track_name(0, 1, "SUB BASS"));
        assert!(pattern(&e).tracks[1].mono);
        assert!(e.change_track_name(0, 1, "SNARE"));
        assert!(pattern(&e).tracks[1].mono, "renaming away from a bass keeps mono");
        e.undo();
        e.undo();
        assert!(!pattern(&e).tracks[1].mono);
    }

    fn editor() -> Editor {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[
                {"name":"KICK","notes":[{"beat":0},{"beat":2}]},
                {"name":"SNARE","stepsPerBeat":8,"loopAtStep":32,"notes":[{"beat":1,"step":7}]}]}]}"#,
        )
        .unwrap();
        let mut editor = Editor::new(library);
        editor.set_clock(Some(0.0));
        editor
    }

    fn pattern(editor: &Editor) -> &Pattern {
        &editor.library().patterns[0]
    }

    #[test]
    fn add_and_delete_notes_undo_redo() {
        let mut e = editor();
        let original = pattern(&e).clone();
        assert_eq!(e.add_note(0, 0, 1, 2, 0.0), Some(2));
        assert_eq!(e.history().next_undo(), Some("Add note on KICK in \"rock\""));
        assert!(e.delete_note(0, 0, 0, 0, 0.0));
        assert!(!e.delete_note(0, 0, 3, 3, 0.0));
        assert_eq!(pattern(&e).tracks[0].notes.len(), 2);
        assert_eq!(e.history().undo_len(), 2);
        e.undo();
        e.undo();
        assert_eq!(*pattern(&e), original);
        e.redo();
        assert_eq!(pattern(&e).tracks[0].notes.len(), 3);
        assert_eq!(e.revision(), 5);
    }

    #[test]
    fn update_track_clamps_and_filters() {
        let mut e = editor();
        let updates = [
            ("velocity", Value::Number(-3.0)),
            ("scaleName", Value::from("major")),
            ("mute", Value::from("yes")),
            ("filterCutoff", Value::Number(50_000.0)),
        ];
        assert!(e.update_track(0, 0, &updates, None, false));
        let track = &pattern(&e).tracks[0];
        assert_eq!(
            (track.volume, track.filter_cutoff, track.mute, track.scale_name.as_deref()),
            (0.0, 20_000.0, false, None)
        );
        assert_eq!(e.history().next_undo(), Some("Update KICK"));
        // No change, no step.
        assert!(e.update_track(0, 0, &[("velocity", Value::Number(0.0))], None, false));
        assert_eq!(e.history().undo_len(), 1);
        assert!(e.toggle_track(0, 1, "mute"));
        assert!(pattern(&e).tracks[1].mute);
    }

    #[test]
    fn coalesced_drag_is_one_step() {
        let mut e = editor();
        for (i, v) in [0.2, 0.3, 0.4].iter().enumerate() {
            e.set_clock(Some(i as f64 * 100.0));
            e.update_track(0, 0, &[("velocity", Value::Number(*v))], Some("Volume on KICK"), true);
        }
        assert_eq!(e.history().undo_len(), 1);
        e.undo();
        assert_eq!(pattern(&e).tracks[0].volume, 1.0);
    }

    #[test]
    fn steps_per_beat_keeps_positions_by_percent() {
        let mut e = editor();
        // SNARE: beat 1 step 7 of 8 (88 %), loop at 32 > 4 beats × 3.
        assert!(e.set_steps_per_beat(0, 1, 3.0, false));
        let track = &pattern(&e).tracks[1];
        assert_eq!((track.notes[0].step, track.loop_length), (2, Some(12)));
        assert!(e.set_steps_per_beat(0, 1, 8.0, false));
        assert_eq!(pattern(&e).tracks[1].notes[0].step, 7, "back to its step, as stepPercent is kept");
        assert!(!e.set_steps_per_beat(0, 1, 8.0, false));
        assert!(e.incr_steps_per_beat(0, 1));
        assert_eq!(pattern(&e).tracks[1].steps_per_beat, 1);
    }

    #[test]
    fn every_step_comes_back_after_going_down_to_one_and_up_again() {
        let mut e = editor();
        // SNARE at 8 steps per beat: a note on every step of beats 0 and 3.
        for beat in [0, 3] {
            for step in 0..8 {
                e.add_note(0, 1, beat, step, 0.0);
            }
        }
        let places = |e: &Editor| pattern(e).tracks[1].notes.iter().map(|n| (n.beat, n.step)).collect::<Vec<_>>();
        let original = places(&e);
        for grid in (1..8).rev() {
            assert!(e.set_steps_per_beat(0, 1, f64::from(grid), true));
        }
        assert!(places(&e).iter().all(|&(_, step)| step == 0), "one step per beat");
        // Saved and opened again at one step per beat: the positions are in the file.
        let (library, report) = Library::parse(&e.library().to_file_text()).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let mut e = Editor::new(library);
        for _ in 1..8 {
            assert!(e.incr_steps_per_beat(0, 1));
        }
        assert_eq!(pattern(&e).tracks[1].steps_per_beat, 8);
        assert_eq!(places(&e), original);
    }

    #[test]
    fn a_moved_note_forgets_its_place_in_the_beat() {
        let mut e = editor();
        // SNARE: beat 1 step 7 of 8, brought down to 1 step per beat, moved to beat 2.
        assert!(e.set_steps_per_beat(0, 1, 1.0, false));
        assert!(e.update_note(0, 1, 0, &[("beat", Value::from(2))], None, false));
        assert!(e.set_steps_per_beat(0, 1, 8.0, false));
        let note = &pattern(&e).tracks[1].notes[0];
        assert_eq!((note.beat, note.step), (2, 0), "where it was put, not at step 7");
    }

    #[test]
    fn loop_point_and_clean() {
        let mut e = editor();
        assert!(e.shorten_loop(0, 0));
        assert_eq!(pattern(&e).tracks[0].loop_length, Some(15));
        e.set_steps_per_beat(0, 0, 1.0, false); // 4 steps
        for _ in 0..4 {
            e.shorten_loop(0, 0);
        }
        assert_eq!(pattern(&e).tracks[0].loop_length, Some(4), "wrapped from 1 to the track end");
        assert!(e.clean_pattern(0));
        assert!(pattern(&e).tracks.iter().all(|t| t.notes.is_empty()));
        assert_eq!(e.history().next_undo(), Some("Clean \"rock\""));
    }

    #[test]
    fn a_full_pattern_takes_no_more_tracks() {
        let mut e = editor();
        while e.has_track_room(0) {
            e.add_track(0, "T", 4).expect("room");
        }
        assert_eq!(pattern(&e).tracks.len(), MAX_TRACKS);
        let kick = pattern(&e).tracks[0].clone();
        assert_eq!(e.add_track(0, "T", 4), None);
        assert_eq!(e.paste_track(0, 0, &kick), None);
    }

    #[test]
    fn tracks_paste_and_remove() {
        let mut e = editor();
        let kick = pattern(&e).tracks[0].clone();
        assert_eq!(e.paste_track(0, 1, &kick), Some(1));
        assert_eq!(e.paste_track(0, 99, &kick), Some(3));
        let names: Vec<_> = pattern(&e).tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["KICK", "KICK copy", "SNARE", "KICK copy 2"]);
        assert!(e.remove_track(0, 0));
        assert_eq!(e.add_track(0, "CHH", 4), Some(3));
        let chh = &pattern(&e).tracks[3];
        assert_eq!(
            (chh.beat_count, chh.loop_length, chh.pan),
            (4, Some(16), ordrumbox_core::drum_taxonomy::pan_from_track_name("CHH"))
        );
    }

    #[test]
    fn tracks_move_as_one_step() {
        let mut e = editor();
        let names = |e: &Editor| pattern(e).tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        e.add_track(0, "CHH", 4).expect("room");
        assert!(!e.move_track(0, 1, 1));
        assert!(!e.move_track(0, 5, 0));
        assert!(e.move_track(0, 0, 99));
        assert_eq!(names(&e), ["SNARE", "CHH", "KICK"]);
        assert!(e.move_track(0, 1, 0));
        assert_eq!(names(&e), ["CHH", "SNARE", "KICK"]);
        assert_eq!(e.undo().as_deref(), Some("Move track CHH"));
        assert_eq!(names(&e), ["SNARE", "CHH", "KICK"]);
    }

    #[test]
    fn patterns_move_as_one_step() {
        let mut e = editor();
        e.add_pattern(Some("verse"));
        e.add_pattern(Some("chorus"));
        let ids = |e: &Editor| e.library().patterns.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        assert!(!e.move_pattern(1, 1));
        assert!(!e.move_pattern(5, 0));
        assert!(e.move_pattern(0, 99));
        assert_eq!(ids(&e), ["verse", "chorus", "rock"]);
        assert!(e.move_pattern(1, 0));
        assert_eq!(ids(&e), ["chorus", "verse", "rock"]);
        assert_eq!(e.undo().as_deref(), Some("Move pattern \"chorus\""));
        assert_eq!(ids(&e), ["verse", "chorus", "rock"]);
    }

    #[test]
    fn patterns() {
        let mut e = editor();
        assert!(!e.remove_pattern(0), "the last pattern stays");
        assert_eq!(e.add_pattern(None), 1);
        assert_eq!(e.add_pattern(Some("rock")), 2);
        let ids: Vec<_> = e.library().patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["rock", "newpat-1", "rock-2"]);
        assert!(e.set_pattern_beat_count(0, 2.0));
        assert!(pattern(&e).tracks.iter().all(|t| t.beat_count == 2));
        assert_eq!(pattern(&e).tracks[1].loop_length, Some(16));
        assert!(e.set_pattern_beat_count(0, 40.0));
        assert_eq!(pattern(&e).beat_count, 4);
        // The tempo is the library's, from the file's first pattern.
        assert_eq!(e.library().bpm, 100.0);
        assert!(e.set_bpm(f64::NAN, false));
        assert_eq!(e.library().bpm, 120.0);
        assert!(!e.set_bpm(400.0, false) || e.library().bpm == 300.0);
        assert!(e.rename_pattern(0, "  "));
        assert_eq!(pattern(&e).name.as_deref(), Some("rock"));
        assert!(e.rename_pattern(0, "  Rock II "));
        assert_eq!((pattern(&e).id.as_str(), pattern(&e).name.as_deref()), ("rock", Some("Rock II")));
        assert_eq!(e.history().next_undo(), Some("Rename pattern → \"Rock II\""));
        e.undo();
        assert!(e.remove_pattern(1));
        e.undo();
        assert_eq!(e.library().patterns.len(), 3);
        assert_eq!(e.duplicate_pattern(0), Some(3));
        let copy = &e.library().patterns[3];
        assert_eq!((copy.id.as_str(), copy.name.as_deref(), copy.tracks.len()), ("rock-copy", Some("rock copy"), 2));
    }

    /// The JS undo loses note edits once a command restored copies of the
    /// notes (removing the track, a steps-per-beat change): its closures then
    /// edit detached objects. Steps holding patterns have no such link.
    #[test]
    fn note_edits_survive_track_level_undo() {
        let mut e = editor();
        e.update_note(0, 1, 0, &[("velocity", Value::Number(0.3))], None, false);
        e.set_steps_per_beat(0, 1, 4.0, false);
        e.remove_track(0, 1);
        while e.undo().is_some() {}
        assert_eq!(pattern(&e).tracks[1].notes[0].velocity, 0.8);
        while e.redo().is_some() {}
        assert_eq!(pattern(&e).tracks.len(), 1);
        e.undo();
        e.undo();
        assert_eq!(pattern(&e).tracks[1].notes[0].velocity, 0.3);
    }

    #[test]
    fn nudges_and_paste_step() {
        let mut e = editor();
        assert!(e.nudge_note(0, 0, 0, Nudge::VelocityUp));
        assert!(e.nudge_note(0, 0, 0, Nudge::VelocityUp));
        assert_eq!(pattern(&e).tracks[0].notes[0].velocity, 0.9);
        assert_eq!(e.history().undo_len(), 1, "held nudges coalesce");
        assert_eq!(nudge_velocity(1.0, true), 1.0);
        assert_eq!(nudge_pitch(48.0, true, 0.0), 48.0);
        assert_eq!(nudge_pitch(2.4, false, 0.0), 1.0);
        let notes = pattern(&e).tracks[0].notes[..1].to_vec();
        assert!(e.paste_step_notes(0, 1, 3, 1, &notes));
        let pasted = pattern(&e).tracks[1].notes.last().unwrap().clone();
        assert_eq!((pasted.beat, pasted.step, pasted.velocity), (3, 1, 0.9));
        assert!(e.delete_step(0, 1, 3, 1));
    }

    fn styles() -> Styles {
        let text = include_str!("../../../assets/data/styles.json");
        Styles::from_json(&ordrumbox_core::json::parse(text).unwrap()).0
    }

    #[test]
    fn generation_commands() {
        let mut e = editor();
        let (scales, styles) = (Scales::default(), styles());
        let mut rng = ordrumbox_core::Rng::new(5);
        let mut rnd = || rng.next_f64();

        // B: a new generated pattern in the asked style, one undo step.
        let undo = e.history().undo_len();
        let p = e.generate_pattern(&scales, &styles, Some("rap"), &mut rnd);
        assert_eq!(p, 1);
        let generated = &e.library().patterns[p];
        assert!(generated.tracks.iter().any(|t| t.name == "KICK" && !t.notes.is_empty()));
        assert_eq!(generated.extra.get("autoGenre").and_then(|g| g.as_str()), Some("hiphop"));
        assert_eq!(e.history().undo_len(), undo + 1);
        assert_eq!(e.history().next_undo(), Some("Generate pattern \"NewPat_1\""));

        // ↻ Drum on: KICK and SNARE in auto mode.
        assert_eq!(e.toggle_drums(0, &scales, &styles, Some("funk"), &mut rnd), Some(true));
        assert!(pattern(&e).tracks.iter().all(|t| t.auto_mode));
        assert_eq!(e.history().next_undo(), Some("Generate drums"));
        // Funk lines vary loop after loop, with no undo step.
        let (undo, before) = (e.history().undo_len(), pattern(&e).clone());
        assert!(e.auto_loop(0, 1, &scales, &styles, &mut rnd));
        assert_eq!(e.history().undo_len(), undo);
        assert_ne!(*pattern(&e), before);
        // ↻ Drum again: auto off.
        assert_eq!(e.toggle_drums(0, &scales, &styles, None, &mut rnd), Some(false));
        assert!(pattern(&e).tracks.iter().all(|t| !t.auto_mode));
        assert!(!e.auto_loop(0, 2, &scales, &styles, &mut rnd));

        // ↻ on one track: a new line, one undo step.
        assert!(e.regenerate_track(0, 1, &scales, &styles, &mut rnd));
        assert_eq!(e.history().next_undo(), Some("Generate SNARE"));

        // ↻ Bass: a synth BASS track with a line, in auto mode.
        assert_eq!(e.toggle_melodic(0, MelodicKind::Bass, &scales, &styles, &mut rnd), Some(true));
        let bass = pattern(&e).tracks.last().unwrap();
        assert!(bass.name == "BASS" && bass.use_synth && bass.auto_mode && !bass.notes.is_empty());
        assert_eq!(e.history().next_undo(), Some("Generate bass"));
        e.undo();
        assert!(pattern(&e).tracks.iter().all(|t| t.name != "BASS"));

        assert!(e.randomize_track(0, 0, &mut rnd));
        assert_eq!(e.history().next_undo(), Some("Randomize KICK"));
        assert!(!pattern(&e).tracks[0].notes.is_empty());
    }

    #[test]
    fn midi_import_is_one_step() {
        let mut e = editor();
        // Format 0, 96 PPQN: a kick (channel 10, note 36) on beats 1 and 3.
        let mut track = vec![0x00, 0x99, 36, 100, 0x18, 0x89, 36, 0, 0x81, 0x28, 0x99, 36, 64, 0x18, 0x89, 36, 0];
        track.extend([0x00, 0xff, 0x2f, 0x00]);
        let mut file = b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x00\x60MTrk".to_vec();
        file.extend((track.len() as u32).to_be_bytes());
        file.extend(track);
        let (index, message) = e.import_midi("groove.mid", &file, &[]).unwrap();
        assert_eq!((index, message.as_str()), (1, "MIDI imported: 1 track(s)"));
        let pattern = &e.library().patterns[1];
        assert_eq!((pattern.id.as_str(), pattern.name.as_deref(), pattern.beat_count), ("groove", Some("groove"), 32));
        let kick = &pattern.tracks[0];
        assert_eq!(kick.name, "KICK");
        let notes: Vec<_> = kick.notes.iter().map(|n| (n.beat, n.step, n.velocity)).collect();
        assert_eq!(notes, [(0, 0, 0.79), (2, 0, 0.5)]);
        assert_eq!(e.history().next_undo(), Some("Import MIDI file"));
        e.undo();
        assert_eq!(e.library().patterns.len(), 1);
        assert_eq!(e.import_midi("x.mid", b"nothing", &[]), Err("No MIDI notes found in file".into()));
    }

    #[test]
    fn a_batch_is_one_undo_step() {
        let mut e = editor();
        e.add_note(0, 0, 3, 0, 0.0);
        let before = e.library().clone();
        let added = e.batch("Remote edit", |e| {
            let t = e.add_track(0, "HH", 4).unwrap();
            e.add_note(0, t, 0, 1, 0.0);
            e.set_bpm(140.0, false);
            e.add_song("verse");
            e.batch("inner", |e| e.set_pattern_tags(0, &[" rock ".into(), " ".into()]));
            t
        });
        assert_eq!(added, 2);
        assert_eq!(e.history().undo_len(), 2);
        assert_eq!(e.history().next_undo(), Some("Remote edit"));
        assert_eq!(pattern(&e).tags, ["rock"]);
        assert_eq!(e.selected_song(), 0);
        let after = e.library().clone();
        e.undo();
        assert_eq!(e.library(), &before);
        e.redo();
        assert_eq!(e.library(), &after);
        // A batch that changes nothing records nothing.
        e.batch("nothing", |e| e.set_bpm(140.0, false));
        assert_eq!(e.history().undo_len(), 2);
    }

    #[test]
    fn song_settings() {
        let mut e = editor();
        assert!(e.set_song_settings(None, Some(90.0), None, None).is_err());
        e.add_song("verse");
        e.set_song_settings(None, Some(500.0), Some("intro"), Some(8.0)).unwrap();
        let song = &e.library().songs[0];
        assert_eq!((song.bpm, song.description.as_str(), song.loop_measures), (300.0, "intro", Some(8.0)));
        e.set_song_settings(Some(0), None, None, Some(0.0)).unwrap();
        assert_eq!(e.library().songs[0].loop_measures, None);
        assert_eq!(e.library().songs[0].bpm, 300.0);
    }

    #[test]
    fn a_song_is_renamed_in_one_step() {
        let mut e = editor();
        assert!(e.rename_song(None, "chorus").is_err());
        e.add_song("verse");
        assert_eq!(e.rename_song(None, "   "), Ok(false));
        assert_eq!(e.rename_song(Some(0), " chorus "), Ok(true));
        assert_eq!(e.rename_song(None, "chorus"), Ok(false));
        let song = &e.library().songs[0];
        assert_eq!((song.name.as_str(), song.id.as_str()), ("chorus", "verse"));
        assert_eq!(e.history().next_undo(), Some("Rename arrangement → \"chorus\""));
        e.undo();
        assert_eq!(e.library().songs[0].name, "verse");
    }

    #[test]
    fn clips_are_placed_by_id_or_name_and_refused_with_a_reason() {
        let mut e = editor();
        let no_song = Err("No arrangement to edit — create a song first".to_owned());
        assert_eq!(e.add_pattern_at_measure("rock", 0, None, None), no_song);
        e.add_song("verse");

        let clip = e.add_pattern_at_measure("  ROCK ", 2, Some(0.75), None).unwrap();
        assert_eq!((clip.pattern_id.as_str(), clip.start_measure, clip.measure_count), ("rock", 2, 0.75));
        let clip = e.add_pattern_at_measure("rock", 0, Some(0.0), None).unwrap();
        assert_eq!(clip.measure_count, 1.0, "no length given: the pattern's, 4 beats");
        assert_eq!(e.history().next_undo(), Some("Add \"rock\" at measure 1"));
        let steps = e.history().undo_len();
        assert_eq!(
            e.add_pattern_at_measure(" no such pattern ", 0, None, None),
            Err("No pattern named \"no such pattern\" in the library".to_owned())
        );
        assert_eq!(e.add_pattern_at_measure("", 0, None, None), Err("No pattern named \"\" in the library".to_owned()));
        assert_eq!(e.repeat_pattern_at_measure(998, None), Err("No clip at measure 999".to_owned()));
        assert_eq!(e.remove_pattern_at_measure(998, None), Err("No clip at measure 999".to_owned()));
        assert_eq!(e.remove_pattern_clips("ghost", None), Ok(Vec::new()));
        assert_eq!(e.remove_pattern_clips("  ", None), Err("No pattern given to remove".to_owned()));
        assert_eq!(e.history().undo_len(), steps, "a refused edit is no step");

        // Repeated right after itself, floored: 2 + 0.75 → measure 2.
        let repeat = e.repeat_pattern_at_measure(2, None).unwrap();
        assert_eq!((repeat.start_measure, repeat.measure_count), (2, 0.75));
        assert_eq!(e.library().songs[0].loop_measures, Some(2.75));

        // Indices out of range and repeated count once; none left: no step.
        assert!(e.remove_song_clips(None, &[0, 2, 2, 999]));
        assert_eq!(e.history().next_undo(), Some("Remove 2 clips"));
        assert!(!e.remove_song_clips(None, &[]));
        assert!(!e.remove_song_clips(None, &[5]));
        assert_eq!(e.library().songs[0].clips.len(), 1);
    }

    #[test]
    fn arrangements_are_added_removed_and_selected_in_one_step() {
        let mut e = editor();
        assert_eq!(e.add_song("  New one "), 0);
        assert_eq!(e.add_song(""), 1);
        assert_eq!(e.add_song("Untitled"), 2);
        let songs: Vec<_> = e.library().songs.iter().map(|s| (s.id.as_str(), s.name.as_str())).collect();
        assert_eq!(songs, [("new-one", "New one"), ("untitled", "Untitled"), ("untitled-2", "Untitled")]);
        assert_eq!(e.selected_song(), 2, "a new arrangement is selected");
        assert_eq!(e.set_selected_song(99), 2, "clamped to the list");

        assert!(!e.remove_song(Some(42)));
        assert!(e.remove_song(Some(0)));
        assert_eq!(e.selected_song(), 1, "kept inside the list");
        assert!(e.remove_song(None));
        assert_eq!((e.library().songs.len(), e.selected_song()), (1, 0));
        assert_eq!(e.history().next_undo(), Some("Remove arrangement \"Untitled\""));

        // Undo selects the arrangement it brings back, redo the one after.
        e.undo();
        assert_eq!((e.library().songs.len(), e.selected_song()), (2, 1));
        e.undo();
        assert_eq!(e.selected_song(), 0);
        e.redo();
        assert_eq!(e.selected_song(), 1);
    }

    #[test]
    fn a_moved_clip_takes_the_loop_along_in_one_step() {
        let mut e = editor();
        e.add_song("verse");
        let clip = ordrumbox_core::SongClip { pattern_id: "a".into(), start_measure: 0, measure_count: 2.0 };
        assert!(e.add_song_clip(None, clip));
        assert!(!e.move_song_clip(None, 0, 0));
        assert!(!e.move_song_clip(None, 1, 3));
        assert!(e.move_song_clip(None, 0, 3));
        let song = &e.library().songs[0];
        assert_eq!((song.clips[0].start_measure, song.loop_measures), (3, Some(5.0)));
        e.undo();
        let song = &e.library().songs[0];
        assert_eq!((song.clips[0].start_measure, song.loop_measures), (0, Some(2.0)));
    }

    #[test]
    fn synth_preset_edits_are_undone_and_a_knob_turned_is_one_step() {
        let mut e = Editor::new(Library::default());
        e.set_clock(Some(0.0));
        let set = |e: &mut Editor, gain: f64| {
            let preset = ordrumbox_core::json::parse(&format!(r#"{{"gain":{gain}}}"#)).unwrap();
            e.edit_synths("Edit \"a\"".into(), Some("synth:a".into()), |synths| {
                synths.insert("a", preset);
            })
        };
        assert!(!e.edit_synths("nothing".into(), None, |_| {}), "nothing changed");
        assert_eq!(e.library().synths, None);
        assert!(set(&mut e, 0.5));
        assert!(set(&mut e, 0.6));
        assert!(!set(&mut e, 0.6));
        assert_eq!(e.history().undo_len(), 1, "one step");
        e.undo();
        assert_eq!(e.library().synths, None);
        e.redo();
        assert_eq!(
            e.library().synths.as_ref().and_then(|s| s.get("a")).map(Value::to_json).as_deref(),
            Some(r#"{"gain":0.6}"#)
        );
        // A song loaded keeps the presets; a library with its own brings them.
        e.load_library(Library::default(), 0);
        assert!(e.library().synths.is_some());
        e.replace_library(Library { synths: Some(Object::new()), ..Library::default() }, 0, "import");
        assert_eq!(e.library().synths, Some(Object::new()));
        e.undo();
        assert!(e.library().synths.as_ref().is_some_and(|s| s.get("a").is_some()));
    }
}
