//! A zone of the piano roll, drawn with Shift+drag from a cell: steps of the
//! page shown and keys, on the roll's track. Ctrl+C copies its notes (with
//! no zone: the selected note), Ctrl+X cuts them, Delete clears them; Ctrl+V
//! pastes them with the zone's top left corner on the roll's cursor,
//! replacing what was under them. Each edit is one undo step.

use crate::scale::Scale;
use ordrumbox_core::Track;
use ordrumbox_core::steps::absolute_step;
use ordrumbox_logic::block::step_units;
use ordrumbox_logic::roll_block::{RollBlock, RollZone, note_key};

use super::Session;
use super::session_labels::notes_label;
use super::session_selection::Clipboard;
use super::session_song::DRAG_START_PX;
use super::session_zone::beats_label;
use crate::note_drag::{MIDDLE_C, midi_name};
use crate::piano_roll::{RollHit, RollSpan, TOTAL_KEYS, key_row, row_key, steps_per_beat};
use crate::ui_input::Button;

/// A Shift+press on a cell of the piano roll that may become a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RollZoneDrag {
    /// The cell pressed (step, row): a corner of the zone.
    anchor: (i64, i32),
    press: (i32, i32),
    /// The pointer has really moved: this is no longer a Shift+click.
    started: bool,
    /// What it was pressed on; another edit cancels the drag.
    revision: u64,
}

impl Session {
    /// The zone drawn over the piano roll, when it is on the roll's track.
    pub(super) fn roll_zone(&self) -> Option<RollZone> {
        self.selection.roll_zone.filter(|z| Some(z.track) == self.roll_track())
    }

    /// Where `zone` is on the roll of `track`: its steps and rows.
    pub(super) fn roll_span(&self, zone: &RollZone, track: &Track) -> RollSpan {
        let step_length = step_units(track.steps_per_beat);
        RollSpan {
            start_step: zone.start_units.div_euclid(step_length),
            end_step: (zone.end_units + step_length - 1).div_euclid(step_length),
            low_row: key_row(zone.low_key),
            high_row: key_row(zone.high_key),
        }
    }

