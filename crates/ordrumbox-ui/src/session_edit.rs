//! The pattern grid's edits: notes added, selected, moved and deleted, the
//! cursor, the tracks added, removed and compacted, the pattern's length,
//! new patterns, and copy and paste.

use super::*;

impl Session {
    pub(super) fn audition(&mut self, t: usize, note: Option<usize>) {
        if let Some(controller) = self.playback.controller_mut() {
            controller.preview(self.editor.library(), self.pattern_index, t, note);
        }
    }

    pub fn audition_row(&mut self, t: usize) {
        if self.track(t).is_some() {
            self.audition(t, None);
        }
    }

    pub fn click(&mut self, hit: Hit) {
        match hit {
            Hit::Mute(t) => self.toggle_row(t, "mute"),
            Hit::Solo(t) => self.toggle_row(t, "solo"),
            Hit::Name(t) => {
                self.selection.put_cursor(Cursor {
                    track: t,
                    beat: self.selection.cursor.map_or(0, |c| c.beat),
                    step: 0,
                });
                self.selection.note_focus = Some(NoteFocus::Track(t));
                self.audition(t, None);
            }
            Hit::Cell { track, beat, step } => {
                self.selection.cursor = Some(Cursor { track, beat, step });
                self.selection.zone = None;
                self.toggle_note(track, beat, step);
            }
            Hit::AddTrack => self.add_track_menu_at_button(),
        }
    }

    /// A grid click / Enter: a step is on or off. An empty step gets a note
    /// (selected); a step with notes loses them all, in one undo step and
    /// without asking. Shift+click selects a note instead ([`Self::select_step_note`]).
    fn toggle_note(&mut self, t: usize, beat: i32, step: i32) {
        let p = self.pattern_index;
        let Some(track) = self.track(t) else { return };
        let at_step = track.notes.iter().position(|n| n.beat == beat && n.step == step);
        match at_step {
            Some(_) => {
                self.selection.cursor = Some(Cursor { track: t, beat, step });
                self.delete_step_now();
                self.selection.note_focus = Some(NoteFocus::Step { track: t, beat, step });
            }
            None => {
                self.selection.selected = self.editor.add_note(p, t, beat, step, 0.0).map(|n| (t, n));
                self.edited();
                self.audition(t, self.selection.selected.map(|(_, n)| n));
            }
        }
    }

    /// Shift+click on a grid cell: the cursor goes there and the step's note
    /// is selected for the note editor, and heard; nothing is added or deleted.
    pub fn select_step_note(&mut self, t: usize, beat: i32, step: i32) {
        let Some(track) = self.track(t) else { return };
        let at_step = track.notes.iter().position(|n| n.beat == beat && n.step == step);
        self.selection.cursor = Some(Cursor { track: t, beat, step });
        self.selection.note_focus = Some(NoteFocus::Step { track: t, beat, step });
        self.selection.selected = at_step.map(|n| (t, n));
        if let Some(n) = at_step {
            self.audition(t, Some(n));
        }
    }

    pub fn enter(&mut self) {
        let Some(c) = self
            .selection
            .cursor
            .or_else(|| self.pattern().filter(|p| !p.tracks.is_empty()).map(|_| Cursor { track: 0, beat: 0, step: 0 }))
        else {
            return;
        };
        self.selection.cursor = Some(c);
        self.toggle_note(c.track, c.beat, c.step);
    }

