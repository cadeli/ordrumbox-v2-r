//! The song panel's menus (a pattern of the list, opened by a click, a clip, a row
//! name, the bare grid) and the song edits they lead to.

use ordrumbox_core::{SongClip, num};

use super::Session;
use super::session_confirm::Pending;
use super::session_grid::moved_index;
use crate::panel::{Dropdown, Id};
use crate::song_view::Row;
use std::path::PathBuf;

impl Session {
    /// A pattern of the list: add it at the playhead (when there is a song),
    /// move it up or down the list, rename it, or delete it.
    pub(super) fn list_menu(&mut self, p: usize, x: i32, y: i32) {
        let patterns = &self.editor.library().patterns;
        let Some(pattern) = patterns.get(p) else { return };
        let (title, id, count) = (self.pattern_label(&pattern.id), pattern.id.clone(), patterns.len());
        let mut lines = Vec::new();
        if self.song().is_some() {
            let measure = self.playhead_measure();
            lines.push((format!("add:{measure}:{id}"), format!("Add at measure {}", measure + 1)));
        }
        if p > 0 {
            lines.push((format!("pattern-up:{p}"), "Move up".to_owned()));
        }
        if p + 1 < count {
            lines.push((format!("pattern-down:{p}"), "Move down".to_owned()));
        }
        lines.extend([
            (format!("rename:{p}"), "Rename".to_owned()),
            (format!("pattern-delete:{p}"), "Delete".to_owned()),
        ]);
        self.chrome.widgets.open_dropdown(Dropdown::menu(Id::SongMenu, x, y, title, lines));
    }

    /// A clip: repeat it after itself, or remove it.
    pub(super) fn clip_menu(&mut self, c: usize, x: i32, y: i32) {
        let Some(clip) = self.song().and_then(|s| s.clips.get(c)).cloned() else { return };
        let title = clip_label(&self.pattern_label(&clip.pattern_id), &clip);
        let lines = [(format!("next:{c}"), "Next".to_owned()), (format!("delete:{c}"), "Delete".to_owned())];
        let shortcuts = crate::menu::shown_shortcuts(&["Ctrl+D", "Delete"]);
        let menu = Dropdown::menu(Id::SongMenu, x, y, title, lines);
        self.chrome.widgets.open_dropdown(menu.with_shortcuts(shortcuts));
    }

    /// Row `r`'s name: add its pattern at the playhead, move the row up or
    /// down, or remove every clip of it.
    pub(super) fn row_menu(&mut self, rows: &[Row], r: usize, x: i32, y: i32) {
        let row = &rows[r];
        let count = self.song().map_or(0, |s| s.clips.iter().filter(|c| c.pattern_id == row.id).count());
        let measure = self.playhead_measure();
        let title = format!("{} - {count} clip(s)", row.name);
        let mut lines = vec![(format!("add:{measure}:{}", row.id), format!("Add at measure {}", measure + 1))];
        if !row.orphan && r > 0 {
            lines.push((format!("up:{r}"), "Move up".to_owned()));
        }
        if rows.get(r + 1).is_some_and(|next| !next.orphan) && !row.orphan {
            lines.push((format!("down:{r}"), "Move down".to_owned()));
        }
        lines.push((format!("row:{}", row.id), "Delete row".to_owned()));
        self.chrome.widgets.open_dropdown(Dropdown::menu(Id::SongMenu, x, y, title, lines));
    }

    /// The name a clip's pattern is shown by (its id when it is gone).
    pub(super) fn pattern_label(&self, id: &str) -> String {
        self.editor.library().pattern(id).and_then(|p| p.name.clone()).unwrap_or_else(|| id.to_owned())
    }

