//! The piano roll's right-click menus: on the keyboard, clear the track or
//! write the next chord progression from a key; on the grid, add a chord
//! there.

use super::Session;
use super::session_labels::{beat_label, notes_label};
use crate::note_drag::{MIDDLE_C, MIDI_MIN, midi_name};
use crate::panel::{Dropdown, Id};
use ordrumbox_core::num;
use ordrumbox_logic::composition;

/// A chord type offered by the piano roll's right-click menu.
#[derive(Debug, Clone, Copy)]
struct RollChord {
    /// Key used in the menu value (`"{key}:{step}:{pitch}"`) and in the
    /// result message (`"Added {key} chord"`).
    key: &'static str,
    /// Label shown in the menu (`"Add {key} chord"`).
    label: &'static str,
    /// Intervals in semitones from the root.
    intervals: &'static [f64],
}

/// The chord types generated from a right-click on the grid: the common
/// triads first, then the enriched (seventh) ones.
const ROLL_CHORDS: &[RollChord] = &[
    RollChord { key: "minor", label: "Add minor chord", intervals: &[0.0, 3.0, 7.0] },
    RollChord { key: "major", label: "Add major chord", intervals: &[0.0, 4.0, 7.0] },
    RollChord { key: "diminished", label: "Add diminished chord", intervals: &[0.0, 3.0, 6.0] },
    RollChord { key: "major7", label: "Add major7 chord", intervals: &[0.0, 4.0, 7.0, 11.0] },
    RollChord { key: "minor7", label: "Add minor7 chord", intervals: &[0.0, 3.0, 7.0, 10.0] },
    RollChord { key: "sus4", label: "Add sus4 chord", intervals: &[0.0, 5.0, 7.0] },
    RollChord { key: "seventh", label: "Add seventh chord", intervals: &[0.0, 4.0, 7.0, 10.0] },
];

impl Session {
    /// The keyboard: clear the track, or write the next progression from
    /// that key.
    pub(super) fn roll_key_menu(&mut self, t: usize, midi: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let progression = composition::progression(self.roll.sequence);
        let title = format!("{} - {} {}", track.name, midi_name(midi), progression.name);
        let tonic = self.roll_pitch(t, midi - MIDI_MIN as i32);
        let lines =
            [("clear".to_owned(), "Clear all".to_owned()), (format!("sequence:{tonic}"), "Add sequence".to_owned())];
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::RollMenu, x, y, title, lines));
    }

    /// A cell: add or delete its note, add a chord on it
    /// (`#showGridContextMenu`; what cannot run is left out).
    pub(super) fn roll_grid_menu(&mut self, t: usize, step: i64, row: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let (beat, beat_step) = self.roll_beat(t, step);
        let title = format!("{} @ {}.{}", track.name, beat + 1, beat_step + 1);
        let pitch = self.roll_pitch(t, row);
        let at = format!("{step}:{pitch}");
        let note = match self.roll_note_at(t, step, row) {
            None => (format!("add:{at}"), "Add note".to_owned()),
            Some(n) => (format!("delete:{n}"), "Delete note".to_owned()),
        };
        let chords = ROLL_CHORDS.iter().map(|chord| (format!("{}:{at}", chord.key), chord.label.to_owned()));
        let lines = std::iter::once(note).chain(chords);
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::RollMenu, x, y, title, lines));
    }

    /// An action of a piano roll menu.
    pub(super) fn roll_menu_pick(&mut self, value: &str) {
        let Some(t) = self.roll_track() else { return };
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return };
        let parts: Vec<&str> = value.split(':').collect();
        let number = |i: usize| parts.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        match parts[0] {
            "add" => {
                let (step, pitch) = (number(1) as i64, number(2));
                let row =
                    (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + pitch - MIDI_MIN).round() as i32;
                if let Some(n) = self.roll_add(t, step, row) {
                    self.audition(t, Some(n));
                    let (beat, beat_step) = self.roll_beat(t, step);
                    self.message =
                        format!("Added note (pitch {}) - {name} @ {}", num::text(pitch), beat_label(beat, beat_step));
                }
            }
            "delete" => {
                let n = number(1) as usize;
                let Some(note) = self.track(t).and_then(|track| track.notes.get(n)).cloned() else { return };
                if self.roll_delete_note(t, n) {
                    self.message = format!("Deleted note - {name} @ {}", beat_label(note.beat, note.step));
                }
            }
            key if ROLL_CHORDS.iter().any(|c| c.key == key) => {
                let (step, root) = (number(1) as i64, number(2));
                if let Some(chord) = ROLL_CHORDS.iter().find(|c| c.key == key) {
                    self.roll_chord(t, step, root, chord.intervals, chord.key);
                }
            }
            "clear" => self.clear_track(t),
            "sequence" => self.roll_sequence_from(t, number(1)),
            _ => {}
        }
    }

    /// Add the chord's notes the step has not got, the
    /// first selected.
    pub(super) fn roll_chord(&mut self, t: usize, step: i64, root: f64, intervals: &[f64], quality: &str) {
        let (beat, beat_step) = self.roll_beat(t, step);
        let Some(track) = self.track(t) else { return };
        let name = track.name.clone();
        let existing: Vec<f64> =
            track.notes.iter().filter(|n| n.beat == beat && n.step == beat_step).map(|n| n.pitch).collect();
        let mut added = Vec::new();
        for interval in intervals {
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
        self.message =
            format!("Added {quality} chord ({}) - {name} @ {}", notes_label(added.len()), beat_label(beat, beat_step));
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
