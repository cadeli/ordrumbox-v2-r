//! The session's side of the song panel (`song_panel.js` and its sections):
//! the pattern list (a click selects, a double-click adds the pattern at the
//! playhead, a right-click opens its menu: add at the playhead, move up or
//! down, rename, delete; a drag drops it on the arrangement, the wheel or its
//! scroll bar scrolls it; a drag moves it along the list, or drops it on the
//! arrangement at the measure under the pointer), the
//! arrangement (a click on the ruler aims the cursor, where play starts; a
//! clip dragged sideways moves; a row dragged by its name moves up or down;
//! the right-click menus of clips, row names and the bare grid), the selected pattern's description (a click edits it, the
//! wheel scrolls it), the song's column (name, date, description, the
//! arrangement select, Save, Load, Export, Import, and New and Delete for
//! arrangements) — and Space plays the song while the panel is shown.
//!
//! Songs are saved to `songs/` in the user data folder (the JS IndexedDB
//! store) and, with Save as and Export, to a folder picked, as `.odbox` files.
//!
//! The mouse is in `session_song_mouse.rs`, the menus and the edits they
//! lead to in `session_song_menu.rs`, song files in `session_song_files.rs`.

use crate::scale::Scale;
use ordrumbox_core::Song;
use ordrumbox_core::json::Value;
use ordrumbox_core::steps::TICK;

use super::session_panel::PanelLayout;
use super::*;
use crate::panel::{self, Item, Kind, Rect};
use crate::song_view::{self, Row, SongLayout, SongView};
use crate::theme;

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

/// Movement before a press becomes a drag, px.
pub(super) const DRAG_START_PX: i32 = 3;

/// What a drag on the song panel carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DragSource {
    /// A pattern of the list.
    Pattern(usize),
    /// A clip, held `grab` measures after its start.
    Clip { clip: usize, grab: u32 },
    /// A row of the arrangement, held by its name.
    Row(usize),
}

/// A press on the song panel that may become a drag. Nothing is edited before
/// the release, so a drag is one undo step and playback hears only the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SongDrag {
    pub(super) source: DragSource,
    pub(super) press: (i32, i32),
    /// Where the pointer is.
    pub(super) x: i32,
    pub(super) y: i32,
    /// The pointer has really moved: this is no longer a click.
    pub(super) started: bool,
    /// What it was pressed on; another edit or arrangement cancels the drag.
    pub(super) revision: u64,
    pub(super) song: usize,
}

/// The song panel's scrolls and what the mouse holds in it.
#[derive(Debug, Default)]
pub(super) struct SongPanel {
    /// First measure and first row of the arrangement shown.
    pub(super) scroll: (u32, usize),
    /// The pattern whose description is scrolled, and its first line shown.
    pub(super) description_scroll: (usize, usize),
    /// The pattern selected when the pattern list was scrolled, and its first row shown.
    pub(super) list_scroll: (usize, usize),
    /// The clip under the mouse (its tooltip).
    pub(super) hovered_clip: Option<usize>,
    pub(super) drag: Option<SongDrag>,
    /// The arrangement's scroll bar held by the mouse.
    pub(super) bar_drag: Option<crate::song_view::Axis>,
}

impl SongPanel {
    /// A pattern, a clip or a row is held by the mouse.
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The scroll bar held by the mouse, if any.
    pub(super) fn held_bar(&self) -> Option<crate::song_view::Axis> {
        self.bar_drag
    }

    pub(super) fn release_bar(&mut self) {
        self.bar_drag = None;
    }

    /// The arrangement shown from its first measure and row.
    pub(super) fn scroll_to_start(&mut self) {
        self.scroll = (0, 0);
    }
}

/// What is shown for a song without a name.
const UNTITLED: &str = "Untitled";

impl Session {
    // ─── State ───────────────────────────────────────────────────────────────

    /// The song panel is shown (the kit editor opens over it).
    pub fn song_view_open(&self) -> bool {
        self.workspace == Workspace::Song && self.kit_editor.is_none()
    }

    /// F3, View > Song: the song panel in the place of the grid, or back.
    pub fn toggle_song_view(&mut self) {
        if self.hosted {
            self.message = crate::menu::SONGS_IN_THE_APP.into();
            return;
        }
        self.set_workspace(if self.workspace == Workspace::Song { Workspace::Grid } else { Workspace::Song });
    }

