//! A zone of the pattern grid, drawn with Shift+drag from a step: its rows
//! and its time, on the page shown. Ctrl+C copies its notes, Ctrl+X cuts
//! them, Delete clears them; Ctrl+V pastes them row by row from the cursor,
//! replacing what was under them. Each edit is one undo step.

use crate::scale::Scale;
use ordrumbox_core::num;
use ordrumbox_logic::block::{Block, StepAt, Zone};

use super::session_song::DRAG_START_PX;
use super::*;

/// A Shift+press on a step that may become a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ZoneDrag {
    /// The step pressed: a corner of the zone.
    anchor: StepAt,
    press: (i32, i32),
    /// The pointer has really moved: this is no longer a Shift+click.
    started: bool,
    /// What it was pressed on; another edit cancels the drag.
    revision: u64,
}

impl Session {
    /// A Shift+press on step `step` of track `t`, at `(x, y)`.
    pub fn press_zone_drag(&mut self, t: usize, beat: i32, step: i32, x: i32, y: i32) {
        self.selection.zone = None;
        let anchor = StepAt { track: t, beat, step };
        self.chrome.zone_drag =
            Some(ZoneDrag { anchor, press: (x, y), started: false, revision: self.editor.revision() });
    }

    /// The mouse moved with a step Shift+pressed: the zone from it to the
    /// step under the mouse (the nearest of the page); whether to redraw.
    pub(super) fn zone_drag_move(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(drag) = &mut self.chrome.zone_drag else { return false };
        if drag.revision != self.editor.revision() {
            self.chrome.zone_drag = None;
            return false;
        }
        let (dx, dy) = (x - drag.press.0, y - drag.press.1);
        drag.started |= dx.abs() >= DRAG_START_PX || dy.abs() >= DRAG_START_PX;
        let drag = *drag;
        if !drag.started {
            return false;
        }
        let Some(pattern) = self.pattern() else { return false };
        let Some((track, beat, step)) = self.grid_layout(size).and_then(|l| l.nearest_cell(pattern, x, y)) else {
            return false;
        };
        let zone = Zone::between(pattern, drag.anchor, StepAt { track, beat, step });
        let changed = zone != self.selection.zone;
        self.selection.zone = zone;
        // The zone, not a note, is what the edits take.
        self.selection.selected = None;
        changed
    }

    /// Ctrl+C on a zone: its notes.
    pub(super) fn copy_zone(&mut self, zone: &Zone) {
        let Some(pattern) = self.pattern() else { return };
        let block = Block::copy(pattern, zone);
        self.message = format!("copied {} — {}", notes_label(block.note_count()), zone_label(zone));
        self.selection.clipboard = Some(Clipboard::Block(block));
    }

    /// Ctrl+X: the zone's notes copied and cleared, else the cursor step's.
    pub fn cut(&mut self) {
        if let Some(zone) = self.selection.zone {
            self.copy_zone(&zone);
            self.undoable(|s| s.delete_zone_now(&zone));
        } else if self.selection.cursor.is_some() {
            self.copy(false);
            self.delete_step();
        }
    }

    /// Delete on a zone: its notes cleared.
    pub(super) fn delete_zone(&mut self, zone: &Zone) {
        self.undoable(|s| s.delete_zone_now(zone));
    }

    fn delete_zone_now(&mut self, zone: &Zone) {
        let removed = self.editor.delete_zone(self.pattern_index, zone);
        if removed > 0 {
            self.edited();
        }
        self.selection.selected = None;
        self.message = format!("Deleted {} — {}", notes_label(removed), zone_label(zone));
    }

    /// Ctrl+V with a zone copied: its rows from the cursor's track down, its
    /// start at the cursor step's. The zone then shows where they went.
    pub(super) fn paste_block(&mut self, block: &Block) {
        let c = self.selection.cursor.unwrap_or(Cursor { track: 0, beat: 0, step: 0 });
        let at = StepAt { track: c.track, beat: c.beat, step: c.step };
        let pasted = self.editor.paste_block(self.pattern_index, at, block);
        if pasted.placed > 0 || pasted.zone.is_some() {
            self.edited();
        }
        self.selection.put_cursor(c);
        self.selection.zone = pasted.zone;
        self.message = match pasted.dropped {
            0 => format!("pasted {}", notes_label(pasted.placed)),
            dropped => format!("pasted {}, {dropped} past the pattern left out", notes_label(pasted.placed)),
        };
    }
}

