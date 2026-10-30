//! The session's side of the song panel (`song_panel.js` and its sections):
//! the pattern list (a click selects, a double-click renames, a right-click
//! adds the pattern at the playhead), the arrangement (a click on the ruler
//! aims the cursor, where play starts; the right-click menus of clips, row
//! names and the bare grid), the song's column (name, date, description,
//! the arrangement select, Save, Load, Export, Import, and New and Delete
//! for arrangements) — and Space plays the song while the panel is shown.
//!
//! Songs are saved to `songs/` in the user data folder (the JS IndexedDB
//! store) and, with Save as and Export, to a folder picked, as `.odbox` files.

use std::time::{SystemTime, UNIX_EPOCH};

use ordrumbox_core::js;
use ordrumbox_core::json::{self, Value};
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Song, SongClip};

use super::session_panel::{DOUBLE_CLICK, PanelLayout};
use super::*;
use crate::panel::{self, Item, Kind, Rect};
use crate::song_view::{self, Row, SongHit, SongLayout, SongView};

/// The column's buttons: id and label.
const BUTTONS: [(&str, &str); 7] = [
    ("save", "Save"),
    ("saveas", "Save as"),
    ("load", "Load"),
    ("export", "Export"),
    ("import", "Import"),
    ("new", "New"),
    ("delete", "Delete"),
];

/// What is shown for a song without a name.
const UNTITLED: &str = "Untitled";

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

impl Session {
    // ─── State ───────────────────────────────────────────────────────────────

    /// The song panel is shown (the kit editor opens over it).
    pub fn song_view_open(&self) -> bool {
        self.workspace == Workspace::Song && self.kit_editor.is_none()
    }

    /// F3, View > Song: the song panel in the place of the grid, or back.
    pub fn toggle_song_view(&mut self) {
        self.set_workspace(if self.workspace == Workspace::Song { Workspace::Grid } else { Workspace::Song });
    }

    /// Show `workspace`. While playing, a switch to or from the song panel
    /// switches the playback to the view's mode, from its start
    ///; the grid and the piano roll share one mode.
    pub(super) fn set_workspace(&mut self, workspace: Workspace) {
        let was_song = self.workspace == Workspace::Song;
        self.workspace = workspace;
        self.kit_editor = None;
        self.focus = None;
        self.dropdown = None;
        self.hovered_clip = None;
        self.roll_drag = None;
        if self.playing() && was_song != (workspace == Workspace::Song) {
            let mode = self.play_mode();
            if let Some(controller) = &mut self.controller {
                controller.play(self.editor.library(), mode);
                self.played_revision = self.editor.revision();
            }
        }
    }

    /// What Space plays: the song in the song panel, else the pattern.
    fn play_mode(&self) -> Mode {
        match self.workspace {
            Workspace::Song => Mode::Song(self.editor.selected_song()),
            _ => Mode::Pattern(self.pattern_index),
        }
    }

    fn song(&self) -> Option<&Song> {
        self.editor.library().songs.get(self.editor.selected_song())
    }

    /// The library header's name, else "Untitled".
    fn song_name(&self) -> String {
        let name = self.editor.library().info.get("name").and_then(Value::as_str).unwrap_or_default();
        if name.is_empty() { UNTITLED.to_owned() } else { name.to_owned() }
    }

    fn info(&self, key: &str) -> String {
        self.editor.library().info.get(key).map(js::to_string).unwrap_or_default()
    }

    pub(super) fn song_rows(&self) -> Vec<Row> {
        self.song().map(|s| song_view::rows(s, &self.editor.library().patterns)).unwrap_or_default()
    }

    /// The song playing now, if the selected one plays.
    fn song_playing(&self) -> bool {
        self.playing() && self.controller.as_ref().is_some_and(|c| c.mode() == Mode::Song(self.editor.selected_song()))
    }

    /// Where the cursor is, in measures: where the song
    /// plays, else the measure play starts from; kept on the grid.
    fn song_cursor_measure(&self) -> f64 {
        let Some(song) = self.song() else { return 0.0 };
        let measure = match self.controller.as_ref().and_then(Controller::tick) {
            Some(tick) if self.song_playing() => song.measure_at_tick(tick as f64, f64::from(TICK)),
            _ => f64::from(self.song_cursor),
        };
        measure.min((song.length_measures() - 1.0).max(0.0))
    }

