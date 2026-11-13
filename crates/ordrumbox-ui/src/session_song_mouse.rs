//! The song panel under the mouse: clicks on the pattern list and the
//! arrangement, drags of patterns, clips and rows (one undo step on release),
//! the scroll bars, the wheel, and the clip tooltip.

use crate::scale::Scale;
use ordrumbox_core::song::measures_for_pattern;

use super::session_panel::DOUBLE_CLICK;
use super::session_song::{DRAG_START_PX, DragSource, SongDrag};
use super::session_song_menu::clip_label;
use super::{Session, Workspace};
use crate::canvas::Canvas;
use crate::panel::{Dropdown, Id, Rect};
use crate::song_view::{Axis, Row, SongGhost, SongHit, SongLayout};
use crate::ui_input::Button;
use crate::view;
use std::time::Instant;

impl Session {
    /// A click on the song panel's list or arrangement; whether it was theirs.
    pub fn song_click(&mut self, size: (i32, i32, Scale), x: i32, y: i32, button: Button) -> bool {
        let layout = self.song_layout(size);
        let rows = self.song_rows();
        let Some(hit) = layout.hit(&self.song_view_data(&rows, &layout), x, y) else { return false };
        match (hit, button) {
            (SongHit::Pattern(p), Button::Left) => {
                let id = Id::SongPattern(p);
                let now = Instant::now();
                let double =
                    self.chrome.widgets.last_click.is_some_and(|(last, at)| last == id && now - at < DOUBLE_CLICK);
                self.chrome.widgets.last_click = if double { None } else { Some((id, now)) };
                if double {
                    self.add_at_playhead(p);
                } else {
                    self.step_pattern(p as isize - self.pattern_index as isize);
                    self.press_song_drag(DragSource::Pattern(p), x, y);
                }
            }
            (SongHit::Clip(clip), Button::Left) => {
                let first = self.song_scroll(&layout, rows.len()).0;
                let start = self.song().and_then(|s| s.clips.get(clip)).map_or(0, |c| c.start_measure);
                let grab = layout.measure_at(x, first).saturating_sub(start);
                self.press_song_drag(DragSource::Clip { clip, grab }, x, y);
            }
            (SongHit::RowName(r), Button::Left) if !rows[r].orphan => {
                self.press_song_drag(DragSource::Row(r), x, y);
            }
            (SongHit::ScrollBar(axis), Button::Left) => {
                self.song_panel.bar_drag = Some(axis);
                self.song_bar_to(size, axis, x, y);
            }
            (SongHit::Pattern(p), Button::Right) => self.list_menu(p, x, y),
            (SongHit::Description, Button::Left) => self.text_entry(Id::PatternDescription),
            (SongHit::Ruler(m), Button::Left) => self.set_song_cursor(m),
            (SongHit::Clip(c), Button::Right) => self.clip_menu(c, x, y),
            (SongHit::RowName(r), Button::Right) => self.row_menu(&rows, r, x, y),
            (SongHit::Cell { row, measure }, Button::Right) => {
                let row = &rows[row];
                let title = format!("Add \"{}\" at measure {}", row.name, measure + 1);
                self.chrome.widgets.dropdown = Some(Dropdown::menu(
                    Id::SongMenu,
                    x,
                    y,
                    title,
                    vec![format!("add:{measure}:{}", row.id)],
                    vec!["Add here".into()],
                ));
            }
            _ => return false,
        }
        true
    }

    /// Shift+click on the song panel: on the ruler, a loop from the cursor to
    /// that measure; elsewhere a plain click.
    pub fn song_shift_click(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let layout = self.song_layout(size);
        let rows = self.song_rows();
        if let Some(SongHit::Ruler(measure)) = layout.hit(&self.song_view_data(&rows, &layout), x, y) {
            self.loop_to(measure);
            return true;
        }
        self.song_click(size, x, y, Button::Left)
    }

    // ─── Dragging ────────────────────────────────────────────────────────────

    fn press_song_drag(&mut self, source: DragSource, x: i32, y: i32) {
        self.song_panel.drag = Some(SongDrag {
            source,
            press: (x, y),
            x,
            y,
            started: false,
            revision: self.editor.revision(),
            song: self.editor.selected_song(),
        });
    }

