//! The pattern grid's right-click menus (`context_menu_section.js`): a
//! track's name or buttons open the track menu, a step the notes menu. What
//! cannot run (paste with nothing copied, delete on an empty step) is left out.
//! A track's name dragged up or down moves the track to another row.

use ordrumbox_core::js;

use super::session_song::DRAG_START_PX;
use super::*;

/// A press on a track's name that may become a drag. Nothing is edited before
/// the release, so the move is one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TrackDrag {
    track: usize,
    press: (i32, i32),
    y: i32,
    /// The pointer has really moved: this is no longer a click.
    started: bool,
    /// What it was pressed on; another edit cancels the drag.
    revision: u64,
}

/// Where item `t` of a list is once the item `from` moved to `to`.
pub(super) fn moved_index(t: usize, from: usize, to: usize) -> usize {
    if t == from {
        to
    } else if from < t && t <= to {
        t - 1
    } else if to <= t && t < from {
        t + 1
    } else {
        t
    }
}

impl Session {
    /// The grid's layout, when the grid is shown.
    fn grid_layout(&self, size: (i32, i32, i32)) -> Option<view::Layout> {
        if self.workspace != Workspace::Grid || self.kit_editor.is_some() {
            return None;
        }
        let body = self.body(size);
        let pattern = self.pattern()?;
        Some(view::Layout::new(
            body.main_w,
            body.editor_h,
            size.2,
            pattern,
            self.status.len(),
            self.chrome.current_page,
        ))
    }

    /// What of the grid is at `(x, y)`.
    pub fn grid_hit(&self, size: (i32, i32, i32), x: i32, y: i32) -> Option<Hit> {
        self.grid_layout(size)?.hit(self.pattern()?, x, y)
    }

    /// A right-click on the grid opens its menu; whether it was the grid's.
    pub fn grid_menu(&mut self, size: (i32, i32, i32), x: i32, y: i32) -> bool {
        match self.grid_hit(size, x, y) {
            Some(Hit::Mute(t) | Hit::Solo(t) | Hit::Name(t)) => self.track_menu(t, x, y),
            Some(Hit::Cell { track, beat, step }) => self.step_menu(track, beat, step, x, y),
            Some(Hit::AddTrack) | None => return false,
        }
        true
    }

