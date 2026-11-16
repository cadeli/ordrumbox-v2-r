//! The kit editor's table of samples: which rows it shows, its scroll bar
//! (held by the mouse) and the wheel over it.

use super::Session;
use crate::scale::Scale;
use crate::view::KitLayout;

/// Where the kit table is scrolled to, and whether its bar is held.
#[derive(Debug, Default)]
pub(super) struct KitList {
    /// The sample selected when the table was scrolled, and its first row
    /// shown then.
    scroll: (usize, usize),
    /// The scroll bar is held by the mouse.
    pub(super) bar_held: bool,
}

impl Session {
    pub(super) fn kit_layout(&self, size: (i32, i32, Scale)) -> KitLayout {
        let body = self.body(size);
        KitLayout::new(body.main_w, body.editor_h, size.2, self.status.len())
    }

    /// The table's first row shown: where the wheel or the bar left it, moved
    /// just enough to show a sample selected since.
    pub fn kit_first(&self, layout: &KitLayout) -> usize {
        let (at, mut first) = self.kit_editor.list.scroll;
        let selected = self.kit_editor.sample.unwrap_or(0);
        if at != selected {
            first = first.clamp(selected.saturating_sub(layout.shown().saturating_sub(1)), selected);
        }
        first.min(layout.last_first(self.kit().map_or(0, |k| k.samples.len())))
    }

    /// A press on the table's scroll bar, when there is one: the thumb goes
    /// to `y`, and follows the mouse until it is released.
    pub fn kit_bar_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let layout = self.kit_layout(size);
        let count = self.kit().map_or(0, |k| k.samples.len());
        if !self.kit_editor.is_open() || count <= layout.shown() || !layout.scroll_bar().contains(x, y) {
            return false;
        }
        self.kit_editor.list.bar_held = true;
        self.kit_bar_to(size, y);
        true
    }

    /// The held bar moved to `y`; whether to redraw.
    pub(super) fn kit_bar_to(&mut self, size: (i32, i32, Scale), y: i32) -> bool {
        let layout = self.kit_layout(size);
        let count = self.kit().map_or(0, |k| k.samples.len());
        let first = layout.first_at(y, count);
        let changed = first != self.kit_first(&layout);
        self.kit_editor.list.scroll = (self.kit_editor.sample.unwrap_or(0), first);
        changed
    }

    /// The wheel over the table, `lines` notches (positive show what is
    /// before); whether it was the table's.
    pub fn kit_wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, lines: isize) -> bool {
        if !self.kit_editor.is_open() || lines == 0 {
            return false;
        }
        let layout = self.kit_layout(size);
        let count = self.kit().map_or(0, |k| k.samples.len());
        if !layout.table_contains(x, y) || count <= layout.shown() {
            return false;
        }
        let first = (self.kit_first(&layout) as isize - lines).clamp(0, layout.last_first(count) as isize) as usize;
        self.kit_editor.list.scroll = (self.kit_editor.sample.unwrap_or(0), first);
        true
    }
}
