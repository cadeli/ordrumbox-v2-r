//! The pattern grid's right-click menus (`context_menu_section.js`): a
//! track's name or buttons open the track menu, a step the notes menu. What
//! cannot run (paste with nothing copied, delete on an empty step) is left out.

use ordrumbox_core::js;

use super::*;

impl Session {
    /// The grid's layout, when the grid is shown.
    fn grid_layout(&self, size: (i32, i32, i32)) -> Option<view::Layout> {
        if self.workspace != Workspace::Grid || self.kit_editor.is_some() {
            return None;
        }
        let body = self.body(size);
        let pattern = self.pattern()?;
        Some(view::Layout::new(body.main_w, body.editor_h, size.2, pattern, self.status.len(), self.current_page))
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
            None => return false,
        }
        true
    }

    /// A track: rename, copy, paste after it, duplicate, delete, randomize, clear.
    fn track_menu(&mut self, t: usize, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let title = track.name.clone();
        let mut items = vec![("rename", "Rename track"), ("copy", "Copy track")];
        if matches!(self.clipboard, Some(Clipboard::Track(_))) {
            items.push(("paste", "Paste track"));
        }
        items.extend([
            ("duplicate", "Duplicate track"),
            ("delete", "Delete track"),
            ("randomize", "Randomize"),
            ("clear", "Clear notes"),
        ]);
        let values = items.iter().map(|(action, _)| format!("{action}:{t}")).collect();
        let labels = items.iter().map(|(_, label)| (*label).to_owned()).collect();
        self.dropdown = Some(Dropdown::menu(Id::GridMenu, x, y, title, values, labels));
    }

    /// A step: copy its notes, paste on it, delete them, add a random note.
    fn step_menu(&mut self, t: usize, beat: i32, step: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let title = format!("{} @ {}.{}", track.name, beat + 1, step + 1);
        let has_notes = track.notes.iter().any(|n| n.beat == beat && n.step == step);
        let mut items = vec![("copy-notes", "Copy notes")];
        if matches!(&self.clipboard, Some(Clipboard::Step(notes)) if !notes.is_empty()) {
            items.push(("paste-notes", "Paste notes"));
        }
        if has_notes {
            items.push(("delete-notes", "Delete notes"));
        }
        items.push(("random-note", "Add rnd note"));
        let values = items.iter().map(|(action, _)| format!("{action}:{t}:{beat}:{step}")).collect();
        let labels = items.iter().map(|(_, label)| (*label).to_owned()).collect();
        self.dropdown = Some(Dropdown::menu(Id::GridMenu, x, y, title, values, labels));
    }

    /// An action of a grid menu: the cursor goes to its track or step, and
    /// the action runs as its key would there.
    pub(super) fn grid_menu_pick(&mut self, value: &str) {
        let mut parts = value.split(':');
        let action = parts.next().unwrap_or_default();
        let mut number = || parts.next().and_then(|v| v.parse::<i32>().ok());
        let Some(t) = number().and_then(|t| usize::try_from(t).ok()) else { return };
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return };
        let (beat, step) = (number().unwrap_or(0), number().unwrap_or(0));
        self.cursor = Some(Cursor { track: t, beat, step });
        match action {
            "rename" => self.text_entry(Id::TrackName(t)),
            "copy" => self.copy(true),
            "paste" => self.paste(true),
            "duplicate" => self.duplicate_track(t),
            "delete" => self.remove_track(),
            "randomize" => self.randomize_track(),
            "clear" => self.clear_track(t),
            "copy-notes" => self.copy(false),
            "paste-notes" => self.paste(false),
            "delete-notes" => {
                let count = self
                    .track(t)
                    .map_or(0, |track| track.notes.iter().filter(|n| n.beat == beat && n.step == step).count());
                self.delete_step();
                self.message = format!("Deleted {} - {name} @ {}", notes_label(count), beat_label(beat, step));
            }
            "random-note" => self.add_random_note(t, beat, step),
            _ => {}
        }
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
            self.cursor = Some(Cursor { track: copy, beat: 0, step: 0 });
            self.selected = None;
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
        self.selected = Some((t, n));
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

    /// The menu open, its labels.
    fn labels(s: &Session) -> Vec<String> {
        s.dropdown.as_ref().map(|d| d.labels.clone()).unwrap_or_default()
    }

    /// Run the menu's action labelled `label`.
    fn pick(s: &mut Session, label: &str) {
        let d = s.dropdown.take().expect("a menu");
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
        assert_eq!(s.pattern().unwrap().tracks.len(), count + 1);
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
        assert_eq!(s.dropdown.as_ref().and_then(|d| d.title.clone()), Some(pattern.tracks[0].name.clone()));
        let (x, y) = cell.expect("a step");
        assert!(s.grid_menu(SIZE, x, y));
        assert!(labels(&s).contains(&"Add rnd note".to_owned()));
        assert!(!s.grid_menu(SIZE, 0, 0), "the menu bar is not the grid");
    }
}