    /// The drag under way, unless what it holds may have changed under it.
    fn live_song_drag(&self) -> Option<SongDrag> {
        self.song_panel.drag.filter(|d| {
            d.started
                && self.song_view_open()
                && d.revision == self.editor.revision()
                && d.song == self.editor.selected_song()
        })
    }

    /// Measures shown past the end while dragging a clip or a pattern: a page to drop on.
    pub(super) fn song_room(&self, layout: &SongLayout) -> u32 {
        let dragged = self.live_song_drag().filter(|d| !matches!(d.source, DragSource::Row(_)));
        dragged.map_or(0, |_| layout.visible_measures())
    }

    /// The measure the pointer is on, and the drag.
    fn song_drag_at(&self, layout: &SongLayout, rows: usize) -> Option<(SongDrag, u32)> {
        let drag = self.live_song_drag()?;
        Some((drag, layout.measure_at(drag.x, self.song_scroll(layout, rows).0)))
    }

    /// The clip being dragged and where it would start.
    pub(super) fn song_dragged_clip(&self, layout: &SongLayout, rows: usize) -> Option<(usize, u32)> {
        match self.song_drag_at(layout, rows)? {
            (SongDrag { source: DragSource::Clip { clip, grab }, .. }, at) => Some((clip, at.saturating_sub(grab))),
            _ => None,
        }
    }

    /// The row being dragged and the row it would take, among those of
    /// patterns of the library (a missing pattern's row stays last).
    pub(super) fn song_row_drop(&self, layout: &SongLayout, rows: &[Row]) -> Option<(usize, usize)> {
        let drag = self.live_song_drag()?;
        let DragSource::Row(from) = drag.source else { return None };
        let movable = rows.iter().filter(|r| !r.orphan).count();
        let first = self.song_scroll(layout, rows.len()).1;
        Some((from, layout.drop_row(drag.y, first).min(movable.saturating_sub(1))))
    }

    /// The pattern being dragged along the list and the place it would take.
    pub(super) fn song_list_drop(&self, layout: &SongLayout) -> Option<(usize, usize)> {
        let drag = self.live_song_drag()?;
        let DragSource::Pattern(from) = drag.source else { return None };
        if !layout.list.contains(drag.x, drag.y) {
            return None;
        }
        let count = self.editor.library().patterns.len();
        Some((from, layout.list_drop_row(drag.y, self.list_scroll(layout), count)))
    }

    /// Where the pattern being dragged would land, the pointer over the arrangement.
    pub(super) fn song_ghost(&self, layout: &SongLayout, rows: &[Row]) -> Option<SongGhost> {
        let (drag, measure) = self.song_drag_at(layout, rows.len())?;
        let DragSource::Pattern(p) = drag.source else { return None };
        if self.song().is_none() || !layout.grid.contains(drag.x, drag.y) {
            return None;
        }
        let pattern = self.editor.library().patterns.get(p)?;
        Some(SongGhost {
            name: pattern.name.clone().unwrap_or_else(|| pattern.id.clone()),
            row: rows.iter().position(|r| r.id == pattern.id).unwrap_or(rows.len()),
            measure,
            measure_count: measures_for_pattern(pattern.beat_count),
        })
    }

    /// The mouse moved with a press held; whether to redraw.
    pub(super) fn song_drag_move(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = &mut self.song_panel.drag else { return false };
        (drag.x, drag.y) = (x, y);
        let (dx, dy) = (x - drag.press.0, y - drag.press.1);
        drag.started |= dx.abs() >= DRAG_START_PX || dy.abs() >= DRAG_START_PX;
        drag.started
    }

