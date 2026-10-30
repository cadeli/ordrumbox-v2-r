//! Confirmations: what removes or replaces the user's work waits for Enter
//! in a notice; Esc or a click cancels it. A single note is removed at once.

use super::*;

/// What waits for the user's yes.
#[derive(Debug, Clone)]
pub(super) enum Pending {
    RemoveTrack(usize),
    ClearTrack(usize),
    ClearStep(Cursor),
    RemoveClip(usize),
    /// Every clip of a pattern (its id) in the arrangement.
    RemoveRow(String),
    RemoveSong,
    RemoveSample {
        kit: usize,
        sample: usize,
    },
    RemoveSynth(String),
    LoadSong(PathBuf),
    ImportSong(PathBuf),
    RandomTrack(usize),
    CompactTracks,
}

impl Session {
    /// Show the notice of `pending`.
    pub(super) fn ask(&mut self, pending: Pending) {
        self.overlay = Some(Overlay::Confirm(pending));
    }

    /// Whether a confirmation is waiting for the user.
    pub(super) fn confirm_open(&self) -> bool {
        matches!(self.overlay, Some(Overlay::Confirm(_)))
    }

    /// The user said yes.
    pub(super) fn run_pending(&mut self, pending: Pending) {
        match pending {
            Pending::RemoveTrack(t) => self.remove_track_now(t),
            Pending::ClearTrack(t) => self.clear_track_now(t),
            Pending::ClearStep(cursor) => {
                self.cursor = Some(cursor);
                self.delete_step_now();
            }
            Pending::RemoveClip(c) => self.remove_clip(c),
            Pending::RemoveRow(id) => self.remove_row(&id),
            Pending::RemoveSong => self.remove_song(),
            Pending::RemoveSample { kit, sample } => self.kit_remove_now(kit, sample),
            Pending::RemoveSynth(key) => self.remove_synth(&key),
            Pending::LoadSong(path) => self.load_song_file(&path),
            Pending::ImportSong(path) => self.import_song_now(&path),
            Pending::RandomTrack(t) => self.randomize_track_now(t),
            Pending::CompactTracks => self.compact_tracks_now(),
        }
    }

    /// The name of track `t`, between quotes, and its notes.
    fn track_summary(&self, t: usize) -> String {
        self.track(t)
            .map_or_else(String::new, |track| format!("\"{}\": {}", track.name, notes_label(track.notes.len())))
    }

    /// The title and the lines of the notice of `pending`.
    pub(super) fn confirm_text(&self, pending: &Pending) -> (&'static str, Vec<String>) {
        const NO_UNDO: &str = "This cannot be undone.";
        let (title, what, verb) = match pending {
            Pending::RemoveTrack(t) => ("Delete the track?", self.track_summary(*t), "delete"),
            Pending::ClearTrack(t) => ("Clear the track?", self.track_summary(*t), "clear"),
            Pending::ClearStep(c) => {
                let name = self.track(c.track).map_or("", |track| track.name.as_str());
                let count = self.step_note_count(*c);
                let what = format!("{} - {name} @ {}", notes_label(count), beat_label(c.beat, c.step));
                ("Delete the notes?", what, "delete")
            }
            Pending::RemoveClip(c) => {
                let clip = self.song().and_then(|song| song.clips.get(*c));
                let what = clip.map_or_else(String::new, |clip| {
                    format!("\"{}\" at measure {}", self.pattern_label(&clip.pattern_id), clip.start_measure + 1)
                });
                ("Delete the clip?", what, "delete")
            }
            Pending::RemoveRow(id) => {
                let count = self.song().map_or(0, |song| song.clips.iter().filter(|c| c.pattern_id == *id).count());
                ("Delete the row?", format!("\"{}\": {count} clip(s)", self.pattern_label(id)), "delete")
            }
            Pending::RemoveSong => {
                let what = self
                    .song()
                    .map_or_else(String::new, |song| format!("\"{}\": {} clip(s)", song.name, song.clips.len()));
                ("Delete the arrangement?", what, "delete")
            }
            Pending::RemoveSample { kit, sample } => {
                let kit = self.kits.kits.get(*kit);
                let name = kit.and_then(|k| k.samples.get(*sample)).map_or("", |s| s.display_name.as_str());
                let what = format!("\"{name}\" from kit \"{}\". {NO_UNDO}", kit.map_or("", |k| k.name.as_str()));
                ("Remove the sample?", what, "remove")
            }
            Pending::RemoveSynth(key) => ("Delete the preset?", format!("\"{key}\". {NO_UNDO}"), "delete"),
            Pending::LoadSong(path) | Pending::ImportSong(path) => {
                let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                ("Replace the library?", format!("{name} takes the place of every pattern and arrangement."), "replace")
            }
            Pending::RandomTrack(t) => {
                ("Replace the notes?", format!("{} replaced by random ones", self.track_summary(*t)), "replace")
            }
            Pending::CompactTracks => {
                ("Compact the tracks?", "Tracks whose notes repeat get a shorter loop.".to_owned(), "compact")
            }
        };
        (title, vec![what, String::new(), format!("ENTER: {verb}   ESC: cancel")])
    }
}

