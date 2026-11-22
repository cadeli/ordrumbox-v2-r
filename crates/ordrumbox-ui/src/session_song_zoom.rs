//! The arrangement's zoom, as on a DAW timeline: an end of the measures'
//! thumb dragged (the other end stays), Ctrl+wheel over the arrangement
//! (the measure under the pointer stays), and what the scroll bars hold.

use super::Session;
use super::session_types::Workspace;
use crate::scale::Scale;
use crate::song_view::{Axis, Edge};

/// A notch of Ctrl+wheel: a measure this many times wider (or narrower).
const ZOOM_STEP: f64 = 1.25;

/// What the mouse holds on the arrangement's scroll bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BarDrag {
    /// A bar's thumb, centred on the pointer.
    Scroll(Axis),
    /// An end of the measures' thumb: the other end stays on measure
    /// `fixed`; the track spans `span` measures while it is held.
    Zoom { edge: Edge, fixed: u32, span: u32 },
}

impl Session {
    /// The bar held by the mouse, moved to `(x, y)`; whether to redraw.
    pub(super) fn song_bar_held(&mut self, size: (i32, i32, Scale), drag: BarDrag, x: i32, y: i32) -> bool {
        match drag {
            BarDrag::Scroll(axis) => self.song_bar_to(size, axis, x, y),
            BarDrag::Zoom { edge, fixed, span } => self.song_zoom_to(size, (edge, fixed, span), x),
        }
    }

    /// The end `edge` of the measures' thumb pressed.
    pub(super) fn press_zoom_handle(&mut self, size: (i32, i32, Scale), edge: Edge) {
        let layout = self.song_layout(size);
        let first = self.song_scroll(&layout, self.song_rows().len()).0;
        let shown = layout.visible_measures();
        let span = self.song_measure_total(&layout).max(shown);
        let fixed = match edge {
            Edge::Start => first + shown,
            Edge::End => first,
        };
        self.song_panel.bar_drag = Some(BarDrag::Zoom { edge, fixed, span });
    }

    /// The end of the measures' thumb held at `x`: the measures from the
    /// other end to `x` fill the arrangement; whether to redraw.
    fn song_zoom_to(&mut self, size: (i32, i32, Scale), (edge, fixed, span): (Edge, u32, u32), x: i32) -> bool {
        let layout = self.song_layout(size);
        let at = layout.track_measure(x, span).max(0.0);
        let measures = match edge {
            Edge::Start => f64::from(fixed) - at,
            Edge::End => at - f64::from(fixed),
        };
        let before = (self.song_panel.zoom, self.song_panel.scroll.0);
        self.song_panel.zoom = layout.zoom_for(measures);
        if edge == Edge::Start {
            let shown = self.song_layout(size).visible_measures();
            self.song_panel.scroll.0 = fixed.saturating_sub(shown);
        }
        before != (self.song_panel.zoom, self.song_panel.scroll.0)
    }

