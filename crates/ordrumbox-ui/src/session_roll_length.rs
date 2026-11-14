//! Drawing a note out in the piano roll: a press on the right edge of a
//! note, dragged sideways, sets how long the note is held (`lengthSteps`),
//! by quarters of a step, up to the end of the pattern. The whole drag is
//! one undo step.

use ordrumbox_core::json::Value;
use ordrumbox_core::steps::absolute_step;

use super::Session;
use crate::control::fmt;
use crate::editors::MIN_LENGTH_SLIDER;
use crate::piano_roll::steps_per_beat;

/// The steps a drag changes a length by: a quarter of a step.
const LENGTH_SNAP_STEPS: f64 = 0.25;

/// The right edge of a note, pressed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RollLengthDrag {
    track: usize,
    note: usize,
    /// Where the press was.
    x: i32,
    /// The note's length at the press, in steps.
    start_steps: f64,
    /// The width of a step, px.
    cell: i32,
}

impl Session {
    /// A press at `x` on the right edge of note `n` of track `t`, a step
    /// being `cell` px wide: the note is selected, and a drag draws it out.
    pub(super) fn roll_length_press(&mut self, t: usize, n: usize, x: i32, cell: i32) {
        let Some(note) = self.track(t).and_then(|track| track.notes.get(n)) else { return };
        let start_steps = note.held_steps();
        self.roll_select(t, n);
        self.roll.length_drag = Some(RollLengthDrag { track: t, note: n, x, start_steps, cell });
    }

    /// The mouse moved to `x` with the edge of a note pressed: the note is
    /// as long as it was, and the steps dragged; whether to redraw.
    pub(super) fn roll_length_move(&mut self, x: i32) -> bool {
        let Some(RollLengthDrag { track: t, note: n, x: press_x, start_steps, cell }) = self.roll.length_drag else {
            return false;
        };
        let Some(track) = self.track(t) else { return false };
        let Some(note) = track.notes.get(n) else { return false };
        let spb = steps_per_beat(track);
        let pattern_steps = self.pattern().map_or(1, |p| i64::from(p.beat_count.max(1))) * i64::from(spb);
        // To the end of the pattern at most: a note does not outlast the loop it is in.
        let room = (pattern_steps - absolute_step(note.beat, note.step, spb)) as f64;
        let dragged = f64::from(x - press_x) / f64::from(cell.max(1));
        let snapped = ((start_steps + dragged) / LENGTH_SNAP_STEPS).round() * LENGTH_SNAP_STEPS;
        let length = snapped.clamp(MIN_LENGTH_SLIDER, room.max(MIN_LENGTH_SLIDER));
        if length == note.length_steps {
            return false;
        }
        let desc = format!("Edit note length on {}", track.name);
        let updates = [("lengthSteps", Value::Number(length))];
        if self.editor.update_note(self.pattern_index, t, n, &updates, Some(&desc), true) {
            self.edited();
            self.roll.gauge = Some(format!("length {}", fmt(length)));
        }
        true
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
                {"name":"BASS","notes":[{"beat":1},{"beat":3,"step":2,"lengthSteps":2}]}]}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session.select_roll_track(0);
        session.toggle_piano_roll();
        session.editor.set_clock(Some(0.0));
        session
    }

    fn lengths(s: &Session) -> Vec<f64> {
        s.track(0).unwrap().notes.iter().map(|n| n.length_steps).collect()
    }

    /// The right edge of note `n`, the middle of its body, its row's middle, and a step's width.
    fn note_points(s: &Session, n: usize) -> (i32, i32, i32, i32) {
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let r = layout.note_rect(&view, n).unwrap();
        (r.x + r.w - 1, r.x + r.w / 2, r.y + r.h / 2, layout.cell)
    }

    #[test]
    fn a_drag_of_the_right_edge_draws_the_note_out_in_one_undo_step() {
        let mut s = session();
        let (edge, _, y, cell) = note_points(&s, 0);
        assert!(s.roll_click(SIZE, edge, y, Button::Left));
        assert_eq!(s.selection.selected, Some((0, 0)), "the note is selected at the press");
        for steps in [1, 2] {
            assert!(s.roll_drag_move(edge + steps * cell, y));
        }
        s.mouse_up();
        assert_eq!(lengths(&s), [3.0, 2.0]);
        // The body of the note drawn out is still the note: a click on it selects it, adds none.
        let (_, middle, y, _) = note_points(&s, 0);
        s.selection.selected = None;
        s.roll_click(SIZE, middle, y, Button::Left);
        s.mouse_up();
        assert_eq!((s.selection.selected, lengths(&s).len()), (Some((0, 0)), 2));
        s.undo();
        assert_eq!(lengths(&s), [1.0, 2.0]);
    }

    #[test]
    fn a_note_is_no_shorter_than_a_quarter_step_and_ends_with_the_pattern() {
        let mut s = session();
        let (edge, _, y, cell) = note_points(&s, 0);
        s.roll_click(SIZE, edge, y, Button::Left);
        s.roll_drag_move(edge - 4 * cell, y);
        s.mouse_up();
        assert_eq!(lengths(&s)[0], MIN_LENGTH_SLIDER);
        // The second note starts 2 steps before the end of the pattern.
        let (edge, _, y, cell) = note_points(&s, 1);
        s.roll_click(SIZE, edge, y, Button::Left);
        s.roll_drag_move(edge + 9 * cell, y);
        s.mouse_up();
        assert_eq!(lengths(&s)[1], 2.0);
        s.roll_drag_move(edge + 9 * cell, y);
        assert_eq!(lengths(&s)[1], 2.0, "the drag ended with the press");
    }
}