#[cfg(test)]
mod tests {
    use ordrumbox_core::json::Value;

    use super::*;

    fn session() -> Session {
        let (library, _) =
            Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{}]},{"name":"SNARE"}]}]}"#)
                .unwrap();
        Session::new(library, KitSet::empty(), Vec::new())
    }

    #[test]
    fn a_track_goes_only_once_confirmed() {
        let mut s = session();
        let tracks = s.pattern().unwrap().tracks.len();
        s.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        s.remove_track();
        assert!(s.confirm_open());
        assert_eq!(s.pattern().unwrap().tracks.len(), tracks);
        s.overlay_key(OverlayKey::Escape);
        assert!(!s.confirm_open());
        assert_eq!(s.pattern().unwrap().tracks.len(), tracks);
        s.remove_track();
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.confirm_open());
        assert_eq!(s.pattern().unwrap().tracks.len(), tracks - 1);
    }

    #[test]
    fn a_click_cancels() {
        let mut s = session();
        let tracks = s.pattern().unwrap().tracks.len();
        s.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        s.remove_track();
        s.overlay_click(800, 600, 1, 10, 10);
        assert!(!s.confirm_open());
        assert_eq!(s.pattern().unwrap().tracks.len(), tracks);
    }

    #[test]
    fn one_note_goes_at_once_and_several_are_confirmed() {
        let mut s = session();
        let p = s.pattern_index;
        s.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        let c = s.cursor.unwrap();
        // The step starts empty, whatever the library holds.
        s.delete_step_now();
        assert!(s.editor.add_note(p, 0, 0, 0, 60.0).is_some());
        s.delete_step();
        assert!(!s.confirm_open());
        assert_eq!(s.step_note_count(c), 0);

        assert!(s.editor.add_note(p, 0, 0, 0, 60.0).is_some());
        assert!(s.editor.add_note(p, 0, 0, 0, 64.0).is_some());
        s.delete_step();
        assert!(s.confirm_open());
        assert_eq!(s.step_note_count(c), 2);
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(s.step_note_count(c), 0);
    }

    #[test]
    fn nothing_to_lose_asks_nothing() {
        let mut s = session();
        let p = s.pattern_index;
        s.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        assert!(s.editor.clean_track(p, 0));
        s.clear_track(0);
        assert!(!s.confirm_open());
        assert_eq!(s.message, "No notes to clear");
        s.randomize_track();
        assert!(!s.confirm_open());
        assert!(!s.track(0).unwrap().notes.is_empty());
        // Now it has notes to lose.
        s.randomize_track();
        assert!(s.confirm_open());
    }

    #[test]
    fn the_mcp_tools_wait_for_the_answer() {
        let mut s = session();
        s.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        s.remove_track();
        assert!(s.mcp_call("listPatterns", &Value::Null).is_err());
        s.overlay_key(OverlayKey::Escape);
        assert!(s.mcp_call("listPatterns", &Value::Null).is_ok());
    }
}