/// `tracks 1-3, beats 2-4.2`: the rows and the time of a zone.
fn zone_label(zone: &Zone) -> String {
    use ordrumbox_logic::block::BEAT_UNITS;
    let tracks = if zone.first_track == zone.last_track {
        format!("track {}", zone.first_track + 1)
    } else {
        format!("tracks {}-{}", zone.first_track + 1, zone.last_track + 1)
    };
    let beats = |units: i64| units as f64 / BEAT_UNITS as f64 + 1.0;
    format!("{tracks}, beats {}-{}", num::round2(beats(zone.start_units)), num::round2(beats(zone.end_units)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;

    const SIZE: (i32, i32, Scale) = (1100, 680, Scale::ONE);

    /// KICK on every beat, SNARE on beats 2 and 4, HAT empty.
    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[
                {"name":"KICK","notes":[{"beat":0},{"beat":1},{"beat":2},{"beat":3}]},
                {"name":"SNARE","notes":[{"beat":1},{"beat":3}]},
                {"name":"HAT"}]}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session
    }

    /// The centre of a step's cell.
    fn centre(s: &Session, t: usize, beat: i32, step: i32) -> (i32, i32) {
        let pattern = s.pattern().unwrap();
        let r = s.grid_layout(SIZE).unwrap().cell_rect(t, beat, step, &pattern.tracks[t]);
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// Shift+drag from one step to another.
    fn draw_zone(s: &mut Session, from: (usize, i32, i32), to: (usize, i32, i32)) {
        let (x0, y0) = centre(s, from.0, from.1, from.2);
        let (x1, y1) = centre(s, to.0, to.1, to.2);
        s.select_step_note(from.0, from.1, from.2);
        s.press_zone_drag(from.0, from.1, from.2, x0, y0);
        s.mouse_move(SIZE, x1, y1, true);
        s.mouse_up();
    }

    fn beats(s: &Session, t: usize) -> Vec<i32> {
        let mut beats: Vec<i32> = s.pattern().unwrap().tracks[t].notes.iter().map(|n| n.beat).collect();
        beats.sort_unstable();
        beats
    }

    #[test]
    fn a_shift_click_without_moving_draws_no_zone() {
        let mut s = session();
        let (x, y) = centre(&s, 0, 1, 0);
        s.select_step_note(0, 1, 0);
        s.press_zone_drag(0, 1, 0, x, y);
        assert!(!s.mouse_move(SIZE, x + 1, y + 1, true), "a small move is still a click");
        s.mouse_up();
        assert_eq!(s.selection.zone, None);
        assert!(s.selection.selected.is_some(), "the note stays selected");
    }

    #[test]
    fn copy_and_paste_a_zone_row_by_row() {
        let mut s = session();
        // KICK and SNARE, beats 1 and 2.
        draw_zone(&mut s, (0, 0, 0), (1, 1, 3));
        let zone = s.selection.zone.expect("a zone");
        assert_eq!((zone.first_track, zone.last_track), (0, 1));
        s.run(Command::CopyStep);
        assert!(matches!(&s.selection.clipboard, Some(Clipboard::Block(b)) if b.note_count() == 3));
        // Pasted from SNARE's beat 3: the KICK row on SNARE, the SNARE row on HAT.
        s.click(Hit::Name(1));
        s.selection.put_cursor(Cursor { track: 1, beat: 2, step: 0 });
        s.run(Command::Paste);
        assert_eq!(beats(&s, 0), [0, 1, 2, 3], "KICK unchanged");
        assert_eq!(beats(&s, 1), [1, 2, 3], "beat 4 replaced by beat 2's note");
        assert_eq!(beats(&s, 2), [3]);
        let pasted = s.selection.zone.expect("the zone shows the pasted notes");
        assert_eq!((pasted.first_track, pasted.last_track), (1, 2));
        s.run(Command::Undo);
        assert_eq!(beats(&s, 1), [1, 3], "one undo step");
        assert_eq!(beats(&s, 2), Vec::<i32>::new());
    }

    #[test]
    fn cut_and_delete_clear_the_zone_in_one_undo_step() {
        let mut s = session();
        draw_zone(&mut s, (1, 3, 3), (0, 1, 0));
        s.run(Command::Cut);
        assert_eq!(beats(&s, 0), [0]);
        assert_eq!(beats(&s, 1), Vec::<i32>::new());
        assert!(matches!(&s.selection.clipboard, Some(Clipboard::Block(b)) if b.note_count() == 5));
        s.run(Command::Undo);
        assert_eq!(beats(&s, 0), [0, 1, 2, 3]);
        assert_eq!(beats(&s, 1), [1, 3]);
        draw_zone(&mut s, (0, 2, 0), (0, 3, 0));
        s.delete_step();
        assert_eq!(beats(&s, 0), [0, 1]);
        s.run(Command::Undo);
        assert_eq!(beats(&s, 0), [0, 1, 2, 3]);
    }

    #[test]
    fn a_click_or_escape_leaves_the_zone() {
        let mut s = session();
        draw_zone(&mut s, (0, 0, 0), (1, 1, 0));
        s.deselect();
        assert_eq!(s.selection.zone, None);
        draw_zone(&mut s, (0, 0, 0), (1, 1, 0));
        s.click(Hit::Cell { track: 2, beat: 0, step: 0 });
        assert_eq!(s.selection.zone, None);
    }

    #[test]
    fn a_drag_past_the_grid_stops_at_its_edge() {
        let mut s = session();
        let (x0, y0) = centre(&s, 0, 0, 0);
        s.press_zone_drag(0, 0, 0, x0, y0);
        assert!(s.mouse_move(SIZE, 5000, 5000, true));
        let zone = s.selection.zone.expect("a zone");
        assert_eq!(zone.last_track, 2, "the last row");
        let pattern = s.pattern().unwrap();
        let (t, beat, step) = s.grid_layout(SIZE).unwrap().nearest_cell(pattern, 5000, 5000).unwrap();
        assert_eq!((t, step), (2, 3), "the last step of the last row");
        let start = StepAt { track: 0, beat: 0, step: 0 };
        assert_eq!(Some(zone), Zone::between(pattern, start, StepAt { track: t, beat, step }));
        assert_eq!(beat + 1, s.grid_layout(SIZE).unwrap().start_beat() + 4, "the page's last beat");
    }
}
