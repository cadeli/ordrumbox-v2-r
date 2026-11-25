//! The piano roll's scroll bar under its grid: a drag of the thumb scrolls
//! the beats shown, a drag of a handle at an end of the thumb zooms (the other
//! end stays), as on the grid's (`session_grid_bar.rs`).

use super::Session;
use crate::piano_roll::{RollLayout, RollView};
use crate::scale::Scale;
use crate::scroll_thumb::Edge;
use crate::view::BarHold;

/// What the pointer at `x` puts on the bar: the first beat that centres the
/// thumb on it, the beat of the track under it, and the pattern's beats.
type BarAt = (i32, f64, i32);

/// The bar's values at `x`.
pub(super) fn bar_at(layout: &RollLayout, view: &RollView, x: i32) -> BarAt {
    (layout.first_beat_at(view, x), layout.track_beat(view, x).max(0.0), view.beat_count)
}

/// The hold a press on the bar takes: of a handle (`Some`) or the rest.
pub(super) fn bar_hold(layout: &RollLayout, view: &RollView, edge: Option<Edge>) -> BarHold {
    let first = view.page_steps().0 / i64::from(crate::piano_roll::steps_per_beat(view.track));
    edge.map_or(BarHold::Scroll, |edge| BarHold::zoom(edge, first as i32, layout.shown(view) as i32))
}

impl Session {
    /// Bring the playhead in sight of the roll when it moves on to another
    /// page of the beats shown.
    pub(super) fn roll_follow_playhead(&mut self) {
        let Some(spb) = self.roll_track().and_then(|t| self.track(t)).map(crate::piano_roll::steps_per_beat) else {
            return;
        };
        let beat = self.roll_playhead().map(|step| (step / i64::from(spb)) as i32);
        let shown = self.roll_shown_beats();
        self.roll.window.follow(shown, beat, None);
    }

    /// The bar held, moved to `x`; whether to redraw.
    pub(super) fn roll_bar_to(&mut self, size: (i32, i32, Scale), x: i32) -> bool {
        let layout = self.roll_layout(size);
        let Some(view) = self.roll_view(&layout) else { return false };
        let at = bar_at(&layout, &view, x);
        self.roll_bar_held(at)
    }

    /// The bar held at `at`; whether it moved the window.
    pub(super) fn roll_bar_held(&mut self, (centred, at, total): BarAt) -> bool {
        let Some(hold) = self.roll.bar_drag else { return false };
        let before = self.roll.window;
        self.roll.window.hold(hold, centred, at, total);
        before != self.roll.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::piano_roll::RollSpot;
    use crate::test_support::SIZE;
    use ordrumbox_core::Library;

    /// A pattern of 32 beats, a track of notes on its first beats, in the piano roll.
    fn long_roll() -> Session {
        let json =
            r#"{"patterns":[{"name":"p","beatCount":32,"tracks":[{"name":"KICK","notes":[{"beat":0},{"beat":9}]}]}]}"#;
        let mut s = Session::new(Library::parse(json).unwrap().0, KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        s.toggle_piano_roll();
        s
    }

    fn state(s: &Session) -> (i32, i32) {
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).expect("a roll");
        (i32::try_from(view.page_steps().0 / 4).unwrap(), layout.shown(&view) as i32)
    }

    #[test]
    fn the_roll_opens_on_four_beats_with_the_bar_under_the_grid() {
        let s = long_roll();
        let layout = s.roll_layout(SIZE);
        assert_eq!(state(&s), (0, 4));
        assert!(layout.bar.y >= layout.grid.y + layout.grid.h, "under the rows");
        let view = s.roll_view(&layout).unwrap();
        let (start, len) = layout.bar_thumb(&view);
        assert_eq!(layout.spot_at(&view, start + len / 2, layout.bar.y + 1), Some(RollSpot::ScrollBar(None)));
    }

    #[test]
    fn the_thumb_scrolls_and_a_handle_zooms() {
        let mut s = long_roll();
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let (start, len) = layout.bar_thumb(&view);
        let y = layout.bar.y + 1;
        assert!(s.roll_click(SIZE, start + len / 2, y, crate::ui_input::Button::Left));
        s.mouse_move(SIZE, layout.bar.x + layout.bar.w / 2, y, false);
        let (first, shown) = state(&s);
        assert!((10..=20).contains(&first) && shown == 4, "the middle of 32 beats: {first}");
        s.mouse_up();
        assert_eq!(s.roll.bar_drag, None);
        // The end handle, dragged to twice as far: twice the beats, from the same first one.
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let (start, len) = layout.bar_thumb(&view);
        assert!(s.roll_click(SIZE, start + len - 1, y, crate::ui_input::Button::Left));
        s.mouse_move(SIZE, start + 2 * len - 1, y, false);
        assert_eq!(state(&s), (first, 8));
        s.mouse_up();
        // Eight beats across the same width: narrower cells.
        assert!(s.roll_layout(SIZE).cell < layout.cell);
    }

    #[test]
    fn the_wheel_zooms_with_ctrl_and_scrolls_with_shift() {
        let mut s = long_roll();
        let grid = s.roll_layout(SIZE).grid;
        let at = (grid.x + grid.w / 2, grid.y + 5);
        assert!(s.roll_wheel(SIZE, at, -2, (true, false)), "out");
        assert!(state(&s).1 > 4);
        assert!(s.roll_wheel(SIZE, at, 40, (true, false)), "in");
        assert_eq!(state(&s).1, 1);
        let first = state(&s).0;
        assert!(s.roll_wheel(SIZE, at, -3, (false, true)));
        assert_eq!(state(&s).0, first + 3);
    }

    #[test]
    fn the_cursor_keys_bring_the_cursor_in_sight() {
        let mut s = long_roll();
        s.roll.cursor = Some((0, 40));
        s.roll_arrow(crate::session::Move::Left, false);
        assert_eq!(s.roll.cursor.map(|c| c.0), Some(127), "wrapped to the last step");
        assert_eq!(state(&s), (28, 4));
    }
}
