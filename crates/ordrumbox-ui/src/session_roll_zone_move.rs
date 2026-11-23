//! A zone of the piano roll dragged by one of its notes: the notes of the
//! zone move together in time and in pitch, replacing what they land on, as
//! one undo step when the mouse is let go. The zone follows the pointer.

use crate::scale::Scale;
use ordrumbox_core::steps::absolute_step;
use ordrumbox_logic::block::step_units;
use ordrumbox_logic::roll_block::{RollZone, note_key};

use super::Session;
use super::session_labels::moved_message;
use super::session_song::DRAG_START_PX;
use crate::piano_roll::{TOTAL_KEYS, row_key, steps_per_beat};

/// A press on a note inside the roll's zone, which may drag the zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RollZoneMove {
    /// The zone when pressed.
    zone: RollZone,
    /// The cell pressed (step, row).
    anchor: (i64, i32),
    press: (i32, i32),
    /// The pointer has really moved.
    started: bool,
    /// The steps and keys it moved by.
    delta: (i64, i32),
    revision: u64,
}

impl Session {
    /// Whether note `n` of track `t` starts inside `zone`.
    pub(super) fn roll_zone_holds_note(&self, zone: &RollZone, n: usize) -> bool {
        let Some(track) = self.track(zone.track) else { return false };
        let Some(note) = track.notes.get(n) else { return false };
        let units = absolute_step(note.beat, note.step, steps_per_beat(track)) * step_units(track.steps_per_beat);
        zone.holds(units, note_key(note, track))
    }

    /// A press on note `n` of the roll's track at the cell `anchor`: when the
    /// note is inside the zone, a drag moves the zone's notes. Whether it
    /// started one.
    pub(super) fn roll_zone_move_press(&mut self, n: usize, anchor: (i64, i32), press: (i32, i32)) -> bool {
        let Some(zone) = self.roll_zone().filter(|zone| self.roll_zone_holds_note(zone, n)) else { return false };
        let revision = self.editor.revision();
        self.roll.zone_move = Some(RollZoneMove { zone, anchor, press, started: false, delta: (0, 0), revision });
        true
    }

    /// The mouse moved with the zone pressed: the zone shown that far from
    /// where it was; whether to redraw.
    pub(super) fn roll_zone_move_to(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(mut drag) = self.roll.zone_move else { return false };
        if drag.revision != self.editor.revision() {
            self.roll.zone_move = None;
            return false;
        }
        drag.started |= (x - drag.press.0).abs() >= DRAG_START_PX || (y - drag.press.1).abs() >= DRAG_START_PX;
        let layout = self.roll_layout(size);
        let cell = drag.started.then(|| self.roll_view(&layout).and_then(|v| layout.nearest_cell(&v, x, y))).flatten();
        let Some((step, row)) = cell else {
            self.roll.zone_move = Some(drag);
            return false;
        };
        drag.delta = (step - drag.anchor.0, row_key(row) - row_key(drag.anchor.1));
        let Some(step_length) = self.track(drag.zone.track).map(|t| step_units(t.steps_per_beat)) else { return false };
        let shift = drag.delta.0 * step_length;
        let shown = RollZone {
            start_units: drag.zone.start_units + shift,
            end_units: drag.zone.end_units + shift,
            low_key: drag.zone.low_key + drag.delta.1,
            high_key: drag.zone.high_key + drag.delta.1,
            ..drag.zone
        };
        let changed = self.selection.roll_zone != Some(shown);
        self.selection.roll_zone = Some(shown);
        self.roll.zone_move = Some(drag);
        changed
    }

    /// The press ended: the zone's notes move to where it is shown, or the
    /// zone is as it was when it did not move.
    pub(super) fn roll_zone_move_release(&mut self) {
        let Some(drag) = self.roll.zone_move.take() else { return };
        if !drag.started || drag.revision != self.editor.revision() {
            return;
        }
        if drag.delta == (0, 0) {
            self.selection.roll_zone = Some(drag.zone);
            return;
        }
        let keys = row_key(0)..=row_key(TOTAL_KEYS - 1);
        let moved = self.editor.move_roll_zone(self.pattern_index, &drag.zone, drag.delta, keys);
        if moved.placed > 0 || moved.zone.is_some() {
            self.edited();
        }
        self.selection.selected = None;
        self.selection.roll_zone = moved.zone;
        self.message = moved_message(moved.placed, moved.dropped, "past the pattern or the keyboard");
    }
}
