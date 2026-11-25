//! The piano roll's right-click menus: on the keyboard, clear the track or
//! write the next chord progression from a key; on the grid, add a chord
//! there.

use super::Session;
use super::session_labels::{beat_label, notes_label};
use crate::menu_action::{RollAction, RollChord};
use crate::note_drag::{MIDDLE_C, MIDI_MIN, midi_name};
use crate::panel::{Dropdown, Id};
use ordrumbox_core::num;
use ordrumbox_logic::composition;

impl Session {
    /// The keyboard: clear the track, or write the next progression from
    /// that key.
    pub(super) fn roll_key_menu(&mut self, t: usize, midi: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let progression = composition::progression(self.roll.sequence);
        let title = format!("{} - {} {}", track.name, midi_name(midi), progression.name);
        let tonic = self.roll_pitch(t, midi - MIDI_MIN as i32);
        let lines =
            [(RollAction::Clear, "Clear all".to_owned()), (RollAction::Sequence { tonic }, "Add sequence".to_owned())];
        self.chrome.widgets.open_dropdown(Dropdown::menu(Id::RollMenu, x, y, title, lines));
    }

    /// A cell: add or delete its note, add a chord on it
    /// (what cannot run is left out).
    pub(super) fn roll_grid_menu(&mut self, t: usize, step: i64, row: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let (beat, beat_step) = self.roll_beat(t, step);
        let title = format!("{} @ {}.{}", track.name, beat + 1, beat_step + 1);
        let pitch = self.roll_pitch(t, row);
        let note = match self.roll_note_at(t, step, row) {
            None => (RollAction::AddNote { step, pitch }, "Add note".to_owned()),
            Some(n) => (RollAction::DeleteNote(n), "Delete note".to_owned()),
        };
        let chords = RollChord::ALL
            .map(|chord| (RollAction::AddChord { chord, step, root: pitch }, format!("Add {} chord", chord.name())));
        let lines = std::iter::once(note).chain(chords);
        self.chrome.widgets.open_dropdown(Dropdown::menu(Id::RollMenu, x, y, title, lines));
    }

    /// An action of a piano roll menu.
    pub(super) fn roll_menu_pick(&mut self, action: RollAction) {
        let Some(t) = self.roll_track() else { return };
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return };
        match action {
            RollAction::AddNote { step, pitch } => {
                let row =
                    (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + pitch - MIDI_MIN).round() as i32;
                if let Some(n) = self.roll_add(t, step, row) {
                    self.audition(t, Some(n));
                    let (beat, beat_step) = self.roll_beat(t, step);
                    self.message =
                        format!("Added note (pitch {}) - {name} @ {}", num::text(pitch), beat_label(beat, beat_step));
                }
            }
            RollAction::DeleteNote(n) => {
                let Some(note) = self.track(t).and_then(|track| track.notes.get(n)).cloned() else { return };
                if self.roll_delete_note(t, n) {
                    self.message = format!("Deleted note - {name} @ {}", beat_label(note.beat, note.step));
                }
            }
            RollAction::AddChord { chord, step, root } => self.roll_chord(t, step, root, chord),
            RollAction::Clear => self.clear_track(t),
            RollAction::Sequence { tonic } => self.roll_sequence_from(t, tonic),
        }
    }

    /// Add the notes of `chord` from `root` the step has not got, the
    /// first selected.
    pub(super) fn roll_chord(&mut self, t: usize, step: i64, root: f64, chord: RollChord) {
        let (beat, beat_step) = self.roll_beat(t, step);
        let Some(track) = self.track(t) else { return };
        let name = track.name.clone();
        let existing: Vec<f64> =
            track.notes.iter().filter(|n| n.beat == beat && n.step == beat_step).map(|n| n.pitch).collect();
        let mut added = Vec::new();
        for interval in chord.intervals() {
            let pitch = root + interval;
            if !existing.contains(&pitch)
                && let Some(n) = self.editor.add_note(self.pattern_index, t, beat, beat_step, pitch)
            {
                added.push(n);
            }
        }
        let Some(&first) = added.first() else {
            self.message = "Chord already present".into();
            return;
        };
        self.edited();
        self.selection.selected = Some((t, first));
        let row = (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + root - MIDI_MIN).round() as i32;
        self.roll.cursor = Some((step, row));
        self.message = format!(
            "Added {} chord ({}) - {name} @ {}",
            chord.name(),
            notes_label(added.len()),
            beat_label(beat, beat_step)
        );
    }

    /// Replace the track's notes with the next progression from `tonic`,
    /// a chord per measure.
    pub(super) fn roll_sequence_from(&mut self, t: usize, tonic: f64) {
        let progression = composition::progression(self.roll.sequence);
        let beat_count = self.pattern().map_or(4, |p| p.beat_count);
        let planned = composition::progression_notes(progression, tonic, beat_count);
        if planned.is_empty() {
            return;
        }
        if self.track(t).is_some_and(|track| !track.notes.is_empty()) {
            self.editor.clean_track(self.pattern_index, t);
        }
        let mut first = None;
        for &(beat, pitch) in &planned {
            let n = self.editor.add_note(self.pattern_index, t, beat, 0, pitch);
            first = first.or(n);
        }
        self.roll.sequence += 1;
        self.edited();
        self.selection.selected = first.map(|n| (t, n));
        let row = (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + tonic - MIDI_MIN).round() as i32;
        self.roll.cursor = Some((0, row));
        let name = self.track(t).map(|track| track.name.clone()).unwrap_or_default();
        self.message = format!(
            "Replaced with sequence \"{}\" ({}, 1 chord/measure) - {name}",
            progression.name,
            notes_label(planned.len())
        );
    }
}