    /// The measure the playhead is on: where the song
    /// plays, else the cursor. What a pattern is added at from the lists.
    fn playhead_measure(&self) -> u32 {
        self.song_cursor_measure().floor() as u32
    }

    /// First measure and first row shown: while the song plays, the page
    /// the cursor is on; else where the wheel left them.
    fn song_scroll(&self, layout: &SongLayout, rows: usize) -> (u32, usize) {
        let Some(song) = self.song() else { return (0, 0) };
        let total = song.length_measures().max(1.0).ceil() as u32;
        let visible = layout.visible_measures();
        let last = total.saturating_sub(visible);
        let x = if self.song_playing() {
            (self.song_cursor_measure() as u32 / visible * visible).min(last)
        } else {
            self.song_scroll.0.min(last)
        };
        (x, self.song_scroll.1.min(rows.saturating_sub(layout.visible_rows())))
    }

    pub(super) fn song_layout(&self, size: (i32, i32, i32)) -> SongLayout {
        let body = self.body(size);
        SongLayout::new(body.main_w, body.editor_h, size.2, self.status.len())
    }

    /// What the song panel shows (its rows are `rows`).
    pub(super) fn song_view_data<'a>(&'a self, rows: &'a [Row], layout: &SongLayout) -> SongView<'a> {
        let library = self.editor.library();
        SongView {
            patterns: library
                .patterns
                .iter()
                .enumerate()
                .map(|(i, p)| p.name.clone().unwrap_or_else(|| format!("Pattern {i}")))
                .collect(),
            selected_pattern: self.pattern_index,
            song: self.song(),
            rows,
            cursor: self.song_cursor_measure(),
            playing: self.song_playing(),
            scroll: self.song_scroll(layout, rows.len()),
        }
    }

    // ─── The column ──────────────────────────────────────────────────────────

    /// The song's column: name, date, description, arrangement, buttons.
    pub(super) fn song_items(&self, layout: &mut PanelLayout, width: i32, height: i32, scale: i32) {
        let s = scale;
        let area = self.song_layout((width, height, scale)).column;
        if area.w <= 0 {
            return;
        }
        let (x, label_w, mut y) = (area.x, 56 * s, area.y);
        let line = 16 * s;
        let field = |y: i32| Rect::new(x + label_w, y, area.w - label_w, 12 * s);
        // The name (a click renames it), the date.
        for (label, id, text, color) in
            [("Song", Some(Id::SongName), self.song_name(), panel::TEXT), ("Date", None, self.info("date"), panel::DIM)]
        {
            layout.items.push(Item::text(x, y + 2 * s, label, panel::DIM, s));
            layout.items.push(Item::new(id, field(y), Kind::Text { text, color }));
            y += line;
        }
        // The description, a click edits it.
        let description = self.info("description");
        let (text, color) =
            if description.is_empty() { ("(description)".to_owned(), panel::DIM) } else { (description, panel::TEXT) };
        let rect = Rect::new(x, y, area.w, 12 * s);
        layout.items.push(Item::new(Some(Id::SongDescription), rect, Kind::Text { text, color }));
        y += line + 4 * s;

        // The arrangement, under its label.
        layout.items.push(Item::text(x, y, "Arrangement", panel::DIM, s));
        y += 12 * s;
        let select = Rect::new(x, y, area.w, 12 * s);
        let songs = &self.editor.library().songs;
        if songs.is_empty() {
            let text = Kind::Text { text: "No arrangement".into(), color: panel::DIM };
            layout.items.push(Item::new(None, select, text));
        } else {
            let labels = songs.iter().map(|s| s.name.clone()).collect();
            let values = (0..songs.len()).map(|i| i.to_string()).collect();
            let selected = self.editor.selected_song().min(songs.len() - 1);
            layout.items.push(Item::new(Some(Id::SongList), select, Kind::Select { values, labels, selected }));
        }
        y += line + 8 * s;

        let gap = 4 * s;
        let w = (area.w - 2 * gap) / 3;
        for (i, (id, label)) in BUTTONS.iter().enumerate() {
            let (col, row) = (i as i32 % 3, i as i32 / 3);
            let rect = Rect::new(x + col * (w + gap), y + row * (14 * s + gap), w, 14 * s);
            layout.items.push(Item::new(
                Some(Id::SongButton(id)),
                rect,
                Kind::Button { label: (*label).into(), on: false },
            ));
        }
    }

