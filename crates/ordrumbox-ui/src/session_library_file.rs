//! The user's library file through a session, from one start to the next:
//! a file that cannot be read is never written over, an edit is saved a
//! moment later (or on quit) and read at the next start, nothing is written
//! without an edit, and a save that fails is reported and done later.
//!
//! Each test has a folder of its own; the session starts as
//! [`Session::load`] does, on that folder's `library.json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ordrumbox_core::Library;

use super::Session;
use crate::kits::KitSet;
use crate::menu::Command;
use crate::panel::{self, Id, Kind};
use crate::scale::Scale;
use crate::ui_input::Button;
use crate::view;
use crate::view::Hit;

const DEMO: &str = r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK","notes":[{}]}]}]}"#;

/// `library.json` in a folder of its own for test `name`, the folder empty.
fn library_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordrumbox-library-{name}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir.join("library.json")
}

/// A session started on the library file `path`, the demo library otherwise.
fn start(path: &Path) -> Session {
    let mut status = Vec::new();
    let demo = Library::parse(DEMO).unwrap().0;
    let (library, library_path, message) = Session::open_library(Some(path.to_owned()), demo, &mut status);
    let mut session = Session::new(library, KitSet::empty(), status);
    session.saving.library_path = library_path;
    session.message = message;
    session
}

/// A note added by a click on the grid: an edit.
fn add_note(session: &mut Session, beat: i32) {
    let before = session.pattern().unwrap().tracks[0].notes.len();
    session.click(Hit::Cell { track: 0, beat, step: 0 });
    assert_eq!(session.pattern().unwrap().tracks[0].notes.len(), before + 1);
}

/// As if the save delay had passed since the last edit.
fn wait_save_delay(session: &mut Session) {
    session.saving.library.expire();
}

