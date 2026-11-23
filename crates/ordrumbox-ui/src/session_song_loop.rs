//! The song's loop set on the ruler: a drag selects the steps it plays (and
//! turns it on), whole measures, beats or steps as fine as the zoom shows
//! them; a drag on one of its ends moves that end, on or off as it is. A
//! click without a drag only puts the cursor on the measure clicked: the
//! loop is turned off with Ctrl+L, not lost to a click. A right-click opens
//! the loop's menu: turn it on or off, remove it.

use super::Session;
use super::session_song::DRAG_START_PX;
use crate::panel::{Dropdown, Id};
use crate::scale::Scale;
use crate::scroll_thumb::Edge;
use ordrumbox_core::automation::STEPS_PER_MEASURE;

/// A press on the ruler, held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoopDrag {
    /// A new loop from step `anchor`, pressed at `press_x` on measure
    /// `measure`; `started` once the pointer has really moved.
    New { anchor: u32, press_x: i32, measure: u32, started: bool },
    /// An end of the loop, moved.
    End(Edge),
}

impl Session {
    /// A press on the ruler at `x`, on measure `measure`: a new loop or, a click, none.
    pub(super) fn press_ruler(&mut self, size: (i32, i32, Scale), x: i32, measure: u32) {
        let layout = self.song_layout(size);
        let first = self.song_scroll(&layout, self.song_rows().len() + self.song_lane_rows()).0;
        let anchor = layout.step_at(x, first);
        self.song_panel.loop_drag = Some(LoopDrag::New { anchor, press_x: x, measure, started: false });
    }

    /// A press on an end of the loop: a drag moves it.
    pub(super) fn press_loop_end(&mut self, edge: Edge) {
        self.song_panel.loop_drag = Some(LoopDrag::End(edge));
    }

    /// The pointer at `x` while the ruler is pressed: the loop selected, or
    /// its end moved, on the zoom's lines; whether it changed.
    pub(super) fn loop_drag_move(&mut self, size: (i32, i32, Scale), x: i32) -> bool {
        let Some(drag) = self.song_panel.loop_drag else { return false };
        let layout = self.song_layout(size);
        let first = self.song_scroll(&layout, self.song_rows().len() + self.song_lane_rows()).0;
        let snap = layout.snap_steps();
        let total = self.song().map_or(1, |song| song.length_measures().max(1.0).ceil() as u32) * STEPS_PER_MEASURE;
        let before = self.song_loop_marks();
        let region = match drag {
            LoopDrag::New { anchor, press_x, measure, started } => {
                let started = started || (x - press_x).abs() >= DRAG_START_PX;
                self.song_panel.loop_drag = Some(LoopDrag::New { anchor, press_x, measure, started });
                if !started {
                    return false;
                }
                let step = layout.step_at(x, first).min(total - 1);
                let (low, high) = (anchor.min(step), anchor.max(step));
                Some((low / snap * snap, ((high / snap + 1) * snap).min(total)))
            }
            LoopDrag::End(edge) => before.and_then(|(start, end)| {
                let line = layout.loop_line_at(x, first, snap, total);
                let (start, end, edge) = edge.moved_to((start, end), line);
                self.song_panel.loop_drag = Some(LoopDrag::End(edge));
                (start < end).then_some((start, end))
            }),
        };
        match (region, drag) {
            (Some(_), _) if region == before => false,
            (Some((start, end)), LoopDrag::New { .. }) => {
                self.set_song_loop(start, end);
                true
            }
            (Some((start, end)), LoopDrag::End(_)) => {
                self.move_song_loop(start, end);
                true
            }
            (None, _) => false,
        }
    }

    /// A right-click on the ruler at `(x, y)`: the loop's menu.
    pub(super) fn loop_menu(&mut self, x: i32, y: i32) {
        if !self.has_loop() {
            self.message = super::session_transport::NO_LOOP.into();
            return;
        }
        let toggle = if self.transport.is_loop_on { "Turn the loop off" } else { "Turn the loop on" };
        let values = vec!["loop:toggle".into(), "loop:clear".into()];
        let labels = vec![toggle.into(), "Remove the loop".into()];
        let shortcuts = crate::menu::shown_shortcuts(&["Ctrl+L", ""], self.host.hosted);
        let menu = Dropdown::menu(Id::SongMenu, x, y, "Loop".into(), values, labels);
        self.chrome.widgets.dropdown = Some(menu.with_shortcuts(shortcuts));
    }

    /// A line of the loop's menu: `toggle` or `clear`.
    pub(super) fn loop_menu_pick(&mut self, action: &str) {
        match action {
            "toggle" => self.toggle_loop(),
            "clear" => self.clear_loop(),
            _ => {}
        }
    }

