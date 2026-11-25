//! The grid's scroll bar, under its rows: a drag of the thumb scrolls the
//! beats shown, a drag of a handle at an end of the thumb zooms (the other
//! end stays), Ctrl+wheel zooms around the pointer and Shift+wheel scrolls.

use super::Session;
use super::session_chrome::GridHold;
use super::session_types::WheelMode;
use crate::scale::Scale;
use crate::view::BarHold;

/// The beats a notch of Shift+wheel scrolls.
const WHEEL_BEATS: isize = 1;

impl Session {
    /// A press at `(x, y)` on the grid's scroll bar takes hold of it; whether it was on it.
    pub fn grid_bar_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(layout) = self.grid_layout(size) else { return false };
        let Some(spot) = layout.bar_at(x, y) else { return false };
        self.chrome.grid_hold = Some(GridHold::Bar(match spot {
            None => BarHold::Scroll,
            Some(edge) => BarHold::zoom(edge, layout.start_beat(), layout.beats()),
        }));
        self.grid_bar_held(size, x);
        true
    }

    /// The bar held, moved to `x`; whether to redraw.
    pub(super) fn grid_bar_held(&mut self, size: (i32, i32, Scale), x: i32) -> bool {
        let (Some(hold), Some(layout)) = (self.held_grid_bar(), self.grid_layout(size)) else { return false };
        let before = self.chrome.beat_window;
        let at = layout.track_beat(x).max(0.0);
        self.chrome.beat_window.hold(hold, layout.first_beat_at(x), at, layout.total_beats());
        before != self.chrome.beat_window
    }

    /// The bar the mouse holds, if any.
    pub(super) fn held_grid_bar(&self) -> Option<BarHold> {
        match self.chrome.grid_hold {
            Some(GridHold::Bar(hold)) => Some(hold),
            _ => None,
        }
    }

    /// The wheel over the grid's steps: Ctrl zooms around the beat under the
    /// pointer, Shift (or a sideways wheel) scrolls; whether it was the grid's.
    pub fn grid_wheel(
        &mut self,
        size: (i32, i32, Scale),
        (x, y): (i32, i32),
        (columns, lines): (isize, isize),
        mode: WheelMode,
    ) -> bool {
        let Some(layout) = self.grid_layout(size) else { return false };
        if !layout.beats_area().contains(x, y) {
            return false;
        }
        let (shown, total) = (layout.beats(), layout.total_beats());
        if mode == WheelMode::Zoom && lines != 0 {
            let window = &mut self.chrome.beat_window;
            window.zoom_at(lines, (shown, total), (layout.beat_under(x), x), layout.bar_track());
            return true;
        }
        let notches = if columns != 0 {
            columns
        } else if mode == WheelMode::Scroll {
            lines
        } else {
            0
        };
        if notches == 0 {
            return false;
        }
        self.chrome.beat_window.scroll_by(notches * WHEEL_BEATS, shown, total);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::scroll_thumb::Edge;
    use crate::test_support::SIZE;
    use ordrumbox_core::Library;

    /// A pattern of 64 beats with a kick on each, in the grid.
    fn long_pattern() -> Session {
        let notes: Vec<String> = (0..64).map(|beat| format!(r#"{{"beat":{beat}}}"#)).collect();
        let json = format!(
            r#"{{"patterns":[{{"name":"p","beatCount":64,"tracks":[{{"name":"KICK","notes":[{}]}}]}}]}}"#,
            notes.join(",")
        );
        let mut s = Session::new(Library::parse(&json).unwrap().0, KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        s
    }

    fn layout(s: &Session) -> crate::view::Layout {
        s.grid_layout(SIZE).expect("the grid")
    }

    #[test]
    fn the_thumb_dragged_scrolls_the_beats() {
        let mut s = long_pattern();
        let bar = layout(&s).scroll_bar();
        let (start, len) = layout(&s).bar_thumb();
        assert_eq!(start, bar.x, "the thumb starts at the first beat");
        let middle = bar.x + bar.w / 2;
        assert!(s.grid_bar_press(SIZE, start + len / 2, bar.y + 1));
        assert!(s.mouse_move(SIZE, middle, bar.y + 1, false));
        let first = layout(&s).start_beat();
        assert!((20..=40).contains(&first), "about the middle of 64 beats: {first}");
        s.mouse_up();
        assert_eq!(s.held_grid_bar(), None);
        assert_eq!(layout(&s).bar_at(0, 0), None);
    }

    #[test]
    fn a_handle_dragged_zooms_and_the_other_end_stays() {
        let mut s = long_pattern();
        let shown = layout(&s).beats();
        let (start, len) = layout(&s).bar_thumb();
        let y = layout(&s).scroll_bar().y + 1;
        assert!(s.grid_bar_press(SIZE, start + len - 1, y));
        assert!(matches!(s.held_grid_bar(), Some(BarHold::Zoom { edge: Edge::End, .. })));
        // Twice as far along: twice the beats, from the same first one.
        assert!(s.mouse_move(SIZE, start + 2 * len - 1, y, false));
        assert_eq!((layout(&s).start_beat(), layout(&s).beats()), (0, 2 * shown));
        s.mouse_up();
        // The start's handle, dragged right, shows fewer beats and keeps the last one.
        let (start, len) = layout(&s).bar_thumb();
        assert!(s.grid_bar_press(SIZE, start, y));
        let last = layout(&s).start_beat() + layout(&s).beats();
        assert!(s.mouse_move(SIZE, start + len / 2, y, false));
        assert_eq!(layout(&s).start_beat() + layout(&s).beats(), last);
        assert!(layout(&s).beats() < 2 * shown);
    }

    #[test]
    fn ctrl_wheel_zooms_around_the_beat_under_the_pointer_and_shift_wheel_scrolls() {
        let mut s = long_pattern();
        s.chrome.beat_window.scroll_to(20);
        let area = layout(&s).beats_area();
        let (x, y) = (area.x + area.w / 2, area.y + 2);
        let shown = layout(&s).beats();
        let under = layout(&s).beat_under(x);
        assert!(s.grid_wheel(SIZE, (x, y), (0, -2), WheelMode::Zoom), "two notches out");
        assert!(layout(&s).beats() > shown);
        assert!((layout(&s).beat_under(x) - under).abs() <= 1.0, "the beat under the pointer stays");
        assert!(s.grid_wheel(SIZE, (x, y), (0, 40), WheelMode::Zoom), "in");
        assert_eq!(layout(&s).beats(), 1, "no fewer than a beat");
        // One beat to the right, then back to the start (the wheel stops there).
        assert!(s.grid_wheel(SIZE, (x, y), (0, -1), WheelMode::Scroll));
        let first = layout(&s).start_beat();
        assert!(s.grid_wheel(SIZE, (x, y), (0, 1), WheelMode::Scroll));
        assert_eq!(layout(&s).start_beat(), first - 1);
        assert!(!s.grid_wheel(SIZE, (x, y), (0, 1), WheelMode::Plain), "a plain wheel is not the grid's");
        assert!(!s.grid_wheel(SIZE, (area.x - 5, y), (0, 1), WheelMode::Scroll), "nor beside the steps");
    }

    #[test]
    fn the_zoom_stays_when_the_playhead_or_a_new_pattern_rewinds() {
        let mut s = long_pattern();
        s.chrome.beat_window.zoom_to(6);
        s.chrome.beat_window.scroll_to(30);
        s.chrome.beat_window.rewind();
        assert_eq!((layout(&s).start_beat(), layout(&s).beats()), (0, 6));
    }
}
