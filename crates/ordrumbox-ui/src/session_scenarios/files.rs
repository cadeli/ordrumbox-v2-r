//! Files out and back in: kits, synth presets and songs.

use super::super::{OverlayKey, SYNTHS_FILE_NAME, Session};
use super::{RATE, demo_session, draw, run};
use crate::menu::Command;
use crate::picker::Outcome;
use crate::store;
use crate::test_support::scratch_dir;
use ordrumbox_core::json;
use ordrumbox_engine::{AudioSide, SampleBuffer, SynthPresets, wav};
use std::path::Path;

// ─── Files out and back in ──────────────────────────────────────────────────

/// A WAV file of `frames` frames of a sine at `level`.
fn write_wav(path: &Path, level: f32, frames: usize) -> Vec<u8> {
    let buffer = SampleBuffer::mono((0..frames).map(|i| level * (i as f32 * 0.3).sin()).collect(), RATE as u32);
    let bytes = wav::encode_float(&buffer);
    std::fs::write(path, &bytes).unwrap();
    bytes
}

/// Sample `sample` of the kit in use, clicked in the kit editor: the energy heard.
fn heard(s: &mut Session, audio: &mut AudioSide, sample: usize) -> f64 {
    s.kit_click(sample);
    run(s, audio, 1024)
}