fn temporary(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[test]
fn a_library_file_that_cannot_be_read_is_never_written_over() {
    let path = library_file("unreadable");
    let broken = r#"{"patterns":[{"name":"mine","#;
    fs::write(&path, broken).unwrap();

    let mut s = start(&path);
    assert!(s.status.iter().any(|line| line.starts_with("library not loaded, not saved over")), "{:?}", s.status);
    assert_eq!(s.pattern().unwrap().name.as_deref(), Some("demo"), "the demo library instead");
    assert_eq!(s.saving.library_path, None);

    // Edits, the save a moment later, an explicit save, the save on quit.
    add_note(&mut s, 1);
    wait_save_delay(&mut s);
    assert_eq!(s.autosave(), None);
    s.save();
    assert!(s.message.starts_with("not saved"), "{}", s.message);
    s.save_if_changed();
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    assert!(!temporary(&path).exists());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn an_edit_is_saved_a_moment_later_and_read_at_the_next_start() {
    let path = library_file("lifecycle");

    // First start: no file yet, the demo library, saved to that file.
    let mut s = start(&path);
    assert!(s.status.is_empty(), "{:?}", s.status);
    assert_eq!(s.saving.library_path.as_deref(), Some(path.as_path()));
    assert!(!path.exists());

    // An edit: saved once the delay has passed, not before.
    add_note(&mut s, 1);
    let due = s.autosave().expect("a save to come");
    assert!(due > Instant::now());
    assert!(!path.exists(), "saved before the delay");
    wait_save_delay(&mut s);
    assert_eq!(s.autosave(), None);
    assert!(path.exists());
    assert!(!temporary(&path).exists());
    assert!(s.message.starts_with("saved"), "{}", s.message);

    // An edit just before quitting: saved on quit.
    add_note(&mut s, 2);
    s.save_if_changed();
    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(saved, s.editor.library().to_file_text());

    // The next start reads it.
    let mut next = start(&path);
    assert!(next.status.is_empty(), "{:?}", next.status);
    assert!(next.message.starts_with("library "), "{}", next.message);
    assert_eq!(next.editor.library().to_file_text(), saved);
    assert_eq!(next.pattern().unwrap().tracks[0].notes.len(), 3);

    // Without an edit nothing is written: the file removed stays removed.
    fs::remove_file(&path).unwrap();
    assert_eq!(next.autosave(), None);
    next.save_if_changed();
    assert!(!path.exists(), "written without an edit");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_save_that_fails_is_reported_and_done_later() {
    let path = library_file("failing");
    let mut s = start(&path);
    // A temporary file left by a save that did not finish.
    fs::write(temporary(&path), "half a fi").unwrap();
    // A folder where the file goes: the save cannot replace it.
    fs::create_dir(&path).unwrap();

    add_note(&mut s, 1);
    s.save_if_changed();
    assert!(s.message.starts_with("not saved: "), "{}", s.message);
    assert!(path.is_dir());
    // Still to save: the save a moment later tries again, and fails again.
    wait_save_delay(&mut s);
    assert_eq!(s.autosave(), None);
    assert!(s.message.starts_with("not saved: "), "{}", s.message);

    // Once the way is clear, the next save writes the file whole.
    fs::remove_dir(&path).unwrap();
    s.save_if_changed();
    assert!(s.message.starts_with("saved"), "{}", s.message);
    assert_eq!(fs::read_to_string(&path).unwrap(), s.editor.library().to_file_text());
    assert!(!temporary(&path).exists());
    assert_eq!(start(&path).pattern().unwrap().tracks[0].notes.len(), 2);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_host_library_comes_back_as_text_and_is_never_saved() {
    let demo = Library::parse(DEMO).unwrap().0;
    let saved = r#"{"patterns":[{"name":"kept","tracks":[{"name":"SNARE","notes":[{},{}]}]}]}"#;
    let mut status = Vec::new();
    let (library, path, message) = Session::open_host_library(Some(saved.to_owned()), demo.clone(), &mut status);
    assert_eq!((path, message.as_str(), status.len()), (None, "library from the host project", 0));
    let mut s = Session::new(library, KitSet::empty(), status);
    s.hosted = true;
    assert_eq!(s.pattern().unwrap().name.as_deref(), Some("kept"));
    s.save();
    assert_eq!(s.message, "the host saves the library with its project");
    let (again, _, _) = Session::open_host_library(Some(s.library_text()), demo.clone(), &mut Vec::new());
    assert_eq!(again.to_file_text(), s.library_text());

    let mut status = Vec::new();
    let (library, path, _) = Session::open_host_library(Some("{".to_owned()), demo, &mut status);
    assert_eq!((library.patterns[0].name.as_deref(), path), (Some("demo"), None));
    assert!(status[0].starts_with("library of the host project not loaded: "), "{status:?}");
}

#[test]
fn a_plugin_leaves_to_the_host_what_the_host_does() {
    let library = Library::parse(DEMO).unwrap().0;
    let mut s = Session::new(library, KitSet::empty(), Vec::new());
    s.hosted = true;
    let bpm = s.editor.library().bpm;
    let refused = [
        Command::Save,
        Command::Quit,
        Command::TempoUp,
        Command::CheckUpdates,
        Command::FactoryReset,
        Command::SongView,
        Command::PlaySong,
        Command::NewSong,
    ];
    for command in refused {
        s.run(command);
        assert_eq!(Some(s.message.as_str()), command.hosted_refusal(), "{command:?}");
    }
    assert!(!s.quit_requested());
    assert_eq!(s.editor.library().bpm, bpm);
    assert_eq!(s.editor.history().next_undo(), None);
    assert!(!s.song_view_open() && !s.playing());
    s.toggle_play_song();
    assert_eq!(s.message, crate::menu::SONGS_IN_THE_APP);
    // Greyed out in the menus (`menu.rs` tests), and the tempo shown is not the session's knob.
    let runnable = s.menus().into_iter().flat_map(|m| m.items).filter_map(|i| i.runnable());
    assert!(runnable.into_iter().all(|c| c.hosted_refusal().is_none()));
    let size = (1100, 680, Scale::ONE);
    let layout = s.panel_layout(size.0, size.1, size.2);
    assert!(!layout.items.iter().any(|i| i.id == Some(Id::Bpm)));
    // The SONG view button greyed out: a click says why.
    let song = panel::find(&layout.items, Id::View(2)).unwrap();
    assert!(matches!(song.kind, Kind::Disabled { .. }));
    let (x, y) = (song.rect.x + 2, song.rect.y + 2);
    s.message.clear();
    s.mouse_down(size, x, y, Button::Left, false);
    s.mouse_up();
    assert_eq!(s.message, crate::menu::SONGS_IN_THE_APP);
    assert!(!s.song_view_open());
    // Tooltips say whose the tempo and the songs are.
    s.control_hover(size, x, y);
    assert_eq!(s.chrome.control_hover.map(|(id, _)| s.control_tip(id).unwrap()[0]), Some("Song: in the app only"));
    let bpm = view::header_controls(Scale::ONE).bpm;
    s.control_hover(size, bpm.x + 2, bpm.y + 2);
    assert_eq!(s.chrome.control_hover.map(|(id, _)| s.control_tip(id).unwrap()[0]), Some("Tempo (HOST)"));
    // No master strip: the editors reach down to where it was.
    assert!(!layout.items.iter().any(|i| matches!(i.id, Some(Id::OutTab(_) | Id::Master(_)))));
    assert!(s.output_view().is_none());
    let body = s.body((1100, 680, Scale::ONE));
    assert_eq!(body.output.h, 0);
    s.hosted = false;
    assert!(s.body((1100, 680, Scale::ONE)).main_bottom < body.main_bottom);
}