    pub fn move_cursor(&mut self, direction: Move) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let tracks = pattern.tracks.len();
        if tracks == 0 {
            return;
        }
        let beats = pattern.beat_count.max(1);
        let Some(mut c) = self.selection.cursor else {
            self.selection.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
            return;
        };
        let spb = pattern.tracks[c.track.min(tracks - 1)].steps_per_beat.max(1);
        match direction {
            Move::Right => {
                c.step += 1;
                if c.step >= spb {
                    c.step = 0;
                    c.beat = (c.beat + 1) % beats;
                }
            }
            Move::Left => {
                c.step -= 1;
                if c.step < 0 {
                    c.step = spb - 1;
                    c.beat = (c.beat - 1).rem_euclid(beats);
                }
            }
            Move::Up => c.track = c.track.saturating_sub(1),
            Move::Down => c.track = (c.track + 1).min(tracks - 1),
        }
        let spb = pattern.tracks[c.track].steps_per_beat.max(1);
        c.step = c.step.min(spb - 1);
        self.selection.cursor = Some(c);
        self.selection.note_focus = Some(NoteFocus::Step { track: c.track, beat: c.beat, step: c.step });
        let note = pattern.tracks[c.track].notes.iter().position(|n| n.beat == c.beat && n.step == c.step);
        self.selection.selected = note.map(|n| (c.track, n));
        if let Some(n) = note {
            self.audition(c.track, Some(n));
        }
    }

    /// Shift+arrows on the selected note: ↑↓ velocity, ←→ pitch.
    pub fn nudge(&mut self, direction: Move) {
        let Some((t, n)) = self.selection.selected else {
            self.message = "select a note first".into();
            return;
        };
        let nudge = match direction {
            Move::Up => Nudge::VelocityUp,
            Move::Down => Nudge::VelocityDown,
            Move::Right => Nudge::PitchUp,
            Move::Left => Nudge::PitchDown,
        };
        if self.editor.nudge_note(self.pattern_index, t, n, nudge) {
            self.edited();
            if let Some(note) = self.track(t).and_then(|track| track.notes.get(n)) {
                self.message = format!("velocity {}  pitch {}", note.velocity, note.pitch);
            }
            self.audition(t, Some(n));
        }
    }

    /// Pattern > Compact tracks (the tools panel's button): every track
    /// shortened to its shortest loop, when its notes repeat one.
    pub fn compact_tracks(&mut self) {
        let can_compact = |track: &Track| track.clone().compact_loop();
        if self.pattern().is_some_and(|p| p.tracks.iter().any(can_compact)) {
            self.undoable(Self::compact_tracks_now);
        } else {
            self.compact_tracks_now();
        }
    }

    fn compact_tracks_now(&mut self) {
        let count = self.editor.compact_pattern(self.pattern_index);
        if count == 0 {
            self.message = "no track repeats a shorter loop".into();
            return;
        }
        self.selection.selected = None;
        self.clamp_selection();
        self.edited();
        self.message = format!("compacted {count} track{}", if count == 1 { "" } else { "s" });
    }

    /// Pattern > Clear pattern: every track's notes removed, the tracks and
    /// their attributes kept.
    pub fn clear_pattern(&mut self) {
        let has_notes = self.pattern().is_some_and(|p| p.tracks.iter().any(|t| !t.notes.is_empty()));
        if has_notes {
            self.undoable(Self::clear_pattern_now);
        } else {
            self.clear_pattern_now();
        }
    }

    fn clear_pattern_now(&mut self) {
        if !self.pattern().is_some_and(|p| p.tracks.iter().any(|t| !t.notes.is_empty())) {
            self.message = "No notes to clear".into();
            return;
        }
        self.editor.clean_pattern(self.pattern_index);
        self.selection.selected = None;
        self.clamp_selection();
        self.edited();
        self.message = "Cleared notes".into();
    }
    /// Clear the zone, else the cursor step (Ctrl+Z undoes it).
    pub fn delete_step(&mut self) {
        if self.piano_roll_open() {
            self.roll_delete();
        } else if let Some(zone) = self.selection.zone {
            self.delete_zone(&zone);
        } else if self.selection.cursor.is_some() {
            self.undoable(Self::delete_step_now);
        }
    }

    /// L: accent the notes of the cursor step, or take their accent off.
    pub fn toggle_accent(&mut self) {
        let Some(c) = self.selection.cursor else { return };
        let name = self.track(c.track).map(|track| track.name.clone()).unwrap_or_default();
        let at = beat_label(c.beat, c.step);
        match self.editor.toggle_accent(self.pattern_index, c.track, c.beat, c.step) {
            Some(accent) => {
                self.edited();
                self.message = format!("{} - {name} @ {at}", if accent { "Accent" } else { "No accent" });
            }
            None => self.message = format!("no note to accent - {name} @ {at}"),
        }
    }

    /// How many notes step `c` holds.
    pub(super) fn step_note_count(&self, c: Cursor) -> usize {
        self.track(c.track)
            .map_or(0, |track| track.notes.iter().filter(|n| n.beat == c.beat && n.step == c.step).count())
    }

    fn delete_step_now(&mut self) {
        let Some(c) = self.selection.cursor else { return };
        let count = self.step_note_count(c);
        let name = self.track(c.track).map(|track| track.name.clone()).unwrap_or_default();
        if self.editor.delete_step(self.pattern_index, c.track, c.beat, c.step) {
            self.edited();
            self.message = format!("Deleted {} - {name} @ {}", notes_label(count), beat_label(c.beat, c.step));
        }
        self.selection.selected = None;
    }

    pub fn deselect(&mut self) {
        self.selection.clear();
    }

    /// The track the cursor is on, else the selected note's.
    pub(super) fn focus_track(&self) -> Option<usize> {
        self.selection.focus_track().filter(|&t| self.track(t).is_some())
    }

    pub fn toggle_row(&mut self, t: usize, key: &str) {
        if self.track(t).is_some() && self.editor.toggle_track(self.pattern_index, t, key) {
            self.edited();
        }
    }

    pub fn change_length(&mut self, delta: i32) {
        let Some(beats) = self.pattern().map(|p| p.beat_count) else { return };
        self.set_length(beats + delta);
    }

    pub(super) fn clamp_cursor_to_grid(&mut self) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let beats = pattern.beat_count.max(1);
        if let Some(c) = &mut self.selection.cursor {
            c.beat = c.beat.min(beats - 1);
            let spb = pattern.tracks.get(c.track).map_or(1, |t| t.steps_per_beat.max(1));
            c.step = c.step.min(spb - 1);
        }
        self.clamp_selection();
    }

    pub fn next_grid(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.incr_steps_per_beat(self.pattern_index, t) {
            self.edited();
            self.clamp_cursor_to_grid();
        }
    }

    pub fn shorten_loop(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.shorten_loop(self.pattern_index, t) {
            self.edited();
        }
    }

    /// Ctrl+T: a new track named [`DEFAULT_TRACK_NAME`] (the menus and the
    /// button let the user choose the instrument: `session_grid.rs`).
    pub fn add_track(&mut self) {
        self.add_track_named(DEFAULT_TRACK_NAME);
    }

    /// A new track named `name`, the cursor on it.
    pub fn add_track_named(&mut self, name: &str) {
        if self.pattern().is_none() {
            return;
        }
        if !self.editor.has_track_room(self.pattern_index) {
            self.message = format!("a pattern holds at most {MAX_TRACKS} tracks");
            return;
        }
        if let Some(t) = self.editor.add_track(self.pattern_index, name, 4) {
            self.selection.put_cursor(Cursor { track: t, beat: 0, step: 0 });
            self.edited();
        }
    }

    /// Remove the focused track, never the last one.
    pub fn remove_track(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.pattern().is_some_and(|p| p.tracks.len() <= 1) {
            self.message = "Cannot delete the last track".into();
        } else {
            self.undoable(|s| s.remove_track_now(t));
        }
    }

    fn remove_track_now(&mut self, t: usize) {
        if self.editor.remove_track(self.pattern_index, t) {
            self.selection.selected = None;
            self.edited();
            self.clamp_selection();
            self.message = "Track deleted".into();
        }
    }

    pub(super) fn clear_track_now(&mut self, t: usize) {
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return };
        if self.editor.clean_track(self.pattern_index, t) {
            self.selection.selected = None;
            self.edited();
            self.message = format!("Cleared notes on \"{name}\"");
        }
    }

    pub fn new_pattern(&mut self) {
        let names = ["KICK", "SNARE", "CHH", "OHH", "CYM", "HTOM", "LTOM", "CLAP"];
        let next = self.editor.library().patterns.len();
        let name = format!("NewPat_{next}");
        self.pattern_index = self.editor.batch(&format!("Add pattern \"{}\"", name), |editor| {
            let p = editor.add_pattern(Some(&name));
            for track in names {
                editor.add_track(p, track, 4);
            }
            p
        });
        self.deselect();
        self.edited();
        self.chrome.current_page = 0;
    }

    pub fn duplicate_pattern(&mut self) {
        if let Some(index) = self.editor.duplicate_pattern(self.pattern_index) {
            self.pattern_index = index;
            self.deselect();
            self.edited();
            self.chrome.current_page = 0;
        }
    }

    /// Ctrl+C: the notes of the zone, else of the cursor step (with Shift,
    /// or no cursor: the track).
    pub fn copy(&mut self, track: bool) {
        if !track && self.piano_roll_open() {
            return self.roll_copy();
        }
        if !track && let Some(zone) = self.selection.zone {
            return self.copy_zone(&zone);
        }
        let Some(t) = self.focus_track() else { return };
        let Some(source) = self.track(t).cloned() else { return };
        let name = source.name.clone();
        if track || self.selection.cursor.is_none() {
            self.message = format!("copied track \"{name}\" ({} notes)", source.notes.len());
            self.selection.clipboard = Some(Clipboard::Track(Box::new(source)));
        } else if let Some(c) = self.selection.cursor {
            let notes: Vec<Note> =
                source.notes.iter().filter(|n| n.beat == c.beat && n.step == c.step).cloned().collect();
            self.message = format!("copied {} note(s) — {name} @ {}.{}", notes.len(), c.beat + 1, c.step + 1);
            self.selection.clipboard = Some(Clipboard::Step(notes));
        }
    }

    /// Ctrl+V: the copied step or zone at the cursor, or the copied track
    /// after the focused one (with Shift, only a track).
    pub fn paste(&mut self, track_only: bool) {
        let p = self.pattern_index;
        match self.selection.clipboard.clone() {
            None => self.message = "clipboard is empty".into(),
            Some(Clipboard::Track(_)) if !self.editor.has_track_room(p) => {
                self.message = format!("a pattern holds at most {MAX_TRACKS} tracks");
            }
            Some(Clipboard::Track(track)) => {
                let after = self.focus_track().map_or_else(|| self.pattern().map_or(0, |p| p.tracks.len()), |t| t + 1);
                if let Some(t) = self.editor.paste_track(p, after, &track) {
                    self.selection.put_cursor(Cursor { track: t, beat: 0, step: 0 });
                    self.edited();
                }
            }
            Some(Clipboard::Step(_) | Clipboard::Block(_) | Clipboard::Roll(_)) if track_only => {
                self.message = "no track copied".into();
            }
            Some(Clipboard::Roll(block)) if self.piano_roll_open() => self.roll_paste(&block),
            Some(Clipboard::Roll(_)) => self.message = "notes copied in the piano roll paste in the piano roll".into(),
            Some(Clipboard::Step(_) | Clipboard::Block(_)) if self.piano_roll_open() => {
                self.message = "notes copied in the grid paste in the grid".into();
            }
            Some(Clipboard::Block(block)) => self.paste_block(&block),
            Some(Clipboard::Step(notes)) => {
                let c = self.selection.cursor.unwrap_or(Cursor { track: 0, beat: 0, step: 0 });
                if self.editor.paste_step_notes(p, c.track, c.beat, c.step, &notes) {
                    self.selection.put_cursor(c);
                    self.edited();
                }
            }
        }
    }
}