    /// Show `workspace`. While playing, a switch to or from the song panel
    /// switches the playback to the view's mode, from its start
    ///; the grid and the piano roll share one mode.
    pub(super) fn set_workspace(&mut self, workspace: Workspace) {
        let was_song = self.workspace == Workspace::Song;
        self.workspace = workspace;
        self.kit_editor = None;
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
        self.song_panel.hovered_clip = None;
        self.song_panel.drag = None;
        self.song_panel.bar_drag = None;
        self.roll.release();
        if self.playing() && was_song != (workspace == Workspace::Song) {
            let mode = self.play_mode();
            self.playback.play(&self.editor, mode);
        }
    }

    /// What Space plays: the song in the song panel, else the pattern.
    fn play_mode(&self) -> Mode {
        match self.workspace {
            Workspace::Song => Mode::Song(self.editor.selected_song()),
            _ => Mode::Pattern(self.pattern_index),
        }
    }

    pub(super) fn song(&self) -> Option<&Song> {
        self.editor.library().songs.get(self.editor.selected_song())
    }

    /// The library header's name, else "Untitled".
    pub(super) fn song_name(&self) -> String {
        let name = self.editor.library().info.get("name").and_then(Value::as_str).unwrap_or_default();
        if name.is_empty() { UNTITLED.to_owned() } else { name.to_owned() }
    }

    pub(super) fn info(&self, key: &str) -> String {
        self.editor.library().info.get(key).and_then(Value::to_text).unwrap_or_default()
    }

    pub(super) fn song_rows(&self) -> Vec<Row> {
        self.song().map(|s| song_view::rows(s, &self.editor.library().patterns)).unwrap_or_default()
    }

    /// The song playing now, if the selected one plays.
    fn song_playing(&self) -> bool {
        self.playback.plays(Mode::Song(self.editor.selected_song()))
    }

    /// Where the cursor is, in measures: where the song
    /// plays, else the measure play starts from; kept on the grid.
    pub(super) fn song_cursor_measure(&self) -> f64 {
        let Some(song) = self.song() else { return 0.0 };
        let measure = match self.playback.tick() {
            Some(tick) if self.song_playing() => song.measure_at_tick(tick as f64, f64::from(TICK)),
            _ => f64::from(self.song_cursor),
        };
        measure.min((song.length_measures() - 1.0).max(0.0))
    }

    /// The measure the playhead is on: where the song
    /// plays, else the cursor. What a pattern is added at from the lists.
    pub(super) fn playhead_measure(&self) -> u32 {
        self.song_cursor_measure().floor() as u32
    }

    /// First measure and first row shown: while the song plays, the page
    /// the cursor is on; else where the wheel left them.
    pub(super) fn song_scroll(&self, layout: &SongLayout, rows: usize) -> (u32, usize) {
        if self.song().is_none() {
            return (0, 0);
        }
        let visible = layout.visible_measures();
        let last = self.song_measure_total(layout).saturating_sub(visible);
        let x = if self.song_playing() {
            (self.song_cursor_measure() as u32 / visible * visible).min(last)
        } else {
            self.song_panel.scroll.0.min(last)
        };
        (x, self.song_panel.scroll.1.min(rows.saturating_sub(layout.visible_rows())))
    }

    /// Measures the arrangement's grid spans: the song, and the room to drop on.
    pub(super) fn song_measure_total(&self, layout: &SongLayout) -> u32 {
        self.song().map_or(0, |song| song.length_measures().max(1.0).ceil() as u32 + self.song_room(layout))
    }

    /// The first line of the selected pattern's description shown, kept so
    /// that the frame is full; another pattern shows from its first line.
    pub(super) fn description_scroll(&self, layout: &SongLayout) -> usize {
        let (p, first) = self.song_panel.description_scroll;
        let Some(pattern) = self.pattern().filter(|_| p == self.pattern_index) else { return 0 };
        first.min(layout.description_lines(&pattern.description).len().saturating_sub(layout.description_rows()))
    }

    /// The pattern list's first row shown: where the wheel or the bar left
    /// it, moved just enough to show a pattern selected since.
    pub(super) fn list_scroll(&self, layout: &SongLayout) -> usize {
        let (p, mut first) = self.song_panel.list_scroll;
        if p != self.pattern_index {
            let selected = self.pattern_index;
            first = first.clamp(selected.saturating_sub(layout.list_rows() - 1), selected);
        }
        first.min(layout.list_last_scroll(self.editor.library().patterns.len()))
    }