    /// An action of a song panel menu (see the menus above, and Load).
    pub(super) fn song_menu_pick(&mut self, value: &str) {
        // A path may hold a `:` (`C:\…`): not split.
        if let Some(path) = value.strip_prefix("load:") {
            return self.ask(Pending::LoadProject(PathBuf::from(path)));
        }
        let mut parts = value.splitn(3, ':');
        let (action, a, b) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next());
        let result = match (action, b) {
            ("loop", None) => return self.loop_menu_pick(a),
            ("add", Some(id)) => {
                let measure = a.parse().unwrap_or(0);
                let label = self.pattern_label(id);
                self.editor
                    .add_pattern_at_measure(id, measure, None, None)
                    .map(|_| format!("\"{label}\" added at measure {}", measure + 1))
            }
            ("next", None) => {
                if let Ok(clip) = a.parse() {
                    self.repeat_clip(clip);
                }
                return;
            }
            ("delete", None) => {
                if let Some(c) = a.parse().ok().filter(|&c: &usize| self.song().is_some_and(|s| c < s.clips.len())) {
                    self.undoable(|s| s.remove_clip(c));
                }
                return;
            }
            ("up", None) => {
                if let Ok(r) = a.parse::<usize>() {
                    self.move_row(r, r.saturating_sub(1));
                }
                return;
            }
            ("down", None) => {
                if let Ok(r) = a.parse::<usize>() {
                    self.move_row(r, r + 1);
                }
                return;
            }
            ("row", None) => return self.undoable(|s| s.remove_row(a)),
            ("pattern-up" | "pattern-down" | "rename" | "pattern-delete", None) => {
                let Some(p) = a.parse().ok().filter(|&p: &usize| p < self.editor.library().patterns.len()) else {
                    return;
                };
                match action {
                    "pattern-up" => self.move_pattern(p, p.saturating_sub(1)),
                    "pattern-down" => self.move_pattern(p, p + 1),
                    "rename" => self.text_entry(Id::SongPattern(p)),
                    _ => self.undoable(|s| s.remove_pattern_now(p)),
                }
                return;
            }
            _ => return,
        };
        self.song_edit_result(result);
    }

    /// Move row `from` of the song to the place of row `to`. Rows
    /// follow the library's order, so the row's pattern takes the place of
    /// that row's pattern in the library, in one undo step.
    pub(super) fn move_row(&mut self, from: usize, to: usize) {
        let rows = self.song_rows();
        let library = self.editor.library();
        let index = |r: usize| rows.get(r).filter(|row| !row.orphan).and_then(|row| library.pattern_index(&row.id));
        let (Some(from), Some(to)) = (index(from), index(to)) else { return };
        self.move_pattern(from, to);
    }

    /// Move pattern `from` of the library to index `to`, in one undo step;
    /// the pattern shown stays the same one.
    pub(super) fn move_pattern(&mut self, from: usize, to: usize) {
        if self.editor.move_pattern(from, to) {
            self.pattern_index = moved_index(self.pattern_index, from, to);
            self.song_panel.description_scroll.0 = moved_index(self.song_panel.description_scroll.0, from, to);
            self.song_panel.list_scroll.0 = moved_index(self.song_panel.list_scroll.0, from, to);
            self.edited();
        }
    }

    /// Remove pattern `p` and its clips of every song, in one undo
    /// step; the pattern shown or playing stays the same one when it is
    /// another. Its name and the clips removed.
    pub(super) fn remove_pattern_with_clips(&mut self, p: usize) -> Result<(String, usize), String> {
        let library = self.editor.library();
        if library.patterns.len() <= 1 {
            return Err("The last pattern of the library stays".into());
        }
        let Some(id) = library.patterns.get(p).map(|pattern| pattern.id.clone()) else {
            return Err(format!("No pattern {}", p + 1));
        };
        let (name, songs) = (self.pattern_label(&id), library.songs.len());
        let clips = self.editor.batch(&format!("Remove pattern \"{name}\""), |editor| {
            let mut clips = 0;
            for s in 0..songs {
                clips += editor.remove_pattern_clips(&id, Some(s))?.len();
            }
            if !editor.remove_pattern(p) {
                return Err("the pattern was not removed".to_owned());
            }
            Ok(clips)
        })?;
        if self.pattern_index > p {
            self.pattern_index -= 1;
        }
        if let Some(controller) = self.playback.controller_mut() {
            controller.remove_pattern(p);
        }
        self.clamp_selection();
        Ok((name, clips))
    }

    /// Pattern `p` deleted with its clips.
    pub(super) fn remove_pattern_now(&mut self, p: usize) {
        let result = self
            .remove_pattern_with_clips(p)
            .map(|(name, clips)| format!("Removed pattern \"{name}\" and {clips} clip(s)"));
        self.song_edit_result(result);
    }

    /// Remove clip `c` of the song.
    pub(super) fn remove_clip(&mut self, c: usize) {
        let Some(clip) = self.song().and_then(|song| song.clips.get(c).cloned()) else { return };
        let label = self.pattern_label(&clip.pattern_id);
        self.editor.remove_song_clips(None, &[c]);
        self.song_edit_result(Ok(format!("Removed \"{label}\" at measure {}", clip.start_measure + 1)));
    }

    /// Remove every clip of pattern `id` from the song.
    pub(super) fn remove_row(&mut self, id: &str) {
        let label = self.pattern_label(id);
        let result = self.editor.remove_pattern_clips(id, None).map(|_| format!("Removed \"{label}\" from the song"));
        self.song_edit_result(result);
    }

    /// What a song edit gave: its message, and the edit taken into account.
    pub(super) fn song_edit_result(&mut self, result: Result<String, String>) {
        match result {
            Ok(message) => {
                self.edited();
                self.message = message;
            }
            Err(message) => self.message = message,
        }
    }
}

