//! Editing notes: add, delete, nudge, paste, zones.

use super::{Editor, Nudge, Played, nudge_pitch, nudge_velocity, pattern_name, step_percent};
use crate::block::{Block, Pasted, StepAt, Zone};
use crate::quantize::{self, Quantize, Scope};
use crate::roll_block::{RollBlock, RollZone};
use crate::{block, roll_block};
use ordrumbox_core::json::Value;
use ordrumbox_core::note::{ACCENT_VELOCITY, ACCENTED_VELOCITY, MAX_SHIFT_STEPS};
use ordrumbox_core::num::round2;
use ordrumbox_core::{Note, Track};
use std::ops::RangeInclusive;

impl Editor {
    /// Add a note (defaults) at `beat`/`beat_step`; its index in the track.
    pub fn add_note(&mut self, p: usize, t: usize, beat: i32, beat_step: i32, pitch: f64) -> Option<usize> {
        self.add_note_played(p, t, beat, beat_step, Played { pitch, velocity: None, shift_steps: 0.0 })
    }

    /// Add a note at `beat`/`beat_step` as it was played ([`Played`]); its
    /// index in the track.
    pub fn add_note_played(&mut self, p: usize, t: usize, beat: i32, beat_step: i32, played: Played) -> Option<usize> {
        let Played { pitch, velocity, shift_steps } = played;
        let pattern = pattern_name(self.library.patterns.get(p)?).to_owned();
        self.edit_track(
            p,
            t,
            |track| format!("Add note on {} in \"{pattern}\"", track.name),
            None,
            |track| {
                let percent = step_percent(beat_step, track.steps_per_beat);
                let mut note = Note { beat, step: beat_step, pitch, step_percent: Some(percent), ..Note::default() };
                if let Some(velocity) = velocity {
                    note.velocity = velocity.clamp(0.0, 1.0);
                }
                // Two decimals, as a note's shift is saved.
                note.shift_steps = round2(shift_steps.clamp(-MAX_SHIFT_STEPS, MAX_SHIFT_STEPS));
                track.notes.push(note);
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

    /// Set field `key` of notes of track `t`, each to its own value (`values`:
    /// note index, value) as one edit named `desc`. With `coalesce`, a run of
    /// them on the same field of the same track is one undo step: a lane of
    /// the grid painted with the mouse.
    pub fn set_note_values(
        &mut self,
        p: usize,
        t: usize,
        key: &str,
        values: &[(usize, f64)],
        desc: &str,
        coalesce: bool,
    ) -> bool {
        let coalesce = coalesce.then(|| format!("notes:{p}:{t}:{key}"));
        self.edit_track(
            p,
            t,
            |_| desc.to_owned(),
            coalesce,
            |track| {
                let mut changed = false;
                for &(n, value) in values {
                    let Some(note) = track.notes.get_mut(n) else { continue };
                    let before = note.clone();
                    note.set_field(key, &Value::Number(value));
                    changed |= *note != before;
                }
                changed
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

    /// Nudge note `n` (`applyNoteNudge`: pitch ±1, one step in time; the
    /// velocity ±0.05 stays for the note editor and the drag).
    pub fn nudge_note(&mut self, p: usize, t: usize, n: usize, nudge: Nudge) -> bool {
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        let Some(note) = track.notes.get(n) else { return false };
        let field = match nudge {
            Nudge::VelocityUp | Nudge::VelocityDown => {
                Some(("velocity", nudge_velocity(note.velocity, nudge == Nudge::VelocityUp)))
            }
            Nudge::PitchUp | Nudge::PitchDown => {
                Some(("pitch", nudge_pitch(note.pitch, nudge == Nudge::PitchUp, track.transpose)))
            }
            Nudge::Later | Nudge::Earlier => None,
        };
        if let Some((key, value)) = field {
            let desc = format!("Edit note {key} on {}", track.name);
            return self.update_note(p, t, n, &[(key, Value::Number(value))], Some(&desc), true);
        }
        // One step in the pattern's time, wrapping at its length, the cursor's way.
        let spb = i64::from(track.steps_per_beat.max(1));
        let total = i64::from(self.library.patterns.get(p).map_or(1, |p| p.beat_count.max(1))) * spb;
        let position = i64::from(note.beat) * spb + i64::from(note.step);
        let next = (position + if nudge == Nudge::Later { 1 } else { -1 }).rem_euclid(total);
        let (beat, step) = ((next / spb) as i32, (next % spb) as i32);
        let desc = format!("Move note on {}", track.name);
        self.update_note(
            p,
            t,
            n,
            &[("beat", Value::Number(f64::from(beat))), ("step", Value::Number(f64::from(step)))],
            Some(&desc),
            true,
        )
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

    /// Remove the notes inside `zone` of pattern `p`, as one step; how many.
    pub fn delete_zone(&mut self, p: usize, zone: &Zone) -> usize {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else { return 0 };
        self.edit(p, |_| format!("Delete notes in \"{pattern}\""), None, |pattern| block::delete_zone(pattern, zone))
            .unwrap_or(0)
    }

    /// Quantize the notes of pattern `p` in `scope` ([`crate::quantize::quantize`]),
    /// as one step; how many moved.
    pub fn quantize(&mut self, p: usize, scope: Scope, how: Quantize) -> usize {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else { return 0 };
        self.edit(
            p,
            |_| format!("Quantize notes in \"{pattern}\""),
            None,
            |pattern| quantize::quantize(pattern, scope, how),
        )
        .unwrap_or(0)
    }

    /// Paste `block` at step `at` of pattern `p` ([`block::paste_block`]), as one step.
    pub fn paste_block(&mut self, p: usize, at: StepAt, block: &Block) -> Pasted {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else {
            return Pasted::default();
        };
        self.edit(
            p,
            |_| format!("Paste notes in \"{pattern}\""),
            None,
            |pattern| block::paste_block(pattern, at, block),
        )
        .unwrap_or_default()
    }

    /// Remove the notes of pattern `p` inside the piano roll zone `zone`
    /// ([`roll_block::delete_roll_zone`]), as one step; how many.
    pub fn delete_roll_zone(&mut self, p: usize, zone: &RollZone) -> usize {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else { return 0 };
        self.edit(
            p,
            |_| format!("Delete notes in \"{pattern}\""),
            None,
            |pattern| roll_block::delete_roll_zone(pattern, zone),
        )
        .unwrap_or(0)
    }

    /// Paste `block` on track `t` of pattern `p` from `step`, its top on
    /// `top_key` ([`roll_block::paste_roll_block`]), as one step.
    pub fn paste_roll_block(
        &mut self,
        p: usize,
        t: usize,
        (step, top_key): (i64, i32),
        block: &RollBlock,
        keys: RangeInclusive<i32>,
    ) -> Pasted<RollZone> {
        let Some(pattern) = self.library.patterns.get(p).map(|p| pattern_name(p).to_owned()) else {
            return Pasted::default();
        };
        self.edit(
            p,
            |_| format!("Paste notes in \"{pattern}\""),
            None,
            |pattern| roll_block::paste_roll_block(pattern, t, step, top_key, block, keys),
        )
        .unwrap_or_default()
    }
}