    /// A track: rename, copy, paste after it, move up or down, duplicate,
    /// delete, randomize, clear.
    fn track_menu(&mut self, t: usize, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let title = track.name.clone();
        let count = self.pattern().map_or(0, |p| p.tracks.len());
        let mut items = vec![("rename", "Rename track"), ("copy", "Copy track")];
        if matches!(self.selection.clipboard, Some(Clipboard::Track(_))) {
            items.push(("paste", "Paste track"));
        }
        if t > 0 {
            items.push(("up", "Move up"));
        }
        if t + 1 < count {
            items.push(("down", "Move down"));
        }
        items.extend([
            ("duplicate", "Duplicate track"),
            ("delete", "Delete track"),
            ("randomize", "Randomize"),
            ("clear", "Clear notes"),
        ]);
        let values = items.iter().map(|(action, _)| format!("{action}:{t}")).collect();
        let labels = items.iter().map(|(_, label)| (*label).to_owned()).collect();
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::GridMenu, x, y, title, values, labels));
    }

    /// A step: copy its notes, paste on it, delete them, add a random note.
    fn step_menu(&mut self, t: usize, beat: i32, step: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let title = format!("{} @ {}.{}", track.name, beat + 1, step + 1);
        let has_notes = track.notes.iter().any(|n| n.beat == beat && n.step == step);
        let mut items = vec![("copy-notes", "Copy notes")];
        if matches!(&self.selection.clipboard, Some(Clipboard::Step(notes)) if !notes.is_empty()) {
            items.push(("paste-notes", "Paste notes"));
        }
        if has_notes {
            items.push(("delete-notes", "Delete notes"));
        }
        items.push(("random-note", "Add rnd note"));
        let values = items.iter().map(|(action, _)| format!("{action}:{t}:{beat}:{step}")).collect();
        let labels = items.iter().map(|(_, label)| (*label).to_owned()).collect();
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::GridMenu, x, y, title, values, labels));
    }

    /// An action of a grid menu: the cursor goes to its track or step, and
    /// the action runs as its key would there.
    pub(super) fn grid_menu_pick(&mut self, value: &str) {
        let mut parts = value.split(':');
        let action = parts.next().unwrap_or_default();
        let mut number = || parts.next().and_then(|v| v.parse::<i32>().ok());
        let Some(t) = number().and_then(|t| usize::try_from(t).ok()) else { return };
        if self.track(t).is_none() {
            return;
        }
        let (beat, step) = (number().unwrap_or(0), number().unwrap_or(0));
        self.selection.cursor = Some(Cursor { track: t, beat, step });
        match action {
            "rename" => self.text_entry(Id::TrackName(t)),
            "copy" => self.copy(true),
            "paste" => self.paste(true),
            "up" => self.move_track(t, t.saturating_sub(1)),
            "down" => self.move_track(t, t + 1),
            "duplicate" => self.duplicate_track(t),
            "delete" => self.remove_track(),
            "randomize" => self.randomize_track(),
            "clear" => self.clear_track(t),
            "copy-notes" => self.copy(false),
            "paste-notes" => self.paste(false),
            "delete-notes" => self.delete_step(),
            "random-note" => self.add_random_note(t, beat, step),
            _ => {}
        }
    }

    /// Move track `from` to row `to`; the cursor and the selection follow
    /// the tracks they were on.
    pub(super) fn move_track(&mut self, from: usize, to: usize) {
        if !self.editor.move_track(self.pattern_index, from, to) {
            return;
        }
        let Some(to) = self.pattern().map(|p| to.min(p.tracks.len() - 1)) else { return };
        self.selection.follow_tracks(|t| moved_index(t, from, to));
        self.chrome.hover = None;
        self.edited();
    }

    // ─── Dragging a track ────────────────────────────────────────────────────

    /// A press on the name of track `t`.
    pub fn press_track_drag(&mut self, t: usize, x: i32, y: i32) {
        self.chrome.track_drag =
            Some(TrackDrag { track: t, press: (x, y), y, started: false, revision: self.editor.revision() });
    }

    /// The drag under way, unless the pattern may have changed under it.
    fn live_track_drag(&self) -> Option<TrackDrag> {
        self.chrome.track_drag.filter(|d| d.started && d.revision == self.editor.revision())
    }

    /// The track dragged and the row it would take.
    pub(super) fn track_drop(&self, size: (i32, i32, i32)) -> Option<(usize, usize)> {
        let drag = self.live_track_drag()?;
        Some((drag.track, self.grid_layout(size)?.drop_row(drag.y)))
    }

    /// The mouse moved with a name pressed; whether to redraw.
    pub(super) fn track_drag_move(&mut self, x: i32, y: i32) -> bool {
        let Some(drag) = &mut self.chrome.track_drag else { return false };
        drag.y = y;
        let (dx, dy) = (x - drag.press.0, y - drag.press.1);
        drag.started |= dx.abs() >= DRAG_START_PX || dy.abs() >= DRAG_START_PX;
        drag.started
    }

    /// The press released: a drag moves its track; whether it was one.
    pub fn track_release(&mut self, size: (i32, i32, i32)) -> bool {
        let drop = self.track_drop(size);
        self.chrome.track_drag = None;
        let Some((from, to)) = drop else { return false };
        self.move_track(from, to);
        true
    }

    /// A copy of track `t` right after it.
    fn duplicate_track(&mut self, t: usize) {
        let p = self.pattern_index;
        if !self.editor.has_track_room(p) {
            self.message = format!("a pattern holds at most {MAX_TRACKS} tracks");
            return;
        }
        let Some(source) = self.track(t).cloned() else { return };
        if let Some(copy) = self.editor.paste_track(p, t + 1, &source) {
            self.selection.put_cursor(Cursor { track: copy, beat: 0, step: 0 });
            self.edited();
            let name = self.track(copy).map(|track| track.name.clone()).unwrap_or_default();
            self.message = format!("Duplicated track as \"{name}\"");
        }
    }

    /// A note at a random pitch within the track's pitch range, selected.
    fn add_random_note(&mut self, t: usize, beat: i32, step: i32) {
        let Some(track) = self.track(t) else { return };
        let (name, range) = (track.name.clone(), track.pitch_range.max(1.0));
        let pitch = (self.rng.next_f64() * (range * 2.0 + 1.0)).floor() - range;
        let Some(n) = self.editor.add_note(self.pattern_index, t, beat, step, pitch) else { return };
        self.selection.selected = Some((t, n));
        self.edited();
        self.audition(t, Some(n));
        self.message =
            format!("Added note (pitch {}) - {name} @ {}", js::number_to_string(pitch), beat_label(beat, step));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;

    const SIZE: (i32, i32, i32) = (1100, 680, 1);

    fn session() -> Session {
        let (library, _) =
            Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{}]},{"name":"SNARE"}]}]}"#)
                .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session
    }

    #[test]
    fn the_history_panel_scrolls_and_closes() {
        let mut s = session();
        for i in 0..200 {
            s.message = format!("action {i}");
            s.note_actions();
        }
        s.open_history();
        let scroll = |s: &Session| match s.chrome.overlay {
            Some(Overlay::History { scroll }) => scroll,
            _ => panic!("the history panel is open"),
        };
        s.overlay_key(OverlayKey::Move(-3));
        assert_eq!(scroll(&s), 0);
        s.overlay_key(OverlayKey::Move(3));
        assert_eq!(scroll(&s), 3);
        s.overlay_key(OverlayKey::Edge(true));
        let last = scroll(&s);
        assert!(last > 0);
        s.overlay_key(OverlayKey::Move(1));
        assert_eq!(scroll(&s), last);
        s.overlay_click(SIZE.0, SIZE.1, SIZE.2, SIZE.0 / 2, SIZE.1 / 2);
        assert!(s.overlay_open(), "a click inside keeps it open");
        s.overlay_click(SIZE.0, SIZE.1, SIZE.2, 1, 1);
        assert!(!s.overlay_open());
    }

    #[test]
    fn no_control_is_on_the_status_line() {
        let s = session();
        let (width, height, scale) = SIZE;
        let layout = s.panel_layout(width, height, scale);
        let y = crate::frame::status_line(scale);
        for y in y..y + 10 * scale {
            assert!(crate::frame::on_status_line(y, scale));
            for x in 0..width {
                assert!(crate::panel::hit(&layout.items, x, y).is_none(), "a control at {x}, {y}");
            }
        }
    }

    /// The menu open, its labels.
    fn labels(s: &Session) -> Vec<String> {
        s.chrome.widgets.dropdown.as_ref().map(|d| d.labels.clone()).unwrap_or_default()
    }

    /// Run the menu's action labelled `label`.
    fn pick(s: &mut Session, label: &str) {
        let d = s.chrome.widgets.dropdown.take().expect("a menu");
        let i = d.labels.iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", d.labels));
        s.grid_menu_pick(&d.values[i]);
    }

    #[test]
    fn the_track_menu_copies_duplicates_and_deletes() {
        let mut s = session();
        let count = s.pattern().unwrap().tracks.len();
        s.track_menu(0, 10, 10);
        assert!(!labels(&s).contains(&"Paste track".to_owned()), "nothing copied yet");
        pick(&mut s, "Duplicate track");
        assert_eq!(s.pattern().unwrap().tracks.len(), count + 1);
        assert!(s.message.starts_with("Duplicated track as"), "{}", s.message);
        s.track_menu(0, 10, 10);
        pick(&mut s, "Copy track");
        s.track_menu(0, 10, 10);
        pick(&mut s, "Paste track");
        assert_eq!(s.pattern().unwrap().tracks.len(), count + 2);
        s.track_menu(1, 10, 10);
        pick(&mut s, "Delete track");
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(s.pattern().unwrap().tracks.len(), count + 1);
    }

    #[test]
    fn the_track_menu_moves_up_and_down() {
        let mut s = session();
        let names = |s: &Session| s.pattern().unwrap().tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        s.track_menu(0, 10, 10);
        assert!(!labels(&s).contains(&"Move up".to_owned()), "the first track goes no higher");
        s.selection.cursor = Some(Cursor { track: 0, beat: 1, step: 0 });
        pick(&mut s, "Move down");
        assert_eq!(names(&s), ["SNARE", "KICK"]);
        assert_eq!(s.selection.cursor.map(|c| c.track), Some(1), "the cursor follows the track");
        assert_eq!(s.message, "Move track KICK");
        s.track_menu(1, 10, 10);
        assert!(!labels(&s).contains(&"Move down".to_owned()), "the last track goes no lower");
        pick(&mut s, "Move up");
        assert_eq!(names(&s), ["KICK", "SNARE"]);
    }

    #[test]
    fn the_button_under_the_rows_adds_a_track() {
        let mut s = session();
        let before = s.pattern().unwrap().tracks.len();
        let rect = s.grid_layout(SIZE).expect("the grid is shown").add_track_rect();
        let last = s.grid_layout(SIZE).unwrap().row_top(before - 1);
        assert!(rect.y > last, "under the last row");
        let hit = s.grid_hit(SIZE, rect.x + rect.w / 2, rect.y + rect.h / 2);
        assert_eq!(hit, Some(Hit::AddTrack));
        s.click(Hit::AddTrack);
        assert_eq!(s.pattern().unwrap().tracks.len(), before + 1);
        assert_eq!(s.selection.cursor.map(|c| c.track), Some(before), "the cursor goes to the new track");
        let rect = s.grid_layout(SIZE).unwrap().add_track_rect();
        assert_eq!(s.grid_hit(SIZE, rect.x + 1, rect.y + 1), Some(Hit::AddTrack), "it moves down with the rows");
    }

    #[test]
    fn a_track_dragged_by_its_name_moves() {
        let mut s = session();
        let layout = s.grid_layout(SIZE).expect("the grid is shown");
        let pattern = s.pattern().unwrap().clone();
        let at = |t: usize| {
            (0..SIZE.1)
                .flat_map(|y| (0..SIZE.0).map(move |x| (x, y)))
                .find(|&(x, y)| layout.hit(&pattern, x, y) == Some(Hit::Name(t)))
                .expect("a track name")
        };
        let ((x0, y0), (x1, y1)) = (at(0), at(1));
        s.click(Hit::Name(0));
        s.press_track_drag(0, x0, y0);
        assert!(!s.track_drag_move(x0, y0 + 1), "a small move is still a click");
        assert!(s.track_drag_move(x1, y1));
        assert_eq!(s.track_drop(SIZE), Some((0, 1)));
        assert!(s.track_release(SIZE));
        let names: Vec<_> = s.pattern().unwrap().tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["SNARE", "KICK"]);
        assert_eq!(s.selection.cursor.map(|c| c.track), Some(1));
        s.press_track_drag(1, x1, y1);
        assert!(!s.track_release(SIZE), "a click moves nothing");
        s.undo();
        assert_eq!(s.pattern().unwrap().tracks[0].name, "KICK");
    }

    #[test]
    fn the_step_menu_adds_copies_pastes_and_deletes() {
        let mut s = session();
        s.editor.clean_track(s.pattern_index, 0);
        s.step_menu(0, 1, 0, 10, 10);
        assert_eq!(labels(&s), ["Copy notes", "Add rnd note"]);
        pick(&mut s, "Add rnd note");
        let note = s.track(0).unwrap().notes[0].clone();
        assert_eq!((note.beat, note.step), (1, 0));
        let range = s.track(0).unwrap().pitch_range.max(1.0);
        assert!(note.pitch.abs() <= range, "{}", note.pitch);
        s.step_menu(0, 1, 0, 10, 10);
        pick(&mut s, "Copy notes");
        s.step_menu(0, 2, 0, 10, 10);
        pick(&mut s, "Paste notes");
        assert_eq!(s.track(0).unwrap().notes.len(), 2);
        s.step_menu(0, 1, 0, 10, 10);
        pick(&mut s, "Delete notes");
        assert_eq!(s.track(0).unwrap().notes.len(), 1);
        assert_eq!(s.message, "Deleted 1 note - KICK @ beat 2.1");
    }

    #[test]
    fn a_right_click_on_a_name_or_a_step_opens_its_menu() {
        let mut s = session();
        let layout = s.grid_layout(SIZE).expect("the grid is shown");
        let pattern = s.pattern().unwrap().clone();
        let (mut name, mut cell) = (None, None);
        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                match layout.hit(&pattern, x, y) {
                    Some(Hit::Name(0)) if name.is_none() => name = Some((x, y)),
                    Some(Hit::Cell { track: 0, .. }) if cell.is_none() => cell = Some((x, y)),
                    _ => {}
                }
            }
        }
        let (x, y) = name.expect("a track name");
        assert!(s.grid_menu(SIZE, x, y));
        assert_eq!(
            s.chrome.widgets.dropdown.as_ref().and_then(|d| d.title.clone()),
            Some(pattern.tracks[0].name.clone())
        );
        let (x, y) = cell.expect("a step");
        assert!(s.grid_menu(SIZE, x, y));
        assert!(labels(&s).contains(&"Add rnd note".to_owned()));
        assert!(!s.grid_menu(SIZE, 0, 0), "the menu bar is not the grid");
    }
}
