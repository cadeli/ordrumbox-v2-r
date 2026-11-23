//! A note pressed in the piano roll: dragged, it moves in time and in pitch
//! (one undo step for the whole drag), never onto another note; let go
//! where it was, it is a click, which selects it; a second click soon after
//! on the same note deletes it.

use std::time::Instant;

use ordrumbox_core::json::Value;
use ordrumbox_core::steps::absolute_step;

use super::Session;
use super::session_panel::DOUBLE_CLICK;
use crate::note_drag::NoteMove;
use crate::piano_roll::{TOTAL_KEYS, note_row, steps_per_beat};

/// A note being pressed in the piano roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RollDrag {
    track: usize,
    note: usize,
    motion: NoteMove,
}

impl Session {
    /// A press at `(x, y)` on note `n` of track `t`, a cell being `cell` px
    /// (width, height): a drag moves the note, a click selects it.
    pub(super) fn roll_note_press(&mut self, t: usize, n: usize, (x, y): (i32, i32), cell: (i32, i32)) {
        let Some(track) = self.track(t) else { return };
        let Some(note) = track.notes.get(n) else { return };
        let from = (absolute_step(note.beat, note.step, steps_per_beat(track)), note_row(note, track));
        self.roll.drag = Some(RollDrag { track: t, note: n, motion: NoteMove::new((x, y), from, cell) });
    }

    /// The mouse moved to `(x, y)` with a note pressed: the note at the cell
    /// as far from its own as the pointer is from the press, unless another
    /// note is there; whether to redraw.
    pub(super) fn roll_note_move(&mut self, x: i32, y: i32) -> bool {
        let Some(RollDrag { track: t, note: n, mut motion }) = self.roll.drag else { return false };
        let Some(spb) = self.track(t).map(steps_per_beat) else { return false };
        let was_moved = motion.has_moved();
        let (step, row) = motion.target((x, y), (self.roll_step_count(spb), TOTAL_KEYS));
        self.roll.drag = Some(RollDrag { track: t, note: n, motion });
        if motion.has_moved() && !was_moved {
            // A drag makes no click: the note is selected as it leaves its cell.
            self.roll_select(t, n);
        }
        let occupied = self.roll_note_at(t, step, row).is_some_and(|other| other != n);
        let Some(note) = self.track(t).and_then(|track| track.notes.get(n)) else { return false };
        let pitch = self.roll_pitch(t, row);
        let (beat, beat_step) = self.roll_beat(t, step);
        if occupied || (note.beat, note.step, note.pitch) == (beat, beat_step, pitch) {
            return false;
        }
        let name = self.track(t).map(|track| track.name.clone()).unwrap_or_default();
        let desc = format!("Move note on {name}");
        let updates = [("beat", Value::from(beat)), ("step", Value::from(beat_step)), ("pitch", Value::Number(pitch))];
        if self.editor.update_note(self.pattern_index, t, n, &updates, Some(&desc), true) {
            self.edited();
            self.audition(t, Some(n));
        }
        self.roll_follow();
        self.roll.gauge = Some(self.pitch_gauge(t, n));
        true
    }