    /// The press released: a drag places or moves its clip; whether it was one.
    pub fn song_release(&mut self, size: (i32, i32, Scale)) -> bool {
        let layout = self.song_layout(size);
        let rows = self.song_rows();
        let at = self.song_drag_at(&layout, rows.len());
        let ghost = self.song_ghost(&layout, &rows);
        let row_drop = self.song_row_drop(&layout, &rows);
        let list_drop = self.song_list_drop(&layout);
        let over_grid = at.is_some_and(|(drag, _)| layout.grid.contains(drag.x, drag.y));
        self.song_panel.drag = None;
        let Some((drag, measure)) = at else { return false };
        match drag.source {
            DragSource::Pattern(p) => {
                if let Some((from, to)) = list_drop {
                    self.move_pattern(from, to);
                } else if self.song().is_none() && over_grid {
                    self.message = "No song to add to".into();
                } else if let (Some(ghost), Some(pattern)) = (ghost, self.editor.library().patterns.get(p)) {
                    let value = format!("add:{}:{}", ghost.measure, pattern.id);
                    self.song_menu_pick(&value);
                }
            }
            DragSource::Clip { clip, grab } => {
                let start = measure.saturating_sub(grab);
                if self.editor.move_song_clip(None, clip, start) {
                    self.edited();
                    let id = self.song().and_then(|s| s.clips.get(clip)).map(|c| c.pattern_id.clone());
                    let label = self.pattern_label(&id.unwrap_or_default());
                    self.message = format!("\"{label}\" moved to measure {}", start + 1);
                }
            }
            DragSource::Row(_) => {
                if let Some((from, to)) = row_drop {
                    self.move_row(from, to);
                }
            }
        }
        true
    }

    /// The scroll bar `axis` held at `(x, y)`: its thumb centred there; whether to redraw.
    pub(super) fn song_bar_to(&mut self, size: (i32, i32, Scale), axis: Axis, x: i32, y: i32) -> bool {
        let layout = self.song_layout(size);
        if axis == Axis::List {
            let count = self.editor.library().patterns.len();
            let first = layout.scroll_at(axis, x, y, layout.list_rows(), count);
            let changed = first != self.list_scroll(&layout);
            self.song_panel.list_scroll = (self.pattern_index, first);
            return changed;
        }
        let rows = self.song_rows().len();
        let (mut sx, mut sy) = self.song_scroll(&layout, rows);
        match axis {
            Axis::Measures => {
                let (shown, total) = (layout.visible_measures() as usize, self.song_measure_total(&layout) as usize);
                sx = layout.scroll_at(axis, x, y, shown, total) as u32;
            }
            Axis::Rows | Axis::List => sy = layout.scroll_at(axis, x, y, layout.visible_rows(), rows),
        }
        let changed = (sx, sy) != self.song_panel.scroll;
        self.song_panel.scroll = (sx, sy);
        changed
    }

