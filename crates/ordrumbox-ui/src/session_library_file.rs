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
use crate::menu::LibraryState;
use crate::test_support::scratch_dir;
use crate::view::Hit;

const DEMO: &str = r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK","notes":[{}]}]}]}"#;

/// `library.json` in a folder of its own for test `name`, the folder empty.
fn library_file(name: &str) -> PathBuf {
    scratch_dir(&format!("library-{name}")).join("library.json")
}

/// A session started on the library file `path`, the starter library otherwise.
fn start(path: &Path) -> Session {
    let mut status = Vec::new();
    let demo = Library::parse(DEMO).unwrap().0;
    let opened = Session::open_library(Some(path.to_owned()), demo, &mut status);
    let mut session = Session::new(opened.library, KitSet::empty(), status);
    session.saving.library_path = opened.file;
    session.saving.on_disk = opened.on_disk;
    session.message = opened.message;
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
    assert_eq!(s.pattern().unwrap().name.as_deref(), Some("demo"), "the starter library instead");
    assert_eq!(s.saving.library_path, None);
    assert_eq!(s.library_state(), LibraryState::NotSaved);

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

    // First start: no file yet, the starter library, saved to that file.
    let mut s = start(&path);
    assert!(s.status.is_empty(), "{:?}", s.status);
    assert_eq!(s.saving.library_path.as_deref(), Some(path.as_path()));
    assert!(!path.exists());
    assert_eq!(s.library_state(), LibraryState::New);
    assert!(s.message.contains("copy of the starter library"), "{}", s.message);

    // An edit: saved once the delay has passed, not before.
    add_note(&mut s, 1);
    assert_eq!(s.library_state(), LibraryState::Modified);
    let due = s.autosave().expect("a save to come");
    assert!(due > Instant::now());
    assert!(!path.exists(), "saved before the delay");
    wait_save_delay(&mut s);
    assert_eq!(s.autosave(), None);
    assert!(path.exists());
    assert!(!temporary(&path).exists());
    assert_eq!(s.library_state(), LibraryState::Saved);
    assert!(!s.message.contains("library.json"), "saved without a word: {}", s.message);

    // An edit just before quitting: saved on quit.
    add_note(&mut s, 2);
    s.save_if_changed();
    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(saved, s.editor.library().to_file_text());

    // The next start reads it.
    let mut next = start(&path);
    assert!(next.status.is_empty(), "{:?}", next.status);
    assert_eq!(next.message, "library loaded");
    assert_eq!(next.library_state(), LibraryState::Saved);
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
    assert!(s.message.starts_with("library not saved: "), "{}", s.message);
    assert_eq!(s.library_state(), LibraryState::Failed);
    assert!(path.is_dir());
    // Still to save: the save a moment later tries again, and fails again.
    wait_save_delay(&mut s);
    assert_eq!(s.autosave(), None);
    assert!(s.message.starts_with("library not saved: "), "{}", s.message);

    // Once the way is clear, the next save writes the file whole.
    fs::remove_dir(&path).unwrap();
    s.save();
    assert_eq!(s.message, "library saved");
    assert_eq!(s.library_state(), LibraryState::Saved);
    assert_eq!(fs::read_to_string(&path).unwrap(), s.editor.library().to_file_text());
    assert!(!temporary(&path).exists());
    assert_eq!(start(&path).pattern().unwrap().tracks[0].notes.len(), 2);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_values_repaired_in_the_library_are_shown() {
    let path = library_file("repaired");
    fs::write(&path, r#"{"bpm":"fast","patterns":[{"name":"p"}]}"#).unwrap();
    let s = start(&path);
    assert_eq!(s.status.len(), 1, "{:?}", s.status);
    assert!(
        s.status[0].starts_with(&format!("{}: 1 value(s) repaired, first: bpm: ", path.display())),
        "{:?}",
        s.status
    );

    let mut status = Vec::new();
    let demo = Library::parse(DEMO).unwrap().0;
    Session::open_text_library(r#"{"bpm":"fast"}"#, demo.clone(), &mut status);
    assert!(status[0].starts_with("library given: 1 value(s) repaired"), "{status:?}");
    let mut status = Vec::new();
    let opened = Session::open_text_library("{", demo, &mut status);
    assert_eq!((opened.library.patterns[0].name.as_deref(), opened.file), (Some("demo"), None));
    assert!(status[0].starts_with("library given not loaded: "), "{status:?}");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