    /// Ctrl+wheel over the arrangement: measures wider for notches up,
    /// narrower down, the measure under the pointer kept under it; whether
    /// it was the arrangement's.
    pub fn song_zoom_wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, lines: isize) -> bool {
        let layout = self.song_layout(size);
        if self.song().is_none() || self.workspace != Workspace::Song || lines == 0 || !layout.grid.contains(x, y) {
            return false;
        }
        let first = self.song_scroll(&layout, self.song_rows().len()).0;
        let anchor = layout.measure_under(x, first);
        let factor = if lines > 0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
        for _ in 0..lines.unsigned_abs() {
            self.song_panel.zoom = self.song_panel.zoom.times(factor);
        }
        let across = self.song_layout(size).measure_under(x, 0);
        self.song_panel.scroll.0 = (anchor - across).round().max(0.0) as u32;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::session::session_song::tests::SIZE;
    use crate::song_view::MeasureZoom;
    use crate::ui_input::Button;
    use ordrumbox_core::Library;

    /// A song of 64 measures, one clip every 8, in the song panel.
    fn long_song() -> Session {
        let clips: Vec<String> =
            (0..8).map(|i| format!(r#"{{"pattern":"p","startMeasure":{},"measureCount":8}}"#, i * 8)).collect();
        let json = format!(r#"{{"songs":[{{"name":"s","clips":[{}]}}],"patterns":[{{"name":"p"}}]}}"#, clips.join(","));
        let mut s = Session::new(Library::parse(&json).unwrap().0, KitSet::empty(), Vec::new());
        s.toggle_song_view();
        s
    }

    #[test]
    fn the_end_of_the_thumb_dragged_zooms_and_its_start_stays() {
        let mut s = long_song();
        let layout = s.song_layout(SIZE);
        let (bar, shown) = (layout.scroll_bar(Axis::Measures), layout.visible_measures());
        assert!(shown < 64, "{shown}");
        // The end handle, dragged to the bar's end: the whole song across.
        let end = bar.x + bar.w * shown as i32 / 64 - 1;
        assert!(s.song_click(SIZE, end, bar.y + 2, Button::Left));
        assert!(s.mouse_move(SIZE, bar.x + bar.w, bar.y + 2, false));
        let layout = s.song_layout(SIZE);
        assert!(layout.visible_measures() >= 64, "{}", layout.visible_measures());
        assert_eq!(s.song_panel.scroll.0, 0);
        s.mouse_up();
        // Dragged back to a quarter of the bar: about 16 measures.
        let layout = s.song_layout(SIZE);
        let (start, len) = layout.thumb(Axis::Measures, 0, layout.visible_measures() as usize, 64);
        assert!(s.song_click(SIZE, start + len - 1, bar.y + 2, Button::Left));
        s.mouse_move(SIZE, bar.x + bar.w / 4, bar.y + 2, false);
        let shown = s.song_layout(SIZE).visible_measures();
        assert!((16..=20).contains(&shown), "{shown}");
        s.mouse_up();
    }

    #[test]
    fn the_start_of_the_thumb_dragged_zooms_and_its_end_stays() {
        let mut s = long_song();
        s.song_panel.zoom = MeasureZoom::new(40);
        s.song_panel.scroll.0 = 40;
        let layout = s.song_layout(SIZE);
        let shown = layout.visible_measures();
        let end = s.song_scroll(&layout, 1).0 + shown;
        let (start, _) = layout.thumb(Axis::Measures, 40, shown as usize, 64);
        let bar = layout.scroll_bar(Axis::Measures);
        assert!(s.song_click(SIZE, start + 1, bar.y + 2, Button::Left));
        // To the bar's start: from the first measure to the same end.
        s.mouse_move(SIZE, bar.x, bar.y + 2, false);
        let layout = s.song_layout(SIZE);
        let first = s.song_scroll(&layout, 1).0;
        assert!(first + layout.visible_measures() >= end && first <= 1, "{first} {end}");
        s.mouse_up();
    }

    #[test]
    fn ctrl_wheel_zooms_around_the_measure_under_the_pointer() {
        let mut s = long_song();
        s.song_panel.scroll.0 = 8;
        let layout = s.song_layout(SIZE);
        let bar = layout.scroll_bar(Axis::Measures);
        let x = bar.x + bar.w / 2;
        let under = layout.measure_under(x, 8);
        assert!(s.song_zoom_wheel(SIZE, x, bar.y - 30, 3));
        assert!(s.song_panel.zoom.units() > MeasureZoom::default().units());
        let layout = s.song_layout(SIZE);
        let first = s.song_scroll(&layout, 1).0;
        assert!((layout.measure_under(x, first) - under).abs() < 1.0);
        assert!(s.song_zoom_wheel(SIZE, x, bar.y - 30, -6));
        assert!(s.song_panel.zoom.units() < MeasureZoom::default().units());
        // Outside the arrangement, it is not the zoom's.
        assert!(!s.song_zoom_wheel(SIZE, x, 0, 1));
    }
}