    /// A Shift+press on the piano roll: on a cell, select its note and
    /// start a zone there; elsewhere, a plain press. Whether it was the roll's.
    pub fn roll_shift_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let layout = self.roll_layout(size);
        let Some(view) = self.roll_view(&layout) else { return false };
        let Some(RollHit::Cell { step, row }) = layout.hit(&view, x, y) else {
            return self.roll_click(size, x, y, Button::Left);
        };
        let Some(t) = self.roll_track() else { return false };
        self.selection.roll_zone = None;
        if let Some(n) = self.roll_note_at(t, step, row) {
            self.roll_select(t, n);
        }
        let revision = self.editor.revision();
        self.roll.zone_drag = Some(RollZoneDrag { anchor: (step, row), press: (x, y), started: false, revision });
        true
    }

    /// The mouse moved with a cell Shift+pressed: the zone from it to the
    /// cell under the mouse (the nearest of the page); whether to redraw.
    pub(super) fn roll_zone_move(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(drag) = &mut self.roll.zone_drag else { return false };
        if drag.revision != self.editor.revision() {
            self.roll.zone_drag = None;
            return false;
        }
        let (dx, dy) = (x - drag.press.0, y - drag.press.1);
        drag.started |= dx.abs() >= DRAG_START_PX || dy.abs() >= DRAG_START_PX;
        let drag = *drag;
        if !drag.started {
            return false;
        }
        let (Some(t), Some(pattern)) = (self.roll_track(), self.pattern()) else { return false };
        let layout = self.roll_layout(size);
        let Some((step, row)) = self.roll_view(&layout).and_then(|v| layout.nearest_cell(&v, x, y)) else {
            return false;
        };
        let (anchor_step, anchor_row) = drag.anchor;
        let zone = RollZone::between(pattern, t, (anchor_step, row_key(anchor_row)), (step, row_key(row)));
        let changed = zone != self.selection.roll_zone;
        self.selection.roll_zone = zone;
        // The zone, not a note, is what the edits take.
        self.selection.selected = None;
        changed
    }

    /// The zone of the roll's selected note alone.
    fn roll_note_zone(&self) -> Option<RollZone> {
        let t = self.roll_track()?;
        let (st, n) = self.selection.selected.filter(|&(st, _)| st == t)?;
        let track = self.track(st)?;
        let note = track.notes.get(n)?;
        let cell = (absolute_step(note.beat, note.step, steps_per_beat(track)), note_key(note, track));
        RollZone::between(self.pattern()?, t, cell, cell)
    }

    /// Ctrl+C in the piano roll: the zone's notes, else the selected note.
    pub(super) fn roll_copy(&mut self) {
        let Some(zone) = self.roll_zone().or_else(|| self.roll_note_zone()) else {
            self.message = "draw a zone (Shift+drag) or select a note to copy".into();
            return;
        };
        let Some(pattern) = self.pattern() else { return };
        let block = RollBlock::copy(pattern, &zone);
        self.message = format!("copied {} — {}", notes_label(block.note_count()), self.roll_zone_label(&zone));
        self.selection.clipboard = Some(Clipboard::Roll(block));
    }

    /// Ctrl+X in the piano roll: the zone's notes, else the selected note,
    /// copied and cleared.
    pub(super) fn roll_cut(&mut self) {
        let Some(zone) = self.roll_zone().or_else(|| self.roll_note_zone()) else {
            self.message = "draw a zone (Shift+drag) or select a note to cut".into();
            return;
        };
        self.roll_copy();
        self.undoable(|s| s.roll_delete_zone(&zone));
    }

    /// The notes of `zone` cleared.
    pub(super) fn roll_delete_zone(&mut self, zone: &RollZone) {
        let removed = self.editor.delete_roll_zone(self.pattern_index, zone);
        if removed > 0 {
            self.edited();
        }
        self.selection.selected = None;
        self.message = format!("Deleted {} — {}", notes_label(removed), self.roll_zone_label(zone));
    }

    /// Ctrl+V in the piano roll: `block` with its top left corner on the
    /// roll's cursor (else the selected note). The zone then shows where its
    /// notes went.
    pub(super) fn roll_paste(&mut self, block: &RollBlock) {
        let Some(t) = self.roll_track() else { return };
        let at = self.roll.cursor.or_else(|| {
            let zone = self.roll_note_zone()?;
            let track = self.track(t)?;
            Some((zone.start_units / step_units(track.steps_per_beat), key_row(zone.high_key)))
        });
        let Some((step, row)) = at else {
            self.message = "put the cursor (arrows) or select a note where to paste".into();
            return;
        };
        let keys = row_key(0)..=row_key(TOTAL_KEYS - 1);
        let pasted = self.editor.paste_roll_block(self.pattern_index, t, (step, row_key(row)), block, keys);
        if pasted.placed > 0 || pasted.zone.is_some() {
            self.edited();
        }
        self.selection.selected = None;
        self.selection.roll_zone = pasted.zone;
        self.message = match pasted.dropped {
            0 => format!("pasted {}", notes_label(pasted.placed)),
            dropped => {
                format!("pasted {}, {dropped} past the pattern or the keyboard left out", notes_label(pasted.placed))
            }
        };
    }

    /// `BASS, beats 1-2, C2-G2`: the track, the time and the keys of a zone.
    fn roll_zone_label(&self, zone: &RollZone) -> String {
        let name = self.track(zone.track).map(|t| t.name.as_str()).unwrap_or_default();
        let key = |key: i32| midi_name(key + MIDDLE_C as i32);
        let keys = if zone.low_key == zone.high_key {
            key(zone.low_key)
        } else {
            format!("{}-{}", key(zone.low_key), key(zone.high_key))
        };
        format!("{name}, {}, {keys}", beats_label(zone.start_units, zone.end_units))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::menu::Command;
    use ordrumbox_core::Library;

    use crate::test_support::SIZE;

    /// BASS, an octave down: C, E, G on beat 1, D on beat 2 (keys -12, -8,
    /// -5 and -10, rows 36, 40, 43 and 38).
    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","beatCount":8,"tracks":[{"name":"KICK","notes":[{}]},
                {"name":"BASS","pitch":-12,"beatCount":8,"notes":[{"beat":0,"pitch":0},{"beat":0,"pitch":4},
                {"beat":0,"step":2,"pitch":7},{"beat":1,"pitch":2}]}]}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session.select_roll_track(1);
        session.toggle_piano_roll();
        session
    }

    /// The middle of the cell at `step`, `row`.
    fn cell(s: &Session, step: i64, row: i32) -> (i32, i32) {
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let (start, _) = view.page_steps();
        let x = layout.grid.x + (step - start) as i32 * layout.cell + layout.cell / 2;
        let y = layout.grid.y + (view.top_row - row) * layout.row + layout.row / 2;
        (x, y)
    }

    /// Shift+drag from one cell to another.
    fn draw_zone(s: &mut Session, from: (i64, i32), to: (i64, i32)) {
        let (x0, y0) = cell(s, from.0, from.1);
        let (x1, y1) = cell(s, to.0, to.1);
        assert!(s.roll_shift_press(SIZE, x0, y0));
        s.mouse_move(SIZE, x1, y1, true);
        s.mouse_up();
    }

    /// Beat, step and key of each of BASS's notes.
    fn notes(s: &Session) -> Vec<(i32, i32, i32)> {
        let track = s.track(1).unwrap();
        let mut notes: Vec<_> = track.notes.iter().map(|n| (n.beat, n.step, note_key(n, track))).collect();
        notes.sort_unstable();
        notes
    }

    #[test]
    fn a_shift_click_selects_the_note_and_draws_no_zone() {
        let mut s = session();
        let (x, y) = cell(&s, 0, 40);
        assert!(s.roll_shift_press(SIZE, x, y));
        assert!(!s.mouse_move(SIZE, x + 1, y + 1, true), "a small move is still a click");
        s.mouse_up();
        assert_eq!(s.selection.roll_zone, None);
        assert_eq!(s.selection.selected, Some((1, 1)), "E, selected");
        assert_eq!(notes(&s).len(), 4, "a Shift+click adds or deletes nothing");
    }

    #[test]
    fn a_chord_copied_and_pasted_elsewhere_in_one_undo_step() {
        let mut s = session();
        // Beat 1, C to G: the chord, not D.
        draw_zone(&mut s, (3, 36), (0, 43));
        let zone = s.roll_zone().expect("a zone");
        assert_eq!((zone.low_key, zone.high_key, zone.end_units), (-12, -5, 840));
        assert!(s.roll_view(&s.roll_layout(SIZE)).unwrap().zone.is_some(), "it is drawn");
        s.run(Command::CopyStep);
        assert!(matches!(&s.selection.clipboard, Some(Clipboard::Roll(b)) if b.note_count() == 3));
        assert_eq!(s.message, "copied 3 notes — BASS, beats 1-2, C3-G3");
        // Its top left corner on beat 3, a fourth up.
        s.roll.cursor = Some((8, 48));
        s.run(Command::Paste);
        let beat_3: Vec<_> = notes(&s).into_iter().filter(|n| n.0 == 2).collect();
        assert_eq!(beat_3, [(2, 0, -7), (2, 0, -3), (2, 2, 0)]);
        assert_eq!(s.roll_zone().map(|z| (z.low_key, z.high_key)), Some((-7, 0)), "the zone shows the paste");
        s.run(Command::Undo);
        assert_eq!(notes(&s).len(), 4, "one undo step");
    }

    #[test]
    fn cut_and_delete_clear_the_zone_in_one_undo_step() {
        let mut s = session();
        draw_zone(&mut s, (0, 36), (7, 38));
        s.run(Command::Cut);
        assert_eq!(notes(&s), [(0, 0, -8), (0, 2, -5)], "C and D cut");
        s.run(Command::Undo);
        assert_eq!(notes(&s).len(), 4);
        draw_zone(&mut s, (0, 40), (3, 43));
        s.roll_delete();
        assert_eq!(notes(&s), [(0, 0, -12), (1, 0, -10)], "E and G deleted");
        s.run(Command::Undo);
        assert_eq!(notes(&s).len(), 4);
    }

    #[test]
    fn without_a_zone_the_selected_note_is_copied() {
        let mut s = session();
        let (x, y) = cell(&s, 4, 38);
        s.roll_click(SIZE, x, y, Button::Left);
        s.mouse_up();
        assert_eq!(s.selection.selected, Some((1, 3)), "D, selected");
        s.run(Command::CopyStep);
        s.roll.cursor = Some((12, 40));
        s.run(Command::Paste);
        assert!(notes(&s).contains(&(3, 0, -8)), "D pasted as E on beat 4");
        // Notes of the grid do not paste in the roll, nor the roll's in the grid.
        s.selection.clipboard = Some(Clipboard::Step(Vec::new()));
        s.run(Command::Paste);
        assert_eq!(s.message, "notes copied in the grid paste in the grid");
    }

    #[test]
    fn a_click_escape_or_another_track_leaves_the_zone() {
        let mut s = session();
        draw_zone(&mut s, (0, 36), (3, 43));
        s.roll_escape();
        assert_eq!(s.roll_zone(), None);
        assert!(s.piano_roll_open(), "Esc first leaves the zone, not the roll");
        draw_zone(&mut s, (0, 36), (3, 43));
        let (x, y) = cell(&s, 6, 39);
        s.roll_click(SIZE, x, y, Button::Left);
        assert_eq!(s.roll_zone(), None);
        draw_zone(&mut s, (0, 36), (3, 43));
        s.select_roll_track(0);
        assert_eq!(s.selection.roll_zone, None);
    }

    #[test]
    fn a_drag_past_the_grid_stops_at_its_edge() {
        let mut s = session();
        let (x0, y0) = cell(&s, 0, 40);
        assert!(s.roll_shift_press(SIZE, x0, y0));
        assert!(s.mouse_move(SIZE, 5000, 5000, true));
        let zone = s.roll_zone().expect("a zone");
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let span = view.zone.unwrap();
        assert_eq!((span.start_step, span.end_step, span.high_row), (0, 16, 40), "the page's last step");
        assert_eq!(span.low_row, view.top_row - layout.visible_rows() + 1, "the lowest row shown");
        assert_eq!(zone.end_units, 4 * ordrumbox_logic::block::BEAT_UNITS);
    }
}