    /// A column button pressed.
    pub(super) fn song_button(&mut self, id: &str) {
        match id {
            "save" => self.save_song(),
            "load" => self.load_song_menu(),
            "saveas" => self.open_export(Export::SongSaveAs),
            "export" => self.open_export(Export::Song),
            "import" => self.open_picker(Purpose::ImportSong),
            "new" => {
                self.entry = Some(Entry {
                    id: Id::SongButton("new"),
                    spec: None,
                    title: "New arrangement name:".into(),
                    text: String::new(),
                });
            }
            "delete" => self.remove_song(),
            _ => {}
        }
    }

    /// The text entry of `id` (a song, pattern or track name, a description).
    pub(super) fn text_entry(&mut self, id: Id) {
        let (title, text) = match id {
            Id::SongName => ("Song name:", self.song_name()),
            Id::SongDescription => ("Song description:", self.info("description")),
            Id::SongPattern(p) => {
                let Some(pattern) = self.editor.library().patterns.get(p) else { return };
                ("Pattern name:", pattern.name.clone().unwrap_or_else(|| format!("Pattern {p}")))
            }
            Id::PatternDescription => {
                let Some(pattern) = self.pattern() else { return };
                ("Pattern description:", pattern.description.clone())
            }
            Id::TrackName(t) => {
                let Some(track) = self.track(t) else { return };
                ("Rename track:", track.name.clone())
            }
            _ => return,
        };
        self.entry = Some(Entry { id, spec: None, title: title.into(), text });
    }

    /// A text entry confirmed.
    pub(super) fn commit_text(&mut self, id: Id, text: &str) {
        let text = text.trim();
        match id {
            // `renameSong`: an empty name keeps the old one.
            Id::SongName if !text.is_empty() => {
                if self.editor.set_library_info("name", text) {
                    self.changed_at = Some(Instant::now());
                }
            }
            Id::SongDescription => {
                if self.editor.set_library_info("description", text) {
                    self.changed_at = Some(Instant::now());
                }
            }
            Id::SongPattern(p) => {
                let before = self.editor.library().patterns.get(p).and_then(|p| p.name.clone());
                if !text.is_empty() && before.as_deref() != Some(text) && self.editor.rename_pattern(p, text) {
                    self.edited();
                }
            }
            Id::PatternDescription => {
                let changed = self.pattern().is_some_and(|p| p.description != text);
                if changed && self.editor.set_pattern_description(self.pattern_index, text) {
                    self.edited();
                }
            }
            // An empty or unchanged name would only add an undo step.
            Id::TrackName(t) => {
                let changed = self.track(t).is_some_and(|track| !text.is_empty() && track.name != text);
                if changed && self.editor.change_track_name(self.pattern_index, t, text) {
                    self.edited();
                    self.message = format!("Renamed track to \"{text}\"");
                }
            }
            Id::SongButton("new") => self.add_song(text),
            _ => {}
        }
    }

    /// The arrangement select: arrangement `index`.
    /// A song playing switches to it, from the cursor.
    pub(super) fn select_song(&mut self, index: usize) {
        let playing = self.song_playing();
        let selected = self.editor.set_selected_song(index);
        self.song_scroll = (0, 0);
        if playing && let Some(controller) = &mut self.controller {
            controller.play_from(self.editor.library(), Mode::Song(selected), u64::from(self.song_cursor));
            self.played_revision = self.editor.revision();
        }
        if let Some(song) = self.song() {
            self.message = format!("song {} ({} measures)", song.name, song.length_measures());
        }
    }

    fn add_song(&mut self, name: &str) {
        self.editor.add_song(name);
        self.song_scroll = (0, 0);
        self.edited();
    }

    fn remove_song(&mut self) {
        if self.song_playing()
            && let Some(controller) = &mut self.controller
        {
            controller.stop();
        }
        if self.editor.remove_song(None) {
            self.song_scroll = (0, 0);
            self.edited();
        } else {
            self.message = "no arrangement to remove".into();
        }
    }

    // ─── The cursor ──────────────────────────────────────────────────────────

    /// Aim the cursor at `measure`: play starts there, and
    /// a song playing goes there at once.
    fn set_song_cursor(&mut self, measure: u32) {
        self.song_cursor = measure;
        if self.song_playing()
            && let Some(controller) = &mut self.controller
        {
            controller.jump(self.editor.library(), u64::from(measure));
        }
    }