    pub(super) fn song_layout(&self, size: (i32, i32, Scale)) -> SongLayout {
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
            list_scroll: self.list_scroll(layout),
            song: self.song(),
            rows,
            cursor: self.song_cursor_measure(),
            playing: self.song_playing(),
            scroll: self.song_scroll(layout, rows.len()),
            dragged_clip: self.song_dragged_clip(layout, rows.len()),
            ghost: self.song_ghost(layout, rows),
            row_drop: self.song_row_drop(layout, rows),
            list_drop: self.song_list_drop(layout),
            room: self.song_room(layout),
            description: self.pattern().map(|p| p.description.as_str()),
            description_scroll: self.description_scroll(layout),
        }
    }

    // ─── The column ──────────────────────────────────────────────────────────

    /// The song's column: name, date, description, arrangement, buttons.
    pub(super) fn song_items(&self, layout: &mut PanelLayout, width: i32, height: i32, scale: Scale) {
        let s = scale;
        let area = self.song_layout((width, height, scale)).column;
        if area.w <= 0 {
            return;
        }
        let (x, label_w, mut y) = (area.x, 56 * s, area.y);
        let line = 16 * s;
        let field = |y: i32| Rect::new(x + label_w, y, area.w - label_w, 12 * s);
        // The name (a click renames it), the date.
        for (label, id, text, color) in [
            ("Song", Some(Id::SongName), self.song_name(), theme::text()),
            ("Date", None, self.info("date"), panel::dim()),
        ] {
            layout.items.push(Item::text(x, y + 2 * s, label, panel::dim(), s));
            layout.items.push(Item::new(id, field(y), Kind::Text { text, color }));
            y += line;
        }
        // The description, a click edits it.
        let description = self.info("description");
        let (text, color) = if description.is_empty() {
            ("(description)".to_owned(), panel::dim())
        } else {
            (description, theme::text())
        };
        let rect = Rect::new(x, y, area.w, 12 * s);
        layout.items.push(Item::new(Some(Id::SongDescription), rect, Kind::Text { text, color }));
        y += line + 4 * s;

        // The arrangement, under its label.
        layout.items.push(Item::text(x, y, "Arrangement", panel::dim(), s));
        y += 12 * s;
        let select = Rect::new(x, y, area.w, 12 * s);
        let songs = &self.editor.library().songs;
        if songs.is_empty() {
            let text = Kind::Text { text: "No arrangement".into(), color: panel::dim() };
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
                self.chrome.widgets.entry = Some(Entry {
                    id: Id::SongButton("new"),
                    spec: None,
                    title: "New arrangement name:".into(),
                    text: String::new(),
                });
            }
            "delete" => {
                if self.song().is_some() {
                    self.undoable(Self::remove_song);
                } else {
                    self.message = "no arrangement to remove".into();
                }
            }
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
        self.chrome.widgets.entry = Some(Entry { id, spec: None, title: title.into(), text });
    }

    /// A text entry confirmed.
    pub(super) fn commit_text(&mut self, id: Id, text: &str) {
        let text = text.trim();
        match id {
            // `renameSong`: an empty name keeps the old one.
            Id::SongName if !text.is_empty() => {
                if self.editor.set_library_info("name", text) {
                    self.saving.library.touch();
                }
            }
            Id::SongDescription => {
                if self.editor.set_library_info("description", text) {
                    self.saving.library.touch();
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
        self.song_panel.scroll = (0, 0);
        if playing {
            self.playback.play_from(&self.editor, Mode::Song(selected), u64::from(self.song_cursor));
        }
        if let Some(song) = self.song() {
            self.message = format!("song {} ({} measures)", song.name, song.length_measures());
        }
    }

    fn add_song(&mut self, name: &str) {
        self.editor.add_song(name);
        self.song_panel.scroll = (0, 0);
        self.edited();
    }

    pub(super) fn remove_song(&mut self) {
        if self.song_playing() {
            self.playback.stop();
        }
        let name = self.song().map(|song| song.name.clone()).unwrap_or_default();
        if self.editor.remove_song(None) {
            self.song_panel.scroll = (0, 0);
            self.edited();
            self.message = format!("Removed the arrangement \"{name}\"");
        } else {
            self.message = "no arrangement to remove".into();
        }
    }

    // ─── The cursor ──────────────────────────────────────────────────────────

    /// Aim the cursor at `measure`: play starts there, and
    /// a song playing goes there at once.
    pub(super) fn set_song_cursor(&mut self, measure: u32) {
        self.song_cursor = measure;
        if self.song_playing()
            && let Some(controller) = self.playback.controller_mut()
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
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::kits::KitSet;
    use ordrumbox_core::Library;

    pub(crate) const SIZE: (i32, i32, Scale) = (1100, 680, Scale::ONE);

    pub(crate) fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"infos":{"name":"lib"},"songs":[{"name":"s","clips":[{"pattern":"b","startMeasure":0,"measureCount":2}]}],
                "patterns":[{"name":"a","beatCount":3},{"name":"b"}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.toggle_song_view();
        session
    }

    /// The menu open: its values.
    pub(crate) fn menu(session: &Session) -> Vec<String> {
        session.chrome.widgets.dropdown.as_ref().map(|d| d.values.clone()).unwrap_or_default()
    }

    /// Option `i` of the open menu chosen with the keys.
    pub(crate) fn choose(session: &mut Session, i: usize) {
        for _ in 0..i {
            session.panel_key(SIZE, PanelKey::Down, false, false);
        }
        session.panel_key(SIZE, PanelKey::Enter, false, false);
    }

    /// `text` typed in the open entry, then Enter.
    pub(crate) fn type_entry(session: &mut Session, text: &str) {
        session.chrome.widgets.entry.as_mut().expect("an entry").text = text.into();
        session.panel_key(SIZE, PanelKey::Enter, false, false);
    }

    pub(crate) fn clips(session: &Session) -> Vec<(String, u32, f64)> {
        let song = session.song().unwrap();
        song.clips.iter().map(|c| (c.pattern_id.clone(), c.start_measure, c.measure_count)).collect()
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
        // A click on the pattern's description edits the selected pattern's.
        let d = s.song_layout(SIZE).description;
        let inside = (d.x + d.w / 2, d.y + d.h / 2);
        assert!(s.song_click(SIZE, inside.0, inside.1, Button::Left));
        type_entry(&mut s, " a groove ");
        assert_eq!(s.editor.library().patterns[s.pattern_index].description, "a groove");
        // The wheel scrolls a long one, no further than its last line.
        s.commit_text(Id::PatternDescription, &"a groove to dance on ".repeat(40));
        assert!(s.song_wheel(SIZE, inside.0, inside.1, (0, -2), false));
        let l = s.song_layout(SIZE);
        assert_eq!(s.song_view_data(&[], &l).description_scroll, 2);
        assert!(s.song_wheel(SIZE, inside.0, inside.1, (0, -1000), false));
        let last = l.description_lines(&s.pattern().unwrap().description).len() - l.description_rows();
        assert_eq!(s.song_view_data(&[], &l).description_scroll, last);
        assert!(s.song_wheel(SIZE, inside.0, inside.1, (0, 1000), false));
        assert_eq!(s.song_view_data(&[], &l).description_scroll, 0);
        // Rename, in the menu of a pattern of the list.
        let l = s.song_layout(SIZE);
        let row = (l.list.x + 2, l.list.y + 12 + 2);
        s.song_click(SIZE, row.0, row.1, Button::Right);
        let rename = menu(&s).iter().position(|v| v == "rename:1").unwrap();
        choose(&mut s, rename);
        assert_eq!(s.chrome.widgets.entry.as_ref().map(|e| (e.id, e.text.as_str())), Some((Id::SongPattern(1), "b")));
        type_entry(&mut s, "B two");
        assert_eq!(s.editor.library().patterns[1].name.as_deref(), Some("B two"));
        // New and Delete.
        s.song_button("new");
        type_entry(&mut s, "Second");
        assert_eq!((s.editor.selected_song(), s.song().unwrap().name.as_str()), (1, "Second"));
        s.select_song(0);
        s.song_button("delete");
        assert_eq!(s.message, "Removed the arrangement \"s\" (Ctrl+Z: undo)");
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
            panel::find(&s.panel_layout(SIZE.0, SIZE.1, Scale::ONE).items, Id::SongButton("load")).unwrap().rect
        );
        s.run(Command::RenameSong);
        assert_eq!(s.chrome.widgets.entry.as_ref().map(|e| e.id), Some(Id::SongName));
        type_entry(&mut s, "From the menu");
        assert_eq!(s.song_name(), "From the menu");
        s.run(Command::NewSong);
        type_entry(&mut s, "Third");
        s.run(Command::PreviousSong);
        assert_eq!(s.editor.selected_song(), 0);
    }
}
