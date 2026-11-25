//! The pointer's shape over the session's views: what a drag where it is
//! does. Every scroll bar follows one rule (`scroll_thumb::hover_shape`):
//! the open hand on a thumb, the closed hand while one is held (the song
//! panel's, the kit table's, the mixer's, an open list's); the ends of
//! the song panel's measures' thumb, which zoom, show the sideways arrows,
//! an automation lane's bottom edge, which sets its height, the up-down ones;
//! in the piano roll, a note's right edge, which sets its length, the
//! sideways arrows, and its body, which a drag moves, the four arrows.

use super::Session;
use super::session_chrome::GridHold;
use super::session_song_zoom::BarDrag;
use crate::piano_roll::RollSpot;
use crate::scale::Scale;
use crate::scroll_thumb::{self, HELD_SHAPE};
use crate::song_view::{Axis, SongSpot};
use crate::ui_input::PointerShape;
use crate::view::BarHold;

impl Session {
    /// The pointer's shape at `(x, y)`.
    #[must_use]
    pub fn pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        match self.song_panel.held_bar() {
            Some(BarDrag::Zoom { .. }) => return PointerShape::ResizeHorizontal,
            Some(BarDrag::Scroll(_)) => return HELD_SHAPE,
            None => {}
        }
        match self.roll.bar_drag.or(self.held_grid_bar()) {
            Some(BarHold::Zoom { .. }) => return PointerShape::ResizeHorizontal,
            Some(BarHold::Scroll) => return HELD_SHAPE,
            None => {}
        }
        if self.song_panel.lane_resize.is_some() || matches!(self.chrome.grid_hold, Some(GridHold::LaneResize(_))) {
            return PointerShape::ResizeVertical;
        }
        if self.roll.length_drag.is_some() || self.is_wave_dragging() {
            return PointerShape::ResizeHorizontal;
        }
        if self.roll.drag.is_some() {
            return PointerShape::Move;
        }
        if self.kit_editor.list.is_bar_held() || self.is_mixer_bar_held() {
            return HELD_SHAPE;
        }
        if let Some(shape) = self.dropdown_pointer_shape(size, x, y) {
            return shape;
        }
        // A menu, a list or an overlay over the views: their bars are not under it.
        if self.popup_open() || self.open_menu().is_some() || self.overlay_open() {
            return PointerShape::Arrow;
        }
        if self.is_over_wave_handle(size, x, y) {
            return PointerShape::ResizeHorizontal;
        }
        if self.song_view_open() {
            return self.song_pointer_shape(size, x, y);
        }
        if self.piano_roll_open() {
            return self.roll_pointer_shape(size, x, y);
        }
        if self.kit_list().is_some() {
            return self.kit_pointer_shape(size, x, y);
        }
        if self.mixer_open() {
            return self.mixer_pointer_shape(size, x, y);
        }
        self.grid_pointer_shape(size, x, y)
    }

    /// Over the piano roll: its scroll bar's thumb and zoom handles, a
    /// note's right edge (its length) and its body (a drag moves it).
    fn roll_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        let layout = self.roll_layout(size);
        let Some(view) = self.roll_view(&layout) else { return PointerShape::Arrow };
        match layout.spot_at(&view, x, y) {
            Some(RollSpot::ScrollBar(Some(_))) => PointerShape::ResizeHorizontal,
            Some(RollSpot::ScrollBar(None)) => scroll_thumb::hover_shape(layout.bar_thumb(&view), x),
            Some(RollSpot::Cell { .. }) => match layout.note_at(&view, x, y) {
                Some((_, true)) => PointerShape::ResizeHorizontal,
                Some((_, false)) => PointerShape::Move,
                None => PointerShape::Arrow,
            },
            _ => PointerShape::Arrow,
        }
    }

    /// Over the grid: its scroll bar's thumb and zoom handles.
    fn grid_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        let Some(layout) = self.grid_layout(size) else { return PointerShape::Arrow };
        if layout.lane_edge_rect().contains(x, y) {
            return PointerShape::ResizeVertical;
        }
        match layout.bar_at(x, y) {
            Some(Some(_)) => PointerShape::ResizeHorizontal,
            Some(None) => scroll_thumb::hover_shape(layout.bar_thumb(), x),
            None => PointerShape::Arrow,
        }
    }

    /// Over the mixer: its track strips' scroll bar, when it has one.
    fn mixer_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        let mixer = self.mixer_layout(size);
        if !mixer.scrolls() || !mixer.scroll_bar.contains(x, y) {
            return PointerShape::Arrow;
        }
        scroll_thumb::hover_shape(mixer.thumb(), x)
    }

    /// Over the song panel: its bars and the zoom's handles.
    fn song_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        let layout = self.song_layout(size);
        let rows = self.song_rows();
        let view = self.song_view_data(&rows, &layout);
        match layout.spot_at(&view, x, y) {
            Some(SongSpot::ZoomHandle(_)) => PointerShape::ResizeHorizontal,
            Some(SongSpot::LaneEdge(_)) => PointerShape::ResizeVertical,
            Some(SongSpot::ScrollBar(axis)) => {
                scroll_thumb::hover_shape(layout.bar_thumb(&view, axis), Axis::along(axis, (x, y)))
            }
            _ => PointerShape::Arrow,
        }
    }

    /// Over an open list's scroll bar, or while its thumb is held.
    fn dropdown_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> Option<PointerShape> {
        let dropdown = self.chrome.widgets.dropdown()?;
        if dropdown.bar_held {
            return Some(HELD_SHAPE);
        }
        let (width, height, scale) = size;
        let bar = dropdown.scroll_bar(width, height, scale).filter(|bar| bar.contains(x, y))?;
        Some(scroll_thumb::hover_shape(dropdown.thumb(bar, scale), y))
    }

    /// Over the kit editor: its table's bar, when it has one.
    fn kit_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        let layout = self.kit_layout(size);
        let count = self.kit_sample_count();
        if count <= layout.shown() || !layout.scroll_bar().contains(x, y) {
            return PointerShape::Arrow;
        }
        scroll_thumb::hover_shape(layout.thumb(self.kit_first(&layout), count), y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::session::session_song::tests::SIZE;
    use crate::ui_input::Button;
    use ordrumbox_core::Library;

    #[test]
    fn the_pointer_shows_the_hand_on_a_thumb_and_the_arrows_on_the_zoom_handles() {
        let clips: Vec<String> =
            (0..8).map(|i| format!(r#"{{"pattern":"p","startMeasure":{},"measureCount":8}}"#, i * 8)).collect();
        let json = format!(r#"{{"songs":[{{"name":"s","clips":[{}]}}],"patterns":[{{"name":"p"}}]}}"#, clips.join(","));
        let mut s = Session::new(Library::parse(&json).unwrap().0, KitSet::empty(), Vec::new());
        assert_eq!(s.pointer_shape(SIZE, 300, 300), PointerShape::Arrow, "the grid");
        s.toggle_song_view();
        let layout = s.song_layout(SIZE);
        let rows = s.song_rows();
        let (start, len) = layout.bar_thumb(&s.song_view_data(&rows, &layout), Axis::Measures);
        let y = layout.scroll_bar(Axis::Measures).y + 2;
        let shape = |s: &Session, x| s.pointer_shape(SIZE, x, y);
        assert_eq!(shape(&s, start + 1), PointerShape::ResizeHorizontal);
        assert_eq!(shape(&s, start + len - 1), PointerShape::ResizeHorizontal);
        assert_eq!(shape(&s, start + len / 2), PointerShape::Grab);
        assert_eq!(shape(&s, start + len + 20), PointerShape::Arrow, "the track: a click jumps");
        // Held: the middle grabs, an end keeps the arrows wherever the pointer goes.
        assert!(s.song_click(SIZE, start + len / 2, y, Button::Left));
        assert_eq!(s.pointer_shape(SIZE, 0, 0), PointerShape::Grabbing);
        s.mouse_up();
        assert!(s.song_click(SIZE, start + len - 1, y, Button::Left));
        assert_eq!(s.pointer_shape(SIZE, 0, 0), PointerShape::ResizeHorizontal);
        s.mouse_up();
        assert_eq!(s.pointer_shape(SIZE, 0, 0), PointerShape::Arrow);
    }

    #[test]
    fn a_lane_s_bottom_edge_shows_the_up_down_arrows() {
        let mut s = crate::session::session_song::tests::session();
        s.editor.add_track(1, "KICK", 4);
        s.editor.add_automation_lane(None, "KICK", ordrumbox_core::AutomationTarget::Volume).unwrap();
        s.set_size(SIZE);
        let at = |wanted: SongSpot| crate::session::session_song::tests::find_spot(&s, |spot| spot == wanted);
        let ((ex, ey), (nx, ny)) = (at(SongSpot::LaneEdge(0)), at(SongSpot::LaneName(0)));
        assert_eq!(s.pointer_shape(SIZE, ex, ey), PointerShape::ResizeVertical);
        assert_eq!(s.pointer_shape(SIZE, nx, ny), PointerShape::Arrow, "the name: its menu");
        // Held, the arrows stay wherever the pointer goes.
        assert!(s.song_click(SIZE, ex, ey, Button::Left));
        assert_eq!(s.pointer_shape(SIZE, 0, 0), PointerShape::ResizeVertical);
        assert!(s.song_release(SIZE));
        assert_eq!(s.pointer_shape(SIZE, 0, 0), PointerShape::Arrow);
    }
}