    /// The press on a note ended: when the note did not move, a click — it
    /// selects the note and plays it, or deletes it when it is the second
    /// click on it.
    pub(super) fn roll_note_release(&mut self) {
        let Some(RollDrag { track: t, note: n, motion }) = self.roll.drag.take() else { return };
        self.roll.gauge = None;
        if motion.has_moved() {
            self.roll.last_note_click = None;
            return;
        }
        let now = Instant::now();
        let last = self.roll.last_note_click.replace(((t, n), now));
        if last.is_some_and(|(note, at)| note == (t, n) && now - at < DOUBLE_CLICK) {
            self.roll.last_note_click = None;
            self.roll_delete_note(t, n);
        } else if self.track(t).is_some_and(|track| n < track.notes.len()) {
            self.roll_select(t, n);
            self.audition(t, Some(n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::test_support::SIZE;
    use crate::ui_input::Button;
    use ordrumbox_core::Library;

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","beatCount":4,"tracks":[
                {"name":"BASS","notes":[{"beat":1},{"beat":2,"pitch":2}]}]}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session.select_roll_track(0);
        session.toggle_piano_roll();
        session.editor.set_clock(Some(0.0));
        session
    }

    /// Each note: beat, step, pitch.
    fn notes(s: &Session) -> Vec<(i32, i32, f64)> {
        s.track(0).unwrap().notes.iter().map(|n| (n.beat, n.step, n.pitch)).collect()
    }

    /// The middle of note `n`, and a cell's width and height.
    fn note_point(s: &Session, n: usize) -> ((i32, i32), (i32, i32)) {
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let r = layout.note_rect(&view, n).unwrap();
        ((r.x + r.w / 2, r.y + r.h / 2), (layout.cell, layout.row))
    }

    #[test]
    fn a_drag_moves_the_note_in_time_and_pitch_in_one_undo_step() {
        let mut s = session();
        let ((x, y), (w, h)) = note_point(&s, 0);
        assert!(s.roll_click(SIZE, x, y, Button::Left));
        assert!(!s.roll_drag_move(x + 2, y + 1), "within its cell: nothing yet");
        assert!(s.roll_drag_move(x + w, y));
        assert!(s.roll_drag_move(x + 3 * w, y - 2 * h));
        s.mouse_up();
        assert_eq!(notes(&s), [(1, 3, 2.0), (2, 0, 2.0)], "three steps later, two semitones up");
        assert_eq!(s.selection.selected, Some((0, 0)), "dragged: selected, not deleted");
        s.undo();
        assert_eq!(notes(&s), [(1, 0, 0.0), (2, 0, 2.0)], "the whole drag is one step");
    }

    #[test]
    fn a_note_does_not_move_onto_another() {
        let mut s = session();
        let ((x, y), (w, h)) = note_point(&s, 0);
        s.roll_click(SIZE, x, y, Button::Left);
        // The second note is a beat (4 steps) later, two semitones up.
        s.roll_drag_move(x + 4 * w, y - 2 * h);
        assert_eq!(notes(&s)[0], (1, 0, 0.0));
        s.roll_drag_move(x + 4 * w, y - 3 * h);
        s.mouse_up();
        assert_eq!(notes(&s), [(2, 0, 3.0), (2, 0, 2.0)]);
    }

    #[test]
    fn the_pointer_shows_what_a_drag_of_a_note_does() {
        use crate::ui_input::PointerShape::{Arrow, Move, ResizeHorizontal};
        let mut s = session();
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let r = layout.note_rect(&view, 0).unwrap();
        let (edge, (x, y)) = (r.x + r.w - 1, (r.x + 2, r.y + r.h / 2));
        assert_eq!(s.pointer_shape(SIZE, x, y), Move, "the body: a drag moves it");
        assert_eq!(s.pointer_shape(SIZE, edge, y), ResizeHorizontal, "the right edge: its length");
        assert_eq!(s.pointer_shape(SIZE, x, y + 3 * layout.row), Arrow, "an empty cell");
        // Held, the shape stays wherever the pointer goes.
        s.roll_click(SIZE, x, y, Button::Left);
        assert_eq!(s.pointer_shape(SIZE, 0, 0), Move);
        s.mouse_up();
        s.roll_click(SIZE, edge, y, Button::Left);
        assert_eq!(s.pointer_shape(SIZE, 0, 0), ResizeHorizontal);
        s.mouse_up();
        assert_eq!(s.pointer_shape(SIZE, 0, 0), Arrow);
    }

    #[test]
    fn a_click_selects_the_note_and_a_double_click_deletes_it() {
        let mut s = session();
        let ((x, y), _) = note_point(&s, 1);
        s.roll_click(SIZE, x, y, Button::Left);
        s.mouse_up();
        assert_eq!((s.selection.selected, notes(&s).len()), (Some((0, 1)), 2));
        // A click on the note already selected, long after: it stays.
        s.roll.last_note_click = s.roll.last_note_click.map(|(note, at)| (note, at - DOUBLE_CLICK));
        s.roll_click(SIZE, x, y, Button::Left);
        s.mouse_up();
        assert_eq!((s.selection.selected, notes(&s).len()), (Some((0, 1)), 2));
        // A second click soon after: deleted.
        s.roll_click(SIZE, x, y, Button::Left);
        s.mouse_up();
        assert_eq!((s.selection.selected, notes(&s)), (None, vec![(1, 0, 0.0)]));
    }
}