    /// The ruler released: a click without a drag puts the cursor on its
    /// measure; whether the ruler was pressed.
    pub(super) fn loop_drag_release(&mut self) -> bool {
        let Some(drag) = self.song_panel.loop_drag.take() else { return false };
        if let LoopDrag::New { measure, started: false, .. } = drag {
            self.set_song_cursor(measure);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, choose, menu, session};
    use super::Session;
    use crate::song_view::{MeasureZoom, SongHit};
    use crate::ui_input::Button;

    /// The middle of the ruler's height.
    fn ruler_y(s: &Session) -> i32 {
        let layout = s.song_layout(SIZE);
        let rows = s.song_rows();
        let view = s.song_view_data(&rows, &layout);
        let g = layout.grid;
        let on = |y: i32| (g.x..g.x + g.w).any(|x| matches!(layout.hit(&view, x, y), Some(SongHit::Ruler(_))));
        let top = (g.y..g.y + g.h).find(|&y| on(y)).expect("a ruler");
        top + (top..g.y + g.h).take_while(|&y| on(y)).count() as i32 / 2
    }

    /// Where `wanted` is on the ruler, scanning it from the left.
    fn on_ruler(s: &Session, wanted: impl Fn(i32, SongHit) -> bool) -> i32 {
        let layout = s.song_layout(SIZE);
        let rows = s.song_rows();
        let view = s.song_view_data(&rows, &layout);
        let y = ruler_y(s);
        (layout.grid.x..layout.grid.x + layout.grid.w)
            .find(|&x| layout.hit(&view, x, y).is_some_and(|hit| wanted(x, hit)))
            .expect("on the ruler")
    }

    /// The ruler's y, and the x of step `step` there.
    fn step_x(s: &Session, step: u32) -> (i32, i32) {
        let layout = s.song_layout(SIZE);
        let first = s.song_scroll(&layout, s.song_rows().len()).0;
        let x = on_ruler(s, |x, hit| matches!(hit, SongHit::Ruler(_)) && layout.step_at(x, first) == step);
        (x + 2, ruler_y(s))
    }

    fn drag(s: &mut Session, from: (i32, i32), to: i32) {
        assert!(s.song_click(SIZE, from.0, from.1, Button::Left));
        s.mouse_move(SIZE, to, from.1 + 50, false);
        assert!(s.song_release(SIZE));
    }

    #[test]
    fn a_drag_on_the_ruler_sets_a_loop_whose_ends_move_and_a_click_clears_it() {
        let mut s = session();
        s.set_size(SIZE);
        // Steps shown: a drag selects whole steps.
        assert_eq!(s.song_layout(SIZE).snap_steps(), 1);
        let ((from, y), (to, _)) = (step_x(&s, 5), step_x(&s, 10));
        drag(&mut s, (from, y), to);
        assert_eq!(s.song_loop_region(), Some((5, 11)));
        assert_eq!(s.message, "loop: 1.6 to 1.12, Ctrl+L turns it off");

        // Its end, dragged to step 14's line; its start past it: they swap.
        let end = on_ruler(&s, |_, hit| hit == SongHit::LoopEnd(crate::scroll_thumb::Edge::End));
        let to = step_x(&s, 14).0 - 2;
        drag(&mut s, (end, y), to);
        assert_eq!(s.song_loop_region(), Some((5, 14)));
        let start = on_ruler(&s, |_, hit| hit == SongHit::LoopEnd(crate::scroll_thumb::Edge::Start));
        let to = step_x(&s, 20).0 - 2;
        drag(&mut s, (start, y), to);
        assert_eq!(s.song_loop_region(), Some((14, 20)));

        // A click: the cursor on that measure, the loop kept.
        s.set_song_cursor(1);
        let (x, y) = step_x(&s, 3);
        assert!(s.song_click(SIZE, x, y, Button::Left));
        assert!(s.song_release(SIZE));
        assert_eq!((s.song_loop_region(), s.transport.song_cursor), (Some((14, 20)), 0));

        // Off: its ends stay, and move; on again where they are.
        s.toggle_loop();
        assert_eq!((s.song_loop_region(), s.song_loop_marks()), (None, Some((14, 20))));
        let end = on_ruler(&s, |_, hit| hit == SongHit::LoopEnd(crate::scroll_thumb::Edge::End));
        let to = step_x(&s, 24).0 - 2;
        drag(&mut s, (end, y), to);
        assert_eq!((s.song_loop_region(), s.song_loop_marks()), (None, Some((14, 24))));
        assert_eq!(s.message, "loop: 1.15 to 2.9, off, Ctrl+L turns it on");
        s.toggle_loop();
        assert_eq!((s.song_loop_region(), s.message.as_str()), (Some((14, 24)), "loop on"));

        // A right-click: turn it off or remove it.
        assert!(s.song_click(SIZE, x, y, Button::Right));
        assert_eq!(menu(&s), ["loop:toggle", "loop:clear"]);
        choose(&mut s, 0);
        assert_eq!((s.song_loop_region(), s.song_loop_marks()), (None, Some((14, 24))));
        assert!(s.song_click(SIZE, x, y, Button::Right));
        choose(&mut s, 1);
        assert_eq!((s.song_loop_marks(), s.message.as_str()), (None, "loop cleared"));
        assert!(s.song_click(SIZE, x, y, Button::Right));
        assert!(s.message.starts_with("no loop"), "no menu without a loop");

        // Zoomed out: whole measures.
        s.song_panel.zoom = MeasureZoom::new(24);
        let ((from, y), (to, _)) = (step_x(&s, 5), step_x(&s, 20));
        drag(&mut s, (from, y), to);
        assert_eq!(s.song_loop_region(), Some((0, 32)));
    }
}