    /// The wheel over the arrangement: measures, sideways ones too (Shift:
    /// rows, which macOS turns into sideways notches); over the pattern's
    /// description: its lines; over the pattern list: its rows; whether it
    /// was theirs.
    pub fn song_wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, wheel: (isize, isize), shift: bool) -> bool {
        // Content moves right and down for positive notches: both show what is before.
        let lines = wheel.0 + wheel.1;
        let layout = self.song_layout(size);
        if self.workspace == Workspace::Song && lines != 0 && layout.description.contains(x, y) {
            let first = (self.description_scroll(&layout) as isize - lines).max(0) as usize;
            self.song_panel.description_scroll = (self.pattern_index, first);
            self.song_panel.description_scroll.1 = self.description_scroll(&layout);
            return true;
        }
        if self.workspace == Workspace::Song && lines != 0 && layout.list.contains(x, y) {
            let last = layout.list_last_scroll(self.editor.library().patterns.len());
            let first = (self.list_scroll(&layout) as isize - lines).clamp(0, last as isize) as usize;
            self.song_panel.list_scroll = (self.pattern_index, first);
            return true;
        }
        if self.song().is_none() || self.workspace != Workspace::Song || lines == 0 || !layout.grid.contains(x, y) {
            return false;
        }
        let rows = self.song_rows().len();
        let (mut sx, mut sy) = self.song_scroll(&layout, rows);
        if shift {
            let last = rows.saturating_sub(layout.visible_rows());
            sy = (sy as isize - lines).clamp(0, last as isize) as usize;
        } else {
            let last = self.song_measure_total(&layout).saturating_sub(layout.visible_measures());
            sx = (i64::from(sx) - 4 * lines as i64).clamp(0, i64::from(last)) as u32;
        }
        self.song_panel.scroll = (sx, sy);
        true
    }

    /// The mouse over the arrangement: the clip it is on, for its tooltip;
    /// whether it changed.
    pub fn song_hover(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let hover = if self.workspace == Workspace::Song
            && self.chrome.widgets.drag.is_none()
            && self.song_panel.drag.is_none()
        {
            let layout = self.song_layout(size);
            let rows = self.song_rows();
            match layout.hit(&self.song_view_data(&rows, &layout), x, y) {
                Some(SongHit::Clip(c)) => Some(c),
                _ => None,
            }
        } else {
            None
        };
        let changed = hover != self.song_panel.hovered_clip;
        self.song_panel.hovered_clip = hover;
        changed
    }

    /// The tooltip of the clip under the mouse (the clip's `title`).
    /// The tooltip of the clip under the mouse, or, while a pattern is
    /// dragged over the arrangement, the measure it would be added at.
    pub(super) fn draw_song_tooltip(&self, canvas: &mut Canvas, scale: Scale) {
        if self.chrome.covered() {
            return;
        }
        let layout = self.song_layout((canvas.width(), canvas.height(), scale));
        let rows = self.song_rows();
        if let (Some(ghost), Some(drag)) = (self.song_ghost(&layout, &rows), self.live_song_drag()) {
            let at = Rect::new(drag.x, drag.y, 0, 0);
            return view::draw_tooltip(canvas, &format!("Add at measure {}", ghost.measure + 1), at, scale);
        }
        let Some(c) = self.song_panel.hovered_clip else { return };
        let Some(clip) = self.song().and_then(|s| s.clips.get(c)) else { return };
        let Some(rect) = layout.clip_rect(&self.song_view_data(&rows, &layout), c) else { return };
        view::draw_tooltip(canvas, &clip_label(&clip.pattern_id, clip), rect, scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::session::session_song::tests::{SIZE, clips, session};
    use crate::song_view;
    use ordrumbox_core::Library;

    #[test]
    fn the_scroll_bars_and_the_wheel_reach_every_measure_and_row() {
        // 30 patterns, each on a row of its own, the last clip at measure 200.
        let patterns: Vec<String> = (0..30).map(|i| format!(r#"{{"name":"p{i}"}}"#)).collect();
        let clips: Vec<String> = (0..30)
            .map(|i| format!(r#"{{"pattern":"p{i}","startMeasure":{},"measureCount":1}}"#, i * 200 / 29))
            .collect();
        let json = format!(
            r#"{{"songs":[{{"name":"s","clips":[{}]}}],"patterns":[{}]}}"#,
            clips.join(","),
            patterns.join(",")
        );
        let mut s = Session::new(Library::parse(&json).unwrap().0, KitSet::empty(), Vec::new());
        s.toggle_song_view();
        let l = s.song_layout(SIZE);
        let rows = s.song_rows();
        assert!(rows.len() > l.visible_rows());
        let scroll = |s: &Session| s.song_view_data(&rows, &l).scroll;
        let (last_measure, last_row) = (201 - l.visible_measures(), rows.len() - l.visible_rows());

        // The measures' bar, dragged to its right end and back.
        let bar = l.scroll_bar(Axis::Measures);
        assert!(s.song_click(SIZE, bar.x + 1, bar.y + 1, Button::Left));
        assert!(s.mouse_move(SIZE, bar.x + bar.w + 50, bar.y, false));
        assert_eq!(scroll(&s), (last_measure, 0));
        s.mouse_move(SIZE, bar.x - 50, bar.y, false);
        assert_eq!(scroll(&s).0, 0);
        s.mouse_up();
        assert!(!s.mouse_move(SIZE, bar.x + bar.w, bar.y, false));

        // The rows' bar: a click on its end.
        let bar = l.scroll_bar(Axis::Rows);
        assert!(s.song_click(SIZE, bar.x + 1, bar.y + bar.h - 1, Button::Left));
        s.mouse_up();
        assert_eq!(scroll(&s), (0, last_row));

        // The wheel: sideways notches scroll the measures, Shift the rows.
        let inside = (l.grid.x + l.grid.w / 2, l.grid.y + l.grid.h / 2);
        assert!(s.song_wheel(SIZE, inside.0, inside.1, (-1000, 0), false));
        assert_eq!(scroll(&s), (last_measure, last_row));
        assert!(s.song_wheel(SIZE, inside.0, inside.1, (1000, 0), true));
        assert_eq!(scroll(&s), (last_measure, 0));

        // The pattern list: the wheel, its bar, and a pattern selected out of view.
        let list_scroll = |s: &Session| s.song_view_data(&rows, &l).list_scroll;
        let last = 30 - l.list_rows();
        let in_list = (l.list.x + 2, l.list.y + 2);
        assert!(s.song_wheel(SIZE, in_list.0, in_list.1, (0, -2), false));
        assert_eq!(list_scroll(&s), 2);
        assert!(s.song_wheel(SIZE, in_list.0, in_list.1, (0, -1000), false));
        assert_eq!(list_scroll(&s), last);
        assert!(s.song_click(SIZE, in_list.0, in_list.1, Button::Left));
        s.mouse_up();
        assert_eq!(s.pattern_index, last);
        let bar = l.scroll_bar(Axis::List);
        assert!(s.song_click(SIZE, bar.x + 1, bar.y + 1, Button::Left));
        s.mouse_up();
        assert_eq!(list_scroll(&s), 0);
        s.step_pattern(1);
        assert_eq!(list_scroll(&s), last + 1 - (l.list_rows() - 1));
    }

    #[test]
    fn a_drag_adds_a_pattern_and_moves_a_clip() {
        let mut s = session();
        let l = s.song_layout(SIZE);
        let measure_x = |m: i32| l.grid.x + song_view::LABEL_WIDTH + m * 24 + 12;
        let row_y = |r: i32| l.grid.y + 14 + 18 + r * 22 + 11;
        let list = (l.list.x + 2, l.list.y + 2);

        // A press and release that hardly moves is a click.
        assert!(s.song_click(SIZE, list.0, list.1, Button::Left));
        s.mouse_move(SIZE, list.0 + 1, list.1, false);
        assert!(!s.song_release(SIZE));
        assert_eq!(clips(&s).len(), 1);

        // Pattern a, not in the arrangement yet, dropped past its end.
        s.chrome.widgets.last_click = None;
        s.song_click(SIZE, list.0, list.1, Button::Left);
        s.mouse_move(SIZE, measure_x(4), row_y(1), false);
        let rows = s.song_rows();
        let ghost = s.song_view_data(&rows, &l).ghost.unwrap();
        assert_eq!((ghost.row, ghost.measure), (1, 4));
        assert!(s.song_release(SIZE));
        assert_eq!(s.message, "\"a\" added at measure 5");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0), ("a".into(), 4, 0.75)]);
        s.undo();
        assert_eq!(clips(&s).len(), 1);

        // Dragged along the list: a line where it goes, and the pattern moves there.
        s.chrome.widgets.last_click = None;
        s.song_click(SIZE, list.0, list.1, Button::Left);
        s.mouse_move(SIZE, list.0, list.1 + 12, false);
        assert_eq!(s.song_view_data(&rows, &l).list_drop, Some((0, 1)));
        assert!(s.song_release(SIZE));
        let names: Vec<_> = s.editor.library().patterns.iter().map(|p| p.name.clone().unwrap()).collect();
        assert_eq!((names, s.pattern_index), (vec!["b".to_owned(), "a".to_owned()], 1));
        assert_eq!(clips(&s).len(), 1);
        s.undo();
        assert_eq!(s.editor.library().patterns[0].name.as_deref(), Some("a"));

        // Dropped outside the arrangement: nothing.
        s.chrome.widgets.last_click = None;
        s.song_click(SIZE, list.0, list.1, Button::Left);
        s.mouse_move(SIZE, l.column.x + 2, l.column.y + 2, false);
        assert!(s.song_release(SIZE));
        assert_eq!(clips(&s).len(), 1);

        // The clip of b, held in its second measure: it keeps its row and its grip.
        assert!(s.song_click(SIZE, measure_x(1), row_y(0), Button::Left));
        s.mouse_move(SIZE, measure_x(4), row_y(0) + 40, false);
        let rows = s.song_rows();
        assert_eq!(s.song_view_data(&rows, &l).dragged_clip, Some((0, 3)));
        assert_eq!(clips(&s)[0].1, 0, "nothing is edited before the release");
        assert!(s.song_release(SIZE));
        assert_eq!(s.message, "\"b\" moved to measure 4");
        assert_eq!(clips(&s), [("b".into(), 3, 2.0)]);
        assert_eq!(s.song().unwrap().loop_measures, Some(5.0));
        s.undo();
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);

        // An edit during the drag cancels it.
        s.song_click(SIZE, measure_x(0), row_y(0), Button::Left);
        s.mouse_move(SIZE, measure_x(4), row_y(0), false);
        s.commit_text(Id::SongPattern(1), "renamed");
        assert!(!s.song_release(SIZE));
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
    }
}
