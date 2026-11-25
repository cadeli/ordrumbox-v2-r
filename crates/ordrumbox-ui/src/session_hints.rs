//! The help line at the bottom of the window, as the pointer moves: what a
//! click, a drag and a right-click do on what is under it (a clip, a step,
//! a lane, a knob…); elsewhere, the keys of the view open.

use super::Session;
use crate::panel::{self, Kind};
use crate::piano_roll::RollHit;
use crate::scale::Scale;
use crate::song_view::SongHit;
use crate::view::Hit;

/// What the mouse does on a beat scroll bar (the grid's, the piano roll's): its handles, the rest.
const ZOOM_HINT: &str = "ZOOM   drag: show more or fewer beats   Ctrl+wheel: zoom around the pointer";
const BEATS_HINT: &str = "BEATS   drag: scroll   Shift+wheel: scroll   drag an end of the thumb: zoom";

/// What the mouse does on `hit`, in the song panel.
const fn song_hint(hit: SongHit) -> &'static str {
    match hit {
        SongHit::Pattern(_) => {
            "PATTERN   click: show it   drag: move it in the list, or onto the song   right-click: add, move, rename, delete"
        }
        SongHit::Description => "DESCRIPTION   click: write it   wheel: scroll",
        SongHit::Tags => "TAGS   click: edit them, the style first, then features (bass, chords...)",
        SongHit::Ruler(_) => {
            "RULER   click: cursor   drag: the song's loop   Shift+click: loop from the cursor   right-click: loop menu"
        }
        SongHit::LoopEnd(_) => "LOOP   drag: move this end   right-click: turn the loop on or off, remove it",
        SongHit::Clip(_) => {
            "CLIP   click: select (DELETE: remove, Ctrl+D: repeat)   double-click: open its pattern   drag: move   right-click: menu"
        }
        SongHit::RowName(_) => "ROW   click: show its pattern   drag: move the row   right-click: add, move, delete",
        SongHit::Cell { .. } => "SONG   click: add the row's pattern here   right-click: menu",
        SongHit::ScrollBar(_) => "drag: scroll   click beside the thumb: a page",
        SongHit::ZoomHandle(_) => "ZOOM   drag: show more or fewer measures   Ctrl+wheel: zoom around the pointer",
        SongHit::AddAutomation => "click: add an automation lane (an instrument, then its control)",
        SongHit::LaneName(_) => "LANE   click or right-click: re-enable, taller, shorter, clear, delete",
        SongHit::LaneBadge(_) => "OFF   click: play this lane again (a control took it back)",
        SongHit::LaneEdge(_) => "LANE   drag: one to four rows tall   right-click: lane menu",
        SongHit::LaneStep { .. } => {
            "LANE   drag: draw (the height is the level)   right-drag: erase   Shift: single steps   drag a line: move it"
        }
    }
}

/// What the mouse does on `hit`, in the grid.
const fn grid_hint(hit: Hit) -> &'static str {
    match hit {
        Hit::Mute(_) => "M   click: mute the track   right-click: track menu",
        Hit::Solo(_) => "S   click: solo the track   right-click: track menu",
        Hit::Name(_) => {
            "TRACK   click: edit it   double-click: piano roll   drag: move the row   right-click: rename, copy, delete..."
        }
        Hit::Cell { .. } => {
            "STEP   click: on / off   Shift+click: select its note   Shift+drag: a zone   right-click: step menu"
        }
        Hit::AddTrack => "click: add a track (a category, then its instrument)",
    }
}

/// What the mouse does on a panel control of `kind`, if it says more than the view's keys.
const fn control_hint(kind: &Kind) -> Option<&'static str> {
    match kind {
        Kind::Value { .. } => Some(
            "drag or wheel: change (Shift: finely)   double-click: default   right-click: type a value, copy, paste",
        ),
        Kind::Select { .. } | Kind::Screen { .. } => Some("click: open the list   wheel or arrows: the next one"),
        _ => None,
    }
}

impl Session {
    /// What the help line says for the pointer at `(x, y)`; `None`: the view's keys.
    fn hint_at(&self, size: (i32, i32, Scale), x: i32, y: i32) -> Option<&'static str> {
        if self.overlay_open() || self.chrome.widgets.dropdown.is_some() || self.chrome.menu.is_some() {
            return None;
        }
        let layout = self.panel_layout(size.0, size.1, size.2);
        if let Some(item) = panel::hit(&layout.items, x, y) {
            return control_hint(&item.kind);
        }
        if self.song_view_open() {
            let layout = self.song_layout(size);
            let rows = self.song_rows();
            return layout.hit(&self.song_view_data(&rows, &layout), x, y).map(song_hint);
        }
        if self.piano_roll_open() {
            let layout = self.roll_layout(size);
            return match layout.hit(&self.roll_view(&layout)?, x, y)? {
                RollHit::ScrollBar(Some(_)) => Some(ZOOM_HINT),
                RollHit::ScrollBar(None) => Some(BEATS_HINT),
                _ => None,
            };
        }
        if let Some(layout) = self.grid_layout(size).filter(|layout| layout.bar_hit(x, y).is_some()) {
            return Some(if matches!(layout.bar_hit(x, y), Some(Some(_))) { ZOOM_HINT } else { BEATS_HINT });
        }
        self.grid_hit(size, x, y).map(grid_hint)
    }

    /// The pointer at `(x, y)`: the help line follows it; whether it changed.
    pub fn hint_hover(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let hint = self.hint_at(size, x, y);
        let changed = hint != self.chrome.hint;
        self.chrome.hint = hint;
        changed
    }

    /// What the help line says now; `None`: the view's keys.
    pub(super) fn hint(&self) -> Option<&'static str> {
        self.chrome.hint
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, find_hit, session};
    use crate::song_view::SongHit;

    #[test]
    fn the_help_line_follows_the_pointer() {
        let mut s = session();
        let (x, y) = find_hit(&s, |hit| matches!(hit, SongHit::Clip(_)));
        assert!(s.hint_hover(SIZE, x, y));
        assert!(s.hint().is_some_and(|hint| hint.starts_with("CLIP")), "{:?}", s.hint());
        assert!(!s.hint_hover(SIZE, x, y), "the same: nothing to redraw");
        let (x, y) = find_hit(&s, |hit| matches!(hit, SongHit::Ruler(_)));
        s.hint_hover(SIZE, x, y);
        assert!(s.hint().is_some_and(|hint| hint.starts_with("RULER")));
        // Off everything: the view's keys again.
        assert!(s.hint_hover(SIZE, 0, SIZE.1 - 1));
        assert_eq!(s.hint(), None);
    }
}