#[test]
fn a_folder_dropped_becomes_a_kit_and_a_wav_dropped_with_shift_replaces_a_sample() {
    let dir = scratch_dir("kit-files");
    let samples = dir.join("samples");
    let (mut s, mut audio) = demo_session(&samples);
    let kit_count = s.kits.kits.len();

    // A folder of WAV files, with a file that is not one and one that does not decode.
    let folder = dir.join("my kit");
    std::fs::create_dir_all(&folder).unwrap();
    let kick = write_wav(&folder.join("Kick01.wav"), 0.8, 800);
    let snare = write_wav(&folder.join("Snare.wav"), 0.5, 800);
    std::fs::write(folder.join("broken.wav"), b"not a wav").unwrap();
    std::fs::write(folder.join("notes.txt"), b"not audio").unwrap();
    s.drop_path(&folder, false);
    assert_eq!(s.message, "Imported 2 files into kit \"my kit\" (1 not decoded)");
    assert_eq!(s.kits.kits.len(), kit_count + 1);
    assert_eq!(s.kit_index, kit_count);
    let keys: Vec<&str> = s.kits.kits[kit_count].samples.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["KICK", "SNARE"]);
    assert_eq!(s.kit_list(), Some((0, 2)), "the kit editor is on its first sample");
    // The audio is kept in the user's samples folder, as it came, in a folder of the kit's.
    assert_eq!(std::fs::read(samples.join("imported/my_kit/Kick01.wav")).unwrap(), kick);
    assert_eq!(std::fs::read(samples.join("imported/my_kit/Snare.wav")).unwrap(), snare);
    let loud = heard(&mut s, &mut audio, 0);
    assert!(loud > 0.0, "the imported sample is heard");
    draw(&mut s);

    // Shift + a WAV dropped: new audio for the selected sample, same url.
    let quiet = dir.join("quiet.wav");
    write_wav(&quiet, 0.1, 800);
    s.drop_path(&quiet, true);
    assert_eq!(s.message, "Replaced with \"quiet.wav\"");
    let sample = &s.kits.kits[kit_count].samples[0];
    assert_eq!(
        (sample.path.as_str(), sample.display_name.as_str(), sample.key.as_str()),
        ("imported/my_kit/Kick01.wav", "quiet.wav", "KICK")
    );
    let saved = wav::decode(&std::fs::read(samples.join("imported/my_kit/Kick01.wav")).unwrap()).unwrap();
    assert_eq!(saved.left, wav::decode(&std::fs::read(&quiet).unwrap()).unwrap().left);
    let replaced = heard(&mut s, &mut audio, 0);
    assert!(replaced > 0.0 && replaced * 10.0 < loud, "the new audio is heard: {replaced} vs {loud}");
    assert_eq!(s.kits.kits[kit_count].samples.len(), 2, "replaced, not added");

    // A file that does not decode replaces nothing.
    s.drop_path(&folder.join("broken.wav"), true);
    assert!(s.message.starts_with("Failed to decode WAV"), "{}", s.message);
    assert_eq!(s.kits.kits[kit_count].samples[0].display_name, "quiet.wav");

    // The folder again: the kit of that name is replaced, not doubled.
    s.drop_path(&folder, false);
    assert_eq!(s.kits.kits.len(), kit_count + 1);
    assert_eq!(s.kits.kits[kit_count].samples[0].display_name, "Kick01.wav");
    let back = heard(&mut s, &mut audio, 0);
    assert!(back > replaced * 10.0, "the folder's audio is back: {back} vs {replaced}");

    // A folder without audio: no kit.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    s.drop_path(&empty, false);
    assert_eq!(s.message, "No audio files found in selected directory");
    assert_eq!(s.kits.kits.len(), kit_count + 1);
    draw(&mut s);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn synth_presets_exported_are_imported_and_saved_ones_read_at_start() {
    let dir = scratch_dir("synth-files");
    let (mut s, _audio) = demo_session(&dir.join("samples"));
    let presets = |text: &str| SynthPresets::from_json(&json::parse(text).unwrap()).unwrap();
    s.set_asset_synths(presets(r#"{"bass":{"vco1":{"gain":0.5}},"lead":{"vco1":{"gain":0.25}}}"#));
    let exported = s.synths.presets.to_json().to_json();

    // Export, from the synth editor's preset bar: a folder picked.
    s.run(Command::SynthEditor);
    s.synth_button("export");
    assert!(s.overlay_open());
    s.picker_outcome(Outcome::Picked(dir.clone()));
    let file = dir.join(SYNTHS_FILE_NAME);
    assert_eq!(s.message, format!("Synth sounds exported to {}", file.display()));
    assert_eq!(store::load_synths(&file).unwrap().unwrap().to_json().to_json(), exported);

    // Import, in another session: the file's presets join its own, the same
    // names replaced; the preset being edited is the imported one.
    let (mut other, _audio) = demo_session(&dir.join("samples"));
    other.set_asset_synths(presets(r#"{"lead":{"vco1":{"gain":0.75}},"pad":{}}"#));
    other.run(Command::SynthEditor);
    other.load_synth("lead");
    other.synth_button("import");
    other.picker_outcome(Outcome::Picked(file.clone()));
    assert_eq!(other.message, "Imported 2 synth sound(s)");
    assert_eq!(other.synth_keys(), ["bass", "lead", "pad"]);
    assert_eq!(other.synths.presets.get("lead"), s.synths.presets.get("lead"));
    assert!(other.saving.library.pending(), "the imported presets wait to be saved with the library");
    draw(&mut other);
    // Revert goes back to the imported preset, not the one it replaced.
    other.synth_button("revert");
    assert_eq!(other.synths.presets.get("lead"), s.synths.presets.get("lead"));

    // Files that are not presets change nothing.
    let before = other.synths.presets.to_json().to_json();
    for (name, text, message) in [
        ("list.json", "[1]", "Invalid synth file: expected a JSON object"),
        ("cut.json", "{", "Import failed: "),
        ("missing.json", "", "Import failed: "),
    ] {
        let path = dir.join(name);
        if !text.is_empty() {
            std::fs::write(&path, text).unwrap();
        }
        other.import_synths(&path);
        assert!(other.message.starts_with(message), "{name}: {}", other.message);
        assert_eq!(other.synths.presets.to_json().to_json(), before, "{name}");
    }

    // A presets file of an earlier version, for a library without presets of
    // its own: those it holds that the assets do not hold as they are; an
    // asset's preset it does not hold is kept.
    let assets = presets(r#"{"bass":{"vco1":{"gain":0.5}},"new":{"vco1":{"gain":1}}}"#);
    let mut status = Vec::new();
    let own = Session::read_user_synths(Some(file.clone()), &assets, &mut status).expect("read");
    assert!(status.is_empty(), "{status:?}");
    assert_eq!(json::Value::Object(own.clone()).to_json(), r#"{"lead":{"vco1":{"gain":0.25}}}"#);
    let merged = crate::session::session_synth::merged_synths(Some(&own), &assets);
    assert_eq!(merged.keys(), ["lead", "bass", "new"]);
    // No file: none of its own.
    for path in [Some(dir.join("none.json")), None] {
        assert_eq!(Session::read_user_synths(path, &assets, &mut status), Some(json::Object::new()));
    }
    assert!(status.is_empty(), "{status:?}");
    // A file that cannot be read: said, and read again at the next start.
    assert_eq!(Session::read_user_synths(Some(dir.join("cut.json")), &assets, &mut status), None);
    assert_eq!(status.len(), 1);
    assert!(status[0].starts_with("synth presets not loaded: "), "{}", status[0]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_song_saved_comes_back_from_the_load_menu() {
    let dir = scratch_dir("song-files");
    let songs = dir.join("songs");
    let (mut s, _audio) = demo_session(&dir.join("samples"));

    // No user data folder: nothing saved, nothing to load.
    s.run(Command::SaveSong);
    assert_eq!(s.message, "Save failed: no user data folder");
    s.run(Command::LoadSong);
    assert_eq!(s.message, "No user data folder: no saved songs");

    s.saving.songs_dir = Some(songs.clone());
    assert!(s.menu_state().can_save_song && !s.menu_state().saved_songs);
    s.run(Command::LoadSong);
    assert_eq!(s.message, "No saved songs found");

    // Saved, in the songs folder (made at need).
    let saved = s.editor.library().to_file_text();
    s.run(Command::SaveSong);
    assert!(s.message.starts_with("Song \"") && s.message.ends_with("\" saved"), "{}", s.message);
    let files = store::list_songs(&songs).unwrap();
    assert_eq!(files.len(), 1);
    assert!(s.menu_state().saved_songs);

    // Changed, then loaded back from the list, once confirmed: one undo step.
    let count = s.editor.library().patterns.len();
    s.run(Command::DuplicatePattern);
    s.run(Command::NewGeneratedPattern);
    assert_eq!(s.editor.library().patterns.len(), count + 2);
    s.run(Command::LoadSong);
    let menu = s.chrome.widgets.dropdown.as_ref().expect("the Load list");
    assert_eq!(menu.values.len(), 1);
    let value = menu.values[0].clone();
    draw(&mut s);
    s.song_menu_pick(&value);
    assert!(s.confirm_open(), "loading replaces the library: asked first");
    draw(&mut s);
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Enter);
    assert!(s.message.starts_with("Song \"") && s.message.ends_with("\" loaded"), "{}", s.message);
    assert_eq!(s.editor.library().patterns.len(), count);
    assert_eq!(s.editor.library().to_file_text(), saved);
    assert_eq!(s.editor.history().next_undo(), Some("Load song"));
    s.run(Command::Undo);
    assert_eq!(s.editor.library().patterns.len(), count + 2);

    // Saved again: the same file, written over.
    s.run(Command::SaveSong);
    assert_eq!(store::list_songs(&songs).unwrap(), files);

    // A songs folder that is a file: said, not listed nor written.
    let blocked = dir.join("blocked");
    std::fs::write(&blocked, b"").unwrap();
    s.saving.songs_dir = Some(blocked.clone());
    s.run(Command::SaveSong);
    assert!(s.message.starts_with("Save failed: "), "{}", s.message);
    s.run(Command::LoadSong);
    assert!(s.message.starts_with("Songs not listed: "), "{}", s.message);
    std::fs::remove_dir_all(&dir).unwrap();
}