    /// ← →: the cursor a measure back or on, inside the arrangement.
    pub fn move_song_cursor(&mut self, delta: i64) {
        let Some(song) = self.song() else { return };
        let last = (song.length_measures().ceil() as i64 - 1).max(0);
        let measure = (self.song_cursor_measure().floor() as i64 + delta).clamp(0, last);
        self.set_song_cursor(measure as u32);
        self.message = format!("cursor at measure {}", measure + 1);
    }

    /// Play the selected song from the cursor (`#applySongCursor` at start).
    pub(super) fn song_start(&self) -> u64 {
        u64::from(self.song_cursor)
    }

    // ─── The mouse ───────────────────────────────────────────────────────────

    /// A click on the song panel's list or arrangement; whether it was theirs.
    pub fn song_click(&mut self, size: (i32, i32, i32), x: i32, y: i32, button: Button) -> bool {
        let layout = self.song_layout(size);
        let rows = self.song_rows();
        let Some(hit) = layout.hit(&self.song_view_data(&rows, &layout), x, y) else { return false };
        match (hit, button) {
            (SongHit::Pattern(p), Button::Left) => {
                let id = Id::SongPattern(p);
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(last, at)| last == id && now - at < DOUBLE_CLICK);
                self.last_click = if double { None } else { Some((id, now)) };
                if double {
                    self.text_entry(id);
                } else {
                    self.step_pattern(p as isize - self.pattern_index as isize);
                }
            }
            (SongHit::Pattern(p), Button::Right) => self.list_menu(p, x, y),
            (SongHit::Ruler(m), Button::Left) => self.set_song_cursor(m),
            (SongHit::Clip(c), Button::Right) => self.clip_menu(c, x, y),
            (SongHit::RowName(r), Button::Right) => self.row_menu(&rows[r], x, y),
            (SongHit::Cell { row, measure }, Button::Right) => {
                let row = &rows[row];
                let title = format!("Add \"{}\" at measure {}", row.name, measure + 1);
                self.dropdown = Some(Dropdown::menu(
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

    /// The wheel over the arrangement: measures (Shift: rows); whether it was its.
    pub fn song_wheel(&mut self, size: (i32, i32, i32), x: i32, y: i32, lines: isize, shift: bool) -> bool {
        let layout = self.song_layout(size);
        let Some(song) = self.song() else { return false };
        if self.workspace != Workspace::Song || lines == 0 || !layout.grid.contains(x, y) {
            return false;
        }
        let rows = self.song_rows().len();
        let (mut sx, mut sy) = self.song_scroll(&layout, rows);
        if shift {
            let last = rows.saturating_sub(layout.visible_rows());
            sy = (sy as isize - lines).clamp(0, last as isize) as usize;
        } else {
            let last = (song.length_measures().max(1.0).ceil() as u32).saturating_sub(layout.visible_measures());
            sx = (i64::from(sx) - 4 * lines as i64).clamp(0, i64::from(last)) as u32;
        }
        self.song_scroll = (sx, sy);
        true
    }

    /// The mouse over the arrangement: the clip it is on, for its tooltip;
    /// whether it changed.
    pub fn song_hover(&mut self, size: (i32, i32, i32), x: i32, y: i32) -> bool {
        let hover = if self.workspace == Workspace::Song && self.drag.is_none() {
            let layout = self.song_layout(size);
            let rows = self.song_rows();
            match layout.hit(&self.song_view_data(&rows, &layout), x, y) {
                Some(SongHit::Clip(c)) => Some(c),
                _ => None,
            }
        } else {
            None
        };
        let changed = hover != self.hovered_clip;
        self.hovered_clip = hover;
        changed
    }

    /// The tooltip of the clip under the mouse (the clip's `title`).
    pub(super) fn draw_song_tooltip(&self, canvas: &mut Canvas, scale: i32) {
        let Some(c) = self.hovered_clip else { return };
        if self.menu.is_some() || self.popup_open() || self.overlay.is_some() {
            return;
        }
        let Some(clip) = self.song().and_then(|s| s.clips.get(c)) else { return };
        let layout = self.song_layout((canvas.width(), canvas.height(), scale));
        let rows = self.song_rows();
        let Some(rect) = layout.clip_rect(&self.song_view_data(&rows, &layout), c) else { return };
        view::draw_tooltip(canvas, &clip_label(&clip.pattern_id, clip), rect, scale);
    }

    // ─── The menus ───────────────────────────────────────────────────────────

    /// A pattern of the list: add it at the playhead.
    fn list_menu(&mut self, p: usize, x: i32, y: i32) {
        if self.song().is_none() {
            self.message = "No song to add to".into();
            return;
        }
        let Some(pattern) = self.editor.library().patterns.get(p) else { return };
        let label = pattern.name.clone().unwrap_or_else(|| pattern.id.clone());
        let measure = self.playhead_measure();
        let values = vec![format!("add:{measure}:{}", pattern.id)];
        let labels = vec![format!("Add at measure {}", measure + 1)];
        self.dropdown = Some(Dropdown::menu(Id::SongMenu, x, y, label, values, labels));
    }

    /// A clip: repeat it after itself, or remove it.
    fn clip_menu(&mut self, c: usize, x: i32, y: i32) {
        let Some(clip) = self.song().and_then(|s| s.clips.get(c)).cloned() else { return };
        let title = clip_label(&self.pattern_label(&clip.pattern_id), &clip);
        let values = vec![format!("next:{c}"), format!("delete:{c}")];
        self.dropdown = Some(Dropdown::menu(Id::SongMenu, x, y, title, values, vec!["Next".into(), "Delete".into()]));
    }

    /// A row's name: add its pattern at the playhead, or remove every clip
    /// of it.
    fn row_menu(&mut self, row: &Row, x: i32, y: i32) {
        let count = self.song().map_or(0, |s| s.clips.iter().filter(|c| c.pattern_id == row.id).count());
        let measure = self.playhead_measure();
        let title = format!("{} - {count} clip(s)", row.name);
        let values = vec![format!("add:{measure}:{}", row.id), format!("row:{}", row.id)];
        let labels = vec![format!("Add at measure {}", measure + 1), "Delete row".into()];
        self.dropdown = Some(Dropdown::menu(Id::SongMenu, x, y, title, values, labels));
    }

    /// The name a clip's pattern is shown by (its id when it is gone).
    fn pattern_label(&self, id: &str) -> String {
        self.editor.library().pattern(id).and_then(|p| p.name.clone()).unwrap_or_else(|| id.to_owned())
    }

    /// An action of a song panel menu (see the menus above, and Load).
    pub(super) fn song_menu_pick(&mut self, value: &str) {
        let mut parts = value.splitn(3, ':');
        let (action, a, b) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next());
        let result = match (action, b) {
            ("add", Some(id)) => {
                let measure = a.parse().unwrap_or(0);
                let label = self.pattern_label(id);
                self.editor
                    .add_pattern_at_measure(id, measure, None, None)
                    .map(|_| format!("\"{label}\" added at measure {}", measure + 1))
            }
            ("next", None) => {
                let Some(clip) = a.parse().ok().and_then(|c: usize| self.song()?.clips.get(c).cloned()) else { return };
                let label = self.pattern_label(&clip.pattern_id);
                self.editor.repeat_pattern_at_measure(clip.start_measure, None).map(|_| {
                    let at = f64::from(clip.start_measure) + clip.measure_count + 1.0;
                    format!("\"{label}\" repeated at measure {}", js::number_to_string(at))
                })
            }
            ("delete", None) => {
                let Some((c, clip)) =
                    a.parse().ok().and_then(|c: usize| Some((c, self.song()?.clips.get(c).cloned()?)))
                else {
                    return;
                };
                let label = self.pattern_label(&clip.pattern_id);
                self.editor.remove_song_clips(None, &[c]);
                Ok(format!("Removed \"{label}\" at measure {}", clip.start_measure + 1))
            }
            ("row", None) => {
                let label = self.pattern_label(a);
                self.editor.remove_pattern_clips(a, None).map(|_| format!("Removed \"{label}\" from the arrangement"))
            }
            ("load", None) => return self.load_song_file(Path::new(a)),
            _ => return,
        };
        match result {
            Ok(message) => {
                self.edited();
                self.message = message;
            }
            Err(message) => self.message = message,
        }
    }

    // ─── Song files ──────────────────────────────────────────────────────────

    /// The song file of the library: its patterns, its arrangements, what is selected.
    fn song_record(&self, stamp: &str) -> Value {
        let library = self.editor.library();
        library.song_record(&self.song_name(), self.pattern_index, self.editor.selected_song(), (stamp, now_ms()))
    }

    /// Save: the song to `songs/<name>.odbox` (`SongService.save`).
    fn save_song(&mut self) {
        let Some(dir) = store::songs_dir() else {
            self.message = "Save failed: no user data folder".into();
            return;
        };
        let name = self.song_name();
        let path = dir.join(format!("{}.odbox", store::safe_name(&name)));
        self.message = match store::save_song(&path, &self.song_record("savedAt")) {
            Ok(()) => format!("Song \"{name}\" saved"),
            Err(e) => format!("Save failed: {}: {e}", path.display()),
        };
    }

    /// Load: the songs saved, to choose from.
    fn load_song_menu(&mut self) {
        let Some(dir) = store::songs_dir() else {
            self.message = "No user data folder: no saved songs".into();
            return;
        };
        let files = match store::list_songs(&dir) {
            Ok(files) => files,
            Err(e) => {
                self.message = format!("Songs not listed: {}: {e}", dir.display());
                return;
            }
        };
        if files.is_empty() {
            self.message = "No saved songs found".into();
            return;
        }
        let labels = files.iter().map(|f| f.file_stem().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let values = files.iter().map(|f| format!("load:{}", f.display())).collect();
        let anchor = self.song_button_rect("load");
        self.dropdown = Some(Dropdown {
            title: Some("Load Song".into()),
            ..Dropdown::new(Id::SongMenu, anchor, values, labels, 0)
        });
    }

    /// Where a column button is (the Load list opens under it); without the
    /// song panel (the Song menu), the middle of the window under the header.
    fn song_button_rect(&self, id: &'static str) -> Rect {
        let (width, height, scale) = self.panel_size;
        let layout = self.panel_layout(width, height, scale);
        panel::find(&layout.items, Id::SongButton(id))
            .map_or_else(|| Rect::new(width / 2 - 100 * scale, frame::body_top(scale), 200 * scale, 0), |i| i.rect)
    }

    /// The file a song is written to (`<name>.odbox`).
    pub(super) fn song_file_name(&self) -> String {
        format!("{}.odbox", store::safe_name(&self.song_name()))
    }

    /// Save as: the song to `path`.
    pub(super) fn save_song_as(&mut self, path: &Path) {
        let name = self.song_name();
        self.message = match store::save_song(path, &self.song_record("savedAt")) {
            Ok(()) => format!("Song \"{name}\" saved to {}", path.display()),
            Err(e) => format!("Save failed: {}: {e}", path.display()),
        };
    }

    /// Export: the song to `path`.
    pub(super) fn export_song(&mut self, path: &Path) {
        let name = self.song_name();
        self.message = match store::save_song(path, &self.song_record("exportedAt")) {
            Ok(()) => format!("Song \"{name}\" exported to {}", path.display()),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// A saved song chosen in the Load list.
    fn load_song_file(&mut self, path: &Path) {
        match self.read_song_file(path) {
            Ok(name) => self.message = format!("Song \"{name}\" loaded"),
            Err(e) => self.message = e,
        }
    }

    /// Import (the file picker), or a `.odbox` dropped on the window.
    pub(super) fn import_song(&mut self, path: &Path) {
        match self.read_song_file(path) {
            Ok(name) => self.message = format!("Song \"{name}\" imported"),
            Err(e) => self.message = e,
        }
    }

    /// Read a song file and put it in the place of the library
    /// (`parseImportedFile`, `applyToAppState`: one undo step); its name.
    fn read_song_file(&mut self, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Load failed: {}: {e}", path.display()))?;
        let value = json::parse(&text).map_err(|e| format!("Load failed: {e}"))?;
        let infos = &self.editor.library().info;
        let record = ordrumbox_core::Library::from_song_record(&value, infos).ok_or("Invalid song file")?;
        let fallback = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let name = record.name.clone().unwrap_or(fallback);
        let mut library = record.library;
        library.info.insert("name", Value::from(name.as_str()));
        self.editor.load_library(library, record.selected_song);
        self.pattern_index = record.selected_pattern;
        self.song_scroll = (0, 0);
        self.after_history();
        let dropped = &record.report.dropped_clips;
        if !dropped.is_empty() {
            let names: Vec<&str> = dropped.iter().map(|d| d.pattern.as_str()).collect();
            self.status.push(format!(
                "Ignored {} song clip(s) referencing an unknown pattern: {}",
                dropped.len(),
                names.join(", ")
            ));
        }
        Ok(name)
    }
}

/// A clip's description (its `title`): `PATTERN - measure N, M measure(s)`.
fn clip_label(label: &str, clip: &SongClip) -> String {
    format!("{label} - measure {}, {} measure(s)", clip.start_measure + 1, js::number_to_string(clip.measure_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use ordrumbox_core::Library;

    const SIZE: (i32, i32, i32) = (1100, 680, 1);

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"infos":{"name":"lib"},"songs":[{"name":"s","clips":[{"pattern":"b","startMeasure":0,"measureCount":2}]}],
                "patterns":[{"name":"a","beatCount":3},{"name":"b"}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.toggle_song_view();
        session
    }

    fn layout() -> SongLayout {
        SongLayout::new(SIZE.0, SIZE.1, SIZE.2, 0)
    }

    /// The menu open: its values.
    fn menu(session: &Session) -> Vec<String> {
        session.dropdown.as_ref().map(|d| d.values.clone()).unwrap_or_default()
    }

    /// Option `i` of the open menu chosen with the keys.
    fn choose(session: &mut Session, i: usize) {
        for _ in 0..i {
            session.panel_key(SIZE, PanelKey::Down, false, false);
        }
        session.panel_key(SIZE, PanelKey::Enter, false, false);
    }

    /// `text` typed in the open entry, then Enter.
    fn type_entry(session: &mut Session, text: &str) {
        session.entry.as_mut().expect("an entry").text = text.into();
        session.panel_key(SIZE, PanelKey::Enter, false, false);
    }

    fn clips(session: &Session) -> Vec<(String, u32, f64)> {
        let song = session.song().unwrap();
        song.clips.iter().map(|c| (c.pattern_id.clone(), c.start_measure, c.measure_count)).collect()
    }

    #[test]
    fn clip_labels() {
        let clip = SongClip { pattern_id: "rock".into(), start_measure: 2, measure_count: 0.75 };
        assert_eq!(clip_label("Rock", &clip), "Rock - measure 3, 0.75 measure(s)");
    }

    #[test]
    fn the_mouse_places_repeats_and_removes_clips() {
        let mut s = session();
        let l = layout();
        // The ruler aims the cursor; a pattern of the list is added there.
        let ruler_y = l.grid.y + 14 + 4;
        let measure_x = |m: i32| l.grid.x + 74 + m * 24 + 12;
        assert!(s.song_click(SIZE, measure_x(1), ruler_y, Button::Left));
        assert_eq!(s.song_cursor, 1);
        assert!(s.song_click(SIZE, l.list.x + 2, l.list.y + 2, Button::Right));
        assert_eq!(menu(&s), ["add:1:a"]);
        choose(&mut s, 0);
        assert_eq!(s.message, "\"a\" added at measure 2");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0), ("a".into(), 1, 0.75)]);
        // Rows: a, b (library order). The clip of a: repeat it.
        let row_y = |r: i32| l.grid.y + 14 + 18 + r * 22 + 11;
        assert!(s.song_click(SIZE, measure_x(1) - 4, row_y(0), Button::Right));
        assert_eq!(menu(&s), ["next:1", "delete:1"]);
        choose(&mut s, 0);
        assert_eq!(s.message, "\"a\" repeated at measure 2.75");
        assert_eq!(clips(&s)[2], ("a".into(), 1, 0.75));
        // The bare grid of row a, measure 0: add a there.
        assert!(s.song_click(SIZE, measure_x(0), row_y(0), Button::Right));
        assert_eq!(menu(&s), ["add:0:a"]);
        choose(&mut s, 0);
        assert_eq!(clips(&s)[3], ("a".into(), 0, 0.75));
        // Nothing past the arrangement's 2 measures.
        assert!(!s.song_click(SIZE, measure_x(2), row_y(1), Button::Right));
        // Row a's name: delete the row.
        assert!(s.song_click(SIZE, l.grid.x + 2, row_y(0), Button::Right));
        assert_eq!(menu(&s), ["add:1:a", "row:a"]);
        choose(&mut s, 1);
        assert_eq!(s.message, "Removed \"a\" from the arrangement");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
        s.undo();
        assert_eq!(clips(&s).len(), 4);
    }

    #[test]
    fn names_descriptions_and_arrangements() {
        let mut s = session();
        assert_eq!(s.song_name(), "lib");
        s.commit_text(Id::SongName, "   ");
        assert_eq!(s.song_name(), "lib");
        s.commit_text(Id::SongName, "  My song ");
        s.commit_text(Id::SongDescription, " about ");
        assert_eq!((s.song_name().as_str(), s.info("description").as_str()), ("My song", "about"));
        // A double-click on a pattern of the list renames it.
        let l = layout();
        let row = (l.list.x + 2, l.list.y + 12 + 2);
        s.song_click(SIZE, row.0, row.1, Button::Left);
        assert_eq!(s.pattern_index, 1);
        s.song_click(SIZE, row.0, row.1, Button::Left);
        assert_eq!(s.entry.as_ref().map(|e| (e.id, e.text.as_str())), Some((Id::SongPattern(1), "b")));
        type_entry(&mut s, "B two");
        assert_eq!(s.editor.library().patterns[1].name.as_deref(), Some("B two"));
        // New and Delete.
        s.song_button("new");
        type_entry(&mut s, "Second");
        assert_eq!((s.editor.selected_song(), s.song().unwrap().name.as_str()), (1, "Second"));
        s.select_song(0);
        s.song_button("delete");
        assert_eq!(s.editor.library().songs.len(), 1);
        assert_eq!(s.song().unwrap().name, "Second");
        s.undo();
        assert_eq!((s.editor.library().songs.len(), s.editor.selected_song()), (2, 0));
    }

    #[test]
    fn the_menus_reach_the_song_panel_commands() {
        let mut s = session();
        s.toggle_song_view(); // back to the grid
        s.set_size(SIZE);
        // The Load list opens under the header, not in a corner.
        let anchor = s.song_button_rect("load");
        assert!(anchor.x > 100 && anchor.y > 0, "{anchor:?}");
        s.toggle_song_view();
        assert_eq!(
            s.song_button_rect("load"),
            panel::find(&s.panel_layout(SIZE.0, SIZE.1, 1).items, Id::SongButton("load")).unwrap().rect
        );
        s.run(Command::RenameSong);
        assert_eq!(s.entry.as_ref().map(|e| e.id), Some(Id::SongName));
        type_entry(&mut s, "From the menu");
        assert_eq!(s.song_name(), "From the menu");
        s.run(Command::NewSong);
        type_entry(&mut s, "Third");
        s.run(Command::PreviousSong);
        assert_eq!(s.editor.selected_song(), 0);
    }

    #[test]
    fn save_as_and_export_write_to_the_folder_picked() {
        let mut s = session();
        let dir = std::env::temp_dir().join(format!("ordrumbox-song-destination-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        s.last_dir = Some(dir.clone());
        let name = store::safe_name(&s.song_name());
        let path = dir.join(format!("{name}.odbox"));

        // Escape writes nothing.
        s.song_button("saveas");
        assert!(s.overlay_open());
        s.overlay_key(OverlayKey::Escape);
        assert!(!path.exists());

        for (id, stamp) in [("saveas", "savedAt"), ("export", "exportedAt")] {
            s.song_button(id);
            s.overlay_key(OverlayKey::Enter);
            assert!(!s.overlay_open());
            assert!(s.message.contains(&path.display().to_string()), "{}", s.message);
            assert!(std::fs::read_to_string(&path).unwrap().contains(stamp), "{id}");
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacing_a_file_is_confirmed() {
        let mut s = session();
        let dir = std::env::temp_dir().join(format!("ordrumbox-song-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        s.last_dir = Some(dir.clone());
        let path = dir.join(s.song_file_name());
        std::fs::write(&path, "old").unwrap();

        // Escape keeps the file.
        s.song_button("export");
        s.overlay_key(OverlayKey::Enter);
        assert!(s.overlay_open());
        s.overlay_key(OverlayKey::Escape);
        assert!(!s.overlay_open());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");

        // Enter replaces it.
        s.song_button("export");
        s.overlay_key(OverlayKey::Enter);
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
        assert!(std::fs::read_to_string(&path).unwrap().contains("exportedAt"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn song_files_load_as_one_undo_step() {
        let mut s = session();
        let dir = std::env::temp_dir().join(format!("ordrumbox-song-files-{}", std::process::id()));
        let path = dir.join("lib.odbox");
        store::save_song(&path, &s.song_record("savedAt")).unwrap();
        s.editor.add_pattern_at_measure("a", 5, None, None).unwrap();
        s.commit_text(Id::SongName, "changed");
        s.load_song_file(&path);
        assert_eq!(s.message, "Song \"lib\" loaded");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
        assert_eq!(s.editor.history().next_undo(), Some("Load song"));
        s.undo();
        assert_eq!(clips(&s).len(), 2);
        assert_eq!(s.song_name(), "changed");
        std::fs::write(&path, r#"{"songs":[]}"#).unwrap();
        s.import_song(&path);
        assert_eq!(s.message, "Invalid song file");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