/// A clip's description (its `title`): `PATTERN - measure N, M measure(s)`.
pub(super) fn clip_label(label: &str, clip: &SongClip) -> String {
    format!("{label} - measure {}, {} measure(s)", clip.start_measure + 1, num::text(clip.length_measures))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::session_song::tests::{SIZE, choose, clips, menu, session};
    use crate::song_view;
    use crate::ui_input::Button;

    #[test]
    fn clip_labels() {
        let clip = SongClip { pattern_id: "rock".into(), start_measure: 2, length_measures: 0.75 };
        assert_eq!(clip_label("Rock", &clip), "Rock - measure 3, 0.75 measure(s)");
    }

    #[test]
    fn the_mouse_places_repeats_and_removes_clips() {
        let mut s = session();
        // Measures 24 px wide, whatever the zoom the panel opens at.
        s.song_panel.zoom = song_view::MeasureZoom::new(24);
        let l = s.song_layout(SIZE);
        // The ruler aims the cursor; a pattern of the list is added there.
        let ruler_y = l.grid.y + 14 + 4;
        let measure_x = |m: i32| l.grid.x + song_view::LABEL_WIDTH + m * 24 + 12;
        assert!(s.song_click(SIZE, measure_x(1), ruler_y, Button::Left));
        assert!(s.song_release(SIZE), "a click on the ruler: on its release");
        assert_eq!(s.transport.song_cursor, 1);
        right_click(&mut s, l.list.x + 2, l.list.y + 2);
        assert_eq!(menu(&s), ["add:1:a", "pattern-down:0", "rename:0", "pattern-delete:0"]);
        choose(&mut s, 0);
        assert_eq!(s.message, "\"a\" added at measure 2");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0), ("a".into(), 1, 0.75)]);
        // Rows: a, b (library order). The clip of a: repeat it.
        let row_y = |r: i32| l.grid.y + 14 + 18 + r * 22 + 11;
        assert!(right_click(&mut s, measure_x(1) - 4, row_y(0)));
        assert_eq!(menu(&s), ["next:1", "delete:1"]);
        choose(&mut s, 0);
        assert_eq!(s.message, "\"a\" repeated at measure 2.75");
        assert_eq!(clips(&s)[2], ("a".into(), 1, 0.75));
        // The bare grid of row a, measure 0: a click adds a there at once,
        // selected; a right-click offers it in a menu.
        assert!(right_click(&mut s, measure_x(0), row_y(0)));
        assert_eq!(menu(&s), ["add:0:a"]);
        s.chrome.widgets.close_dropdown();
        assert!(click_and_release(&mut s, measure_x(0), row_y(0)));
        assert!(menu(&s).is_empty());
        assert_eq!(clips(&s)[3], ("a".into(), 0, 0.75));
        assert_eq!(s.selected_clip(), Some(3));
        // Nothing past the song's 2 measures.
        assert!(!click_and_release(&mut s, measure_x(2), row_y(1)));
        // Row a's name: delete the row.
        assert!(right_click(&mut s, l.grid.x + 2, row_y(0)));
        assert_eq!(menu(&s), ["add:1:a", "down:0", "row:a"]);
        choose(&mut s, 2);
        assert_eq!(s.message, format!("Removed \"a\" from the song ({}+Z: undo)", crate::menu::command_label()));
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
        s.undo();
        assert_eq!(clips(&s).len(), 4);
    }

    /// A right-click, which opens the menu of what is under it.
    fn right_click(s: &mut Session, x: i32, y: i32) -> bool {
        s.song_click(SIZE, x, y, Button::Right)
    }

    /// A left click without a drag (press and release), which acts on what is under it.
    fn click_and_release(s: &mut Session, x: i32, y: i32) -> bool {
        let handled = s.song_click(SIZE, x, y, Button::Left);
        s.song_release(SIZE);
        handled
    }

    #[test]
    fn the_pattern_list_adds_moves_and_deletes_patterns() {
        let mut s = session();
        let l = s.song_layout(SIZE);
        let row = |r: i32| (l.list.x + 2, l.list.y + r * 12 + 2);
        // Add it at the playhead from its menu.
        let (x, y) = row(0);
        click_and_release(&mut s, x, y);
        assert!(menu(&s).is_empty(), "a left click shows the pattern, no menu");
        right_click(&mut s, x, y);
        choose(&mut s, 0);
        assert_eq!(s.message, "\"a\" added at measure 1");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0), ("a".into(), 0, 0.75)]);
        // Move down, then up: the pattern shown follows.
        right_click(&mut s, x, y);
        choose(&mut s, 1);
        assert_eq!(s.editor.library().patterns[1].name.as_deref(), Some("a"));
        assert_eq!(s.pattern_index, 1);
        let (x, y) = row(1);
        right_click(&mut s, x, y);
        assert_eq!(menu(&s), ["add:0:a", "pattern-up:1", "rename:1", "pattern-delete:1"]);
        choose(&mut s, 1);
        assert_eq!(s.editor.library().patterns[0].name.as_deref(), Some("a"));
        // Delete is done at once, takes the clips along, and is one undo step.
        let (x, y) = row(1);
        right_click(&mut s, x, y);
        choose(&mut s, 3);
        assert!(!s.confirm_open());
        assert_eq!(
            s.message,
            format!("Removed pattern \"b\" and 1 clip(s) ({}+Z: undo)", crate::menu::command_label())
        );
        assert_eq!(s.editor.library().patterns.len(), 1);
        assert_eq!(clips(&s), [("a".into(), 0, 0.75)]);
        // The last pattern stays.
        s.song_menu_pick("pattern-delete:0");
        assert_eq!(s.message, "The last pattern of the library stays");
        s.undo();
        assert_eq!(s.editor.library().patterns.len(), 2);
        assert_eq!(clips(&s).len(), 2);
    }

    #[test]
    fn rows_move_by_their_menu_and_by_a_drag() {
        let mut s = session();
        s.song_menu_pick("add:4:a");
        let names = |s: &Session| s.song_rows().iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&s), ["a", "b"]);
        let l = s.song_layout(SIZE);
        let name_x = l.grid.x + 4;
        let row_y = |r: i32| l.grid.y + 14 + 18 + r * 22 + 11;

        // The first row moves down only, the last one up only.
        assert!(right_click(&mut s, name_x, row_y(0)));
        assert!(menu(&s).contains(&"down:0".to_owned()) && !menu(&s).iter().any(|v| v.starts_with("up:")));
        s.chrome.widgets.close_dropdown();
        assert!(right_click(&mut s, name_x, row_y(1)));
        assert!(menu(&s).contains(&"up:1".to_owned()) && !menu(&s).iter().any(|v| v.starts_with("down:")));
        s.chrome.widgets.close_dropdown();

        // The pattern shown follows its row, through the undo too.
        s.song_menu_pick("down:0");
        assert_eq!(names(&s), ["b", "a"]);
        assert_eq!(s.message, "Move pattern \"a\"");
        assert_eq!(s.pattern().unwrap().id, "a");
        s.undo();
        assert_eq!(names(&s), ["a", "b"]);
        assert_eq!(s.pattern().unwrap().id, "a");

        // A press that hardly moves is a click, which shows the row's pattern;
        // a drag moves the row on release, in one step.
        assert!(s.song_click(SIZE, name_x, row_y(1), Button::Left));
        s.mouse_move(SIZE, name_x, row_y(1) + 1, false);
        assert!(s.song_release(SIZE));
        assert!(s.chrome.widgets.dropdown().is_none());
        assert_eq!(s.pattern().unwrap().id, "b");
        assert_eq!(names(&s), ["a", "b"]);
        s.song_click(SIZE, name_x, row_y(1), Button::Left);
        s.mouse_move(SIZE, name_x, row_y(0), false);
        let rows = s.song_rows();
        assert_eq!(s.song_view_data(&rows, &l).row_drop, Some((1, 0)));
        assert_eq!(s.song_view_data(&rows, &l).room, 0, "no page to drop on for a row");
        assert!(s.song_release(SIZE));
        assert_eq!(names(&s), ["b", "a"]);
        s.undo();
        assert_eq!(names(&s), ["a", "b"]);
    }
}
