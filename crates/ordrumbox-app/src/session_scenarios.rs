//! The Playwright scenarios of ordrumbox-v2 that go through the session:
//! `e2e/playback`, `playback_tempo`, `song-playback` (the view decides what
//! plays and at which tempo) and `pattern-lifecycle` (a pattern built in the
//! grid, saved, reloaded, exported to MIDI and WAV).
//!
//! The session gets a real [`Controller`]; its audio side is rendered here as
//! the audio callback would, with `update` before each callback as the UI does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordrumbox_core::json;
use ordrumbox_core::{Library, Rng, Scales};
use ordrumbox_engine::kit::{BankSample, Kit, KitSample};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{AudioSide, Controller, Mode, Position, SampleBank, SampleBuffer, SynthPresets};
use ordrumbox_logic::history::MAX_STEPS;
use ordrumbox_logic::midi::{self, PPQN, smf};

use super::session_panel::Button;
use super::{Move, OverlayKey, Session, Workspace};
use crate::canvas::Canvas;
use crate::editors;
use crate::kits::KitSet;
use crate::menu::Command;
use crate::panel::{self, Id, Kind, Rect};
use crate::picker::Export;
use crate::store;
use crate::view::Hit;

const RATE: f32 = 8000.0;
/// Frames per audio callback.
const CALLBACK: usize = 256;

fn bank(rate: u32) -> Arc<SampleBank> {
    let instrument = KitSample {
        key: "KICK".into(),
        display_name: "k".into(),
        path: "k".into(),
        decay: Some(100.0),
        ..Default::default()
    };
    let buffer = SampleBuffer::mono(vec![0.5; 400], rate);
    Arc::new(SampleBank { samples: vec![BankSample { kit: "x".into(), sample: instrument, buffer: buffer.into() }] })
}

/// A session on `json` with sound.
fn session(json: &str) -> (Session, AudioSide) {
    let (library, _) = Library::parse(json).unwrap();
    let mut session = Session::new(library, KitSet::empty(), Vec::new());
    let (controller, audio) =
        Controller::with_rng(RATE, bank(RATE as u32), Scales::default(), SynthPresets::default(), Rng::new(1));
    session.controller = Some(controller);
    (session, audio)
}

/// `frames` of playback; the energy heard.
fn run(session: &mut Session, audio: &mut AudioSide, frames: usize) -> f64 {
    let (mut l, mut r) = (vec![0.0; CALLBACK], vec![0.0; CALLBACK]);
    let mut energy = 0.0;
    for _ in 0..frames.div_ceil(CALLBACK) {
        session.update();
        audio.render(&mut l, &mut r);
        energy += l.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
    }
    energy
}

fn position(session: &Session) -> Option<Position> {
    session.controller.as_ref()?.position()
}

fn mode(session: &Session) -> Mode {
    session.controller.as_ref().unwrap().mode()
}

/// Frames of `beats` beats at `bpm` (a pass: the pattern, or a song measure).
fn pass_frames(beats: f64, bpm: f64) -> u64 {
    (beats * 60.0 / bpm * f64::from(RATE)).round() as u64
}

fn assert_pass_frames(session: &Session, beats: f64, bpm: f64) {
    let frames = position(session).expect("playing").frames;
    let expected = pass_frames(beats, bpm);
    assert!(frames.abs_diff(expected) <= 1, "{frames} frames a pass, {expected} at {bpm} bpm");
}

const LIBRARY: &str = r#"{"songs":[{"name":"s","bpm":120,"clips":[{"pattern":"p","startMeasure":0,"measureCount":4}]}],
    "patterns":[{"name":"p","bpm":140,"tracks":[{"name":"KICK","notes":[{},{"beat":2}]}]}]}"#;

// ─── playback.spec.js ───────────────────────────────────────────────────────

#[test]
fn space_plays_the_playhead_advances_and_space_stops() {
    let (mut s, mut audio) = session(LIBRARY);
    assert!(!s.playing());
    s.toggle_play();
    assert!(s.playing());
    assert!(run(&mut s, &mut audio, 1000) > 0.0);
    let first = position(&s).expect("playing");
    run(&mut s, &mut audio, 2000);
    let later = position(&s).expect("playing");
    assert!((later.pass, later.frame) > (first.pass, first.frame), "the playhead advances");
    s.toggle_play();
    assert!(!s.playing());
    run(&mut s, &mut audio, CALLBACK);
    assert_eq!(position(&s), None);
}

#[test]
fn a_click_on_a_grid_cell_sounds_the_note() {
    let (mut s, mut audio) = session(LIBRARY);
    // An empty step: a note is added and heard, while stopped.
    s.click(Hit::Cell { track: 0, beat: 1, step: 0 });
    assert_eq!(s.pattern().unwrap().tracks[0].notes.len(), 3);
    assert!(run(&mut s, &mut audio, 512) > 0.0);
    assert!(!s.playing());
    // A note: selected and heard.
    run(&mut s, &mut audio, 4000);
    s.click(Hit::Cell { track: 0, beat: 0, step: 0 });
    assert_eq!(s.selected, Some((0, 0)));
    assert!(run(&mut s, &mut audio, 512) > 0.0);
    // The track name: heard too.
    run(&mut s, &mut audio, 4000);
    s.click(Hit::Name(0));
    assert!(run(&mut s, &mut audio, 512) > 0.0);
}

// ─── playback_tempo.spec.js, song-playback.spec.js ──────────────────────────

#[test]
fn the_tempo_and_what_plays_follow_the_view() {
    let (mut s, mut audio) = session(LIBRARY);
    // The grid plays the pattern at its own tempo.
    s.toggle_play();
    run(&mut s, &mut audio, 1000);
    assert_eq!(mode(&s), Mode::Pattern(0));
    assert_pass_frames(&s, 4.0, 140.0);

    // The tempo changed while playing (the header's tempo control).
    s.set_tempo(100.0);
    run(&mut s, &mut audio, CALLBACK);
    assert_pass_frames(&s, 4.0, 100.0);

    // Grid → synth → piano roll: the same mode, neither the tempo nor the loop restarts.
    // (`show_view` takes the index of a view button: grid, piano roll, song, synth.)
    for (button, view) in [(3, Workspace::Synth), (1, Workspace::PianoRoll), (0, Workspace::Grid)] {
        let before = position(&s).expect("playing");
        s.show_view(button);
        assert_eq!(s.workspace, view);
        run(&mut s, &mut audio, CALLBACK);
        let after = position(&s).expect("playing");
        assert_eq!(mode(&s), Mode::Pattern(0), "{view:?}");
        assert!((after.pass, after.frame) > (before.pass, before.frame), "{view:?}: the loop goes on");
        assert_pass_frames(&s, 4.0, 100.0);
    }

    // The song view plays the arrangement, from its start, at the song's tempo.
    s.show_view(2);
    run(&mut s, &mut audio, CALLBACK);
    assert_eq!(mode(&s), Mode::Song(0));
    assert_eq!(position(&s).expect("playing").pass, 0);
    assert_pass_frames(&s, 4.0, 120.0);
    // ...and goes through it, measure after measure.
    assert!(run(&mut s, &mut audio, 20_000) > 0.0);
    assert_eq!(position(&s).expect("playing").pass, 1);

    // Back to the grid: the pattern again, at its tempo.
    s.show_view(0);
    run(&mut s, &mut audio, CALLBACK);
    assert_eq!(mode(&s), Mode::Pattern(0));
    assert_pass_frames(&s, 4.0, 100.0);
    s.toggle_play();
}

#[test]
fn space_in_the_song_view_plays_the_song() {
    let (mut s, mut audio) = session(LIBRARY);
    s.toggle_song_view();
    s.toggle_play();
    run(&mut s, &mut audio, 1000);
    assert_eq!(mode(&s), Mode::Song(0));
    assert_pass_frames(&s, 4.0, 120.0);
}

// ─── pattern-lifecycle.spec.js ──────────────────────────────────────────────

#[test]
fn a_pattern_built_in_the_grid_is_saved_reloaded_and_exported() {
    let (mut s, _audio) = session(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK","notes":[{}]}]}]}"#);

    // T1: a new pattern, 4 tracks, 8 beats at 100 bpm, notes clicked in, T2 soloed.
    s.new_pattern();
    assert_eq!(s.pattern_index, 1);
    // (The length first: a track keeps the loop point it was made with.)
    s.set_length(8);
    s.set_tempo(100.0);
    for _ in 0..4 {
        s.add_track();
    }
    let notes: [(usize, &[i32]); 3] = [(0, &[0, 2, 4, 6]), (1, &[1, 5]), (2, &[3])];
    for (track, beats) in notes {
        for &beat in beats {
            s.click(Hit::Cell { track, beat, step: 0 });
        }
    }
    s.click(Hit::Solo(1));
    let pattern = s.pattern().unwrap().clone();
    assert_eq!((pattern.beat_count, pattern.bpm), (8, 100.0));
    let names: Vec<&str> = pattern.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["T1", "T2", "T3", "T4"]);
    let counts: Vec<usize> = pattern.tracks.iter().map(|t| t.notes.len()).collect();
    assert_eq!(counts, [4, 2, 1, 0]);
    assert!(pattern.tracks[1].solo);

    // T2: saved and loaded again, nothing lost.
    let dir = std::env::temp_dir().join(format!("ordrumbox-lifecycle-{}", std::process::id()));
    let path = dir.join("library.json");
    store::save(&path, s.editor.library()).unwrap();
    let reloaded = store::load(&path).unwrap().expect("saved");
    std::fs::remove_dir_all(&dir).ok();
    // (`step_percent` is an editing aid, never saved: the files are compared.)
    assert_eq!(reloaded.to_file_text(), s.editor.library().to_file_text());
    assert_eq!(reloaded.patterns[1].tracks[1].notes.len(), 2);

    // T4: MIDI, only the soloed track, after the conductor track.
    let bytes = midi::export(&pattern, 1, &[], &mut Rng::new(1), &Scales::default());
    let file = smf::parse(&bytes);
    assert_eq!((file.header.format, file.header.track_count, file.header.division), (1, 2, PPQN as u16));
    assert_eq!(file.track_names, ["orDrumbox Pattern", "T2"]);
    let meta = |kind: u8| {
        file.tracks[0].iter().find_map(|e| match &e.kind {
            smf::EventKind::Meta { meta_type: Some(t), data } if *t == kind => Some(data.clone()),
            _ => None,
        })
    };
    assert_eq!(meta(0x58).unwrap()[..2], [4, 2], "4/4");
    assert_eq!(meta(0x51).unwrap(), [0x09, 0x27, 0xc0], "600000 µs a beat: 100 bpm");
    let ticks: Vec<i64> = file.note_ons().iter().map(|n| n.tick).collect();
    assert_eq!(ticks, [i64::from(PPQN), 5 * i64::from(PPQN)]);
    let (mut ons, mut offs) = (0, 0);
    for event in &file.tracks[1] {
        match event.kind {
            smf::EventKind::Note { status: 0x90, velocity, .. } if velocity > 0 => ons += 1,
            smf::EventKind::Note { status: 0x80, .. } | smf::EventKind::Note { status: 0x90, .. } => offs += 1,
            _ => {}
        }
    }
    assert_eq!((ons, offs), (2, 2), "every note-on has its note-off");

    // T5: WAV, one loop of the pattern at its tempo, with sound (the soloed track).
    let setup = RenderSetup {
        bank: bank(EXPORT_RATE),
        scales: Scales::default(),
        synths: SynthPresets::default(),
        kit: "x".into(),
        sample_rate: EXPORT_RATE,
        seed: 1,
    };
    let (left, _) = offline::render_pattern(s.editor.library(), 1, 1, &setup, None).expect("renders");
    // floor(rate × 8 beats × 60 / 100 bpm), as the JS sizes its OfflineAudioContext.
    assert_eq!(left.len(), (f64::from(EXPORT_RATE) * 8.0 * 60.0 / 100.0).floor() as usize);
    assert!(left.iter().any(|s| s.abs() > 0.01));
}

// ─── Auto mode ──────────────────────────────────────────────────────────────

#[test]
fn the_grid_shows_the_loop_being_heard_in_auto_mode() {
    const LOOP_COUNT: usize = 8;
    // 4 beats at 120 bpm: 16000 frames a loop, 1000 a step.
    const LOOP_FRAMES: usize = 16_000;
    const STEP_FRAMES: usize = 1000;
    // No variation: the passes play the lines as they are.
    let (mut s, mut audio) = session(
        r#"{"patterns":[{"name":"p","bpm":120,"tracks":[
            {"name":"KICK","autoMode":true,"autoHitVariation":0,"autoNoteVariation":0}]}]}"#,
    );
    s.rng = Rng::new(7);
    s.toggle_play();
    let (mut left, mut right) = (vec![0.0; CALLBACK], vec![0.0; CALLBACK]);
    let mut heard = Vec::new();
    let mut shown = Vec::new();
    while heard.len() < LOOP_COUNT * LOOP_FRAMES {
        s.update();
        audio.render(&mut left, &mut right);
        heard.extend_from_slice(&left);
        // In the middle of each loop.
        if heard.len() / LOOP_FRAMES == shown.len() && heard.len() % LOOP_FRAMES >= LOOP_FRAMES / 2 {
            shown.push(s.pattern().unwrap().tracks[0].notes.clone());
        }
    }
    assert!(shown.windows(2).any(|w| w[0] != w[1]), "the lines change");
    for (index, notes) in shown.iter().enumerate() {
        for note in notes {
            let at = index * LOOP_FRAMES + (note.beat * 4 + note.step) as usize * STEP_FRAMES;
            let sounds = heard[at..at + STEP_FRAMES / 4].iter().any(|s| s.abs() > 1e-3);
            assert!(sounds, "loop {index}: beat {} step {} is shown, not heard", note.beat, note.step);
        }
    }
}

#[test]
fn an_edit_in_auto_mode_goes_on_the_loop_being_heard_and_stays() {
    let (mut s, mut audio) =
        session(r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK","autoMode":true},{"name":"SNARE"}]}]}"#);
    s.toggle_play();
    run(&mut s, &mut audio, 1000);
    let lines = s.pattern().unwrap().tracks[0].notes.clone();
    // On a track not in auto mode, before the next loop starts.
    s.click(Hit::Cell { track: 1, beat: 1, step: 0 });
    run(&mut s, &mut audio, 1000);
    assert_eq!(s.pattern().unwrap().tracks[0].notes, lines, "the lines of this loop stay");
    run(&mut s, &mut audio, 16_000);
    assert_eq!(position(&s).expect("playing").pass, 1);
    assert_eq!(s.pattern().unwrap().tracks[1].notes.len(), 1, "the edit survives the next loop");
}

// ─── A whole session ────────────────────────────────────────────────────────

const WINDOW: (i32, i32, i32) = (1100, 680, 1);

/// One frame of the window, as the UI thread draws it after each event.
fn draw(s: &mut Session) {
    let (width, height, scale) = WINDOW;
    let mut pixels = vec![0; (width * height) as usize];
    s.set_size(WINDOW);
    s.draw(&mut Canvas::new(&mut pixels, width as usize, height as usize), scale);
    assert!(pixels.iter().any(|&p| p != pixels[0]), "a blank window");
}

/// The keys of the demo the kits have a sample for.
const KIT_KEYS: [&str; 4] = ["KICK", "SNARE", "CHH", "OHH"];

/// The demo library, kits `a` and `b` (`b` at half level) saving their
/// audio under `samples_dir`, and sound.
fn demo_session(samples_dir: &Path) -> (Session, AudioSide) {
    let text = include_str!("../../ordrumbox-core/tests/fixtures/song.json");
    let library = Library::from_json(&json::parse(text).unwrap()).0;
    let mut samples = Vec::new();
    for (kit, level) in [("a", 0.5), ("b", 0.25)] {
        for key in KIT_KEYS {
            let instrument = KitSample {
                key: key.into(),
                display_name: key.to_lowercase(),
                path: format!("{kit}/{key}.wav"),
                decay: Some(100.0),
                ..Default::default()
            };
            let buffer = SampleBuffer::mono((0..400).map(|i| level * (i as f32 * 0.3).sin()).collect(), RATE as u32);
            samples.push(BankSample { kit: kit.into(), sample: instrument, buffer: buffer.into() });
        }
    }
    let bank = SampleBank { samples };
    let kits = ["a", "b"]
        .map(|name| Kit {
            name: name.into(),
            samples: bank.samples.iter().filter(|s| s.kit == name).map(|s| s.sample.clone()).collect(),
            ..Kit::default()
        })
        .to_vec();
    let kits = KitSet::new(kits, &bank, None, None, Some(samples_dir.to_path_buf()));
    let mut session = Session::new(library, kits, Vec::new());
    session.rng = Rng::new(1);
    let (controller, audio) =
        Controller::with_rng(RATE, Arc::new(bank), Scales::default(), SynthPresets::default(), Rng::new(1));
    session.controller = Some(controller);
    // As after loading the assets: the bank, the exports, the kit in use.
    session.kits_changed("kits loaded".into());
    (session, audio)
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordrumbox-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_session_through_the_menus_draws_every_step() {
    let dir = scratch_dir("session");
    let (mut s, mut audio) = demo_session(&dir.join("samples"));
    draw(&mut s);
    assert!(!s.menus().is_empty());
    assert!(!s.check_audio(), "no output device: no error to show");

    // Kits and patterns, by the menus.
    s.run(Command::NextKit);
    assert_eq!((s.kit_index, s.message.as_str()), (1, "kit b"));
    s.run(Command::PreviousKit);
    assert_eq!(s.kit_index, 0);
    s.run(Command::SelectRandomDrumkit);
    s.run(Command::Kit(0));
    assert_eq!(s.message, "kit a");
    s.run(Command::SelectRandomPattern);
    assert!(s.message.starts_with("pattern "));
    s.run(Command::ToggleVus);
    assert_eq!(s.message, "VU meters off");
    draw(&mut s);
    s.run(Command::ToggleVus);

    // Playing, with the length and tempo changed under it.
    s.run(Command::PlayPattern);
    assert!(run(&mut s, &mut audio, 4000) > 0.0);
    let (bpm, beats) = s.pattern().map(|p| (p.bpm, p.beat_count)).unwrap();
    s.run(Command::TempoUp);
    s.run(Command::LengthUp);
    assert_eq!(s.pattern().map(|p| (p.bpm, p.beat_count)), Some((bpm + 1.0, beats + 1)));
    s.run(Command::TempoDown);
    s.run(Command::LengthDown);
    assert_eq!(s.pattern().map(|p| (p.bpm, p.beat_count)), Some((bpm, beats)));
    draw(&mut s);

    // The cursor and the step it is on: a note in, the grid and loop of its track changed.
    s.move_cursor(Move::Down);
    s.move_cursor(Move::Right);
    s.move_cursor(Move::Left);
    s.move_cursor(Move::Down);
    let c = s.cursor.unwrap();
    let notes = |s: &Session| s.pattern().unwrap().tracks[c.track].notes.len();
    let before = notes(&s);
    if s.selected.is_some() {
        s.enter(); // a note there: selected, entered again it goes
    }
    s.enter();
    assert_ne!(notes(&s), before, "Enter toggles the step");
    let spb = s.pattern().unwrap().tracks[c.track].steps_per_beat;
    s.run(Command::NextGrid);
    assert_ne!(s.pattern().unwrap().tracks[c.track].steps_per_beat, spb);
    s.run(Command::ShortenLoop);
    s.run(Command::PreviewTrack);
    draw(&mut s);
    s.run(Command::CompactTracks);
    if s.confirm_open() {
        draw(&mut s);
        s.overlay_key(OverlayKey::Enter);
    }
    assert!(!s.overlay_open());

    // Patterns made, generated and put in auto mode, while it plays.
    let count = s.editor.library().patterns.len();
    s.run(Command::DuplicatePattern);
    s.run(Command::NewGeneratedPattern);
    assert_eq!(s.editor.library().patterns.len(), count + 2);
    s.run(Command::GenerateDrums);
    s.run(Command::TrackAuto);
    assert!(run(&mut s, &mut audio, 8000) > 0.0);
    draw(&mut s);
    s.run(Command::NextPattern);
    s.run(Command::PreviousPattern);

    // Every view, drawn playing.
    for command in [Command::SongView, Command::PianoRoll, Command::SynthEditor, Command::Grid, Command::TrackEditor] {
        s.run(command);
        run(&mut s, &mut audio, CALLBACK);
        draw(&mut s);
    }
    s.run(Command::TrackEditor);
    s.show_generation_tab();
    assert!(s.show_track_editor);
    draw(&mut s);

    // The kit editor, stopped (what is heard is the sample): a sample chosen
    // and heard, the kit normalized, a sample removed.
    s.run(Command::PlayPattern);
    assert!(!s.playing());
    run(&mut s, &mut audio, 16_000);
    s.run(Command::KitEditor);
    assert_eq!(s.kit_list(), Some((0, KIT_KEYS.len())));
    s.kit_select(1);
    s.kit_select(-2);
    assert_eq!(s.kit_list(), Some((KIT_KEYS.len() - 1, KIT_KEYS.len())));
    s.kit_click(0);
    assert!(run(&mut s, &mut audio, 512) > 0.0, "the sample clicked is heard");
    s.kit_audition();
    draw(&mut s);
    s.run(Command::NormalizeKit);
    assert_eq!(s.message, format!("Normalized {} sample(s)", KIT_KEYS.len()));
    s.kit_remove();
    assert!(s.confirm_open());
    draw(&mut s);
    s.overlay_key(OverlayKey::Enter);
    assert_eq!(s.kit_list(), Some((0, KIT_KEYS.len() - 1)));
    s.drop_path(Path::new("notes.txt"), false);
    assert_eq!(s.message, "notes.txt: not a MIDI, WAV, song or kit file");
    s.run(Command::KitEditor);
    assert!(!s.kit_editor_open());

    // The panels over the window; the factory reset is shown, never run.
    for command in [Command::Shortcuts, Command::About, Command::FactoryReset] {
        s.run(command);
        draw(&mut s);
        s.overlay_key(OverlayKey::Escape);
        assert!(!s.overlay_open());
    }
    s.run(Command::ImportMidi);
    draw(&mut s);
    s.close_overlay();
    s.open_history();
    draw(&mut s);
    s.overlay_key(OverlayKey::Escape);

    // Exports to a folder, then again over the files once confirmed.
    let exports = dir.join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    let all = [Export::PatternWav, Export::PatternMidi, Export::PatternJson, Export::KitMapping];
    for export in all {
        s.export_to(export, &exports);
        assert!(!s.message.starts_with("not exported"), "{}", s.message);
        let name = s.export_file_name(export).unwrap();
        assert!(exports.join(&name).is_file(), "{name} not written");
    }
    for export in all {
        s.export_to(export, &exports);
        assert!(s.overlay_open(), "{export:?} written over without asking");
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
    }
    // The MIDI file back in: a new pattern, shown.
    let count = s.editor.library().patterns.len();
    s.drop_path(&exports.join(s.export_file_name(Export::PatternMidi).unwrap()), false);
    assert_eq!(s.editor.library().patterns.len(), count + 1);
    assert_eq!(s.pattern_index, count);
    draw(&mut s);

    // Undone back to the library as it came; then stopped, and quit.
    while s.editor.history().next_undo().is_some() {
        s.run(Command::Undo);
    }
    let text = include_str!("../../ordrumbox-core/tests/fixtures/song.json");
    assert_eq!(s.editor.library().to_file_text(), Library::from_json(&json::parse(text).unwrap()).0.to_file_text());
    s.run(Command::PlayPattern);
    assert!(run(&mut s, &mut audio, 4000) > 0.0);
    s.run(Command::PlayPattern);
    run(&mut s, &mut audio, CALLBACK);
    assert!(!s.playing());
    assert!(!s.quit_requested());
    s.run(Command::Quit);
    assert!(s.quit_requested());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_knob_of_the_editors_is_undone() {
    let dir = scratch_dir("knobs");
    let (mut s, mut audio) = demo_session(&dir);
    s.run(Command::PlayPattern);
    // A note selected: the note editor's knobs are shown too.
    s.click(Hit::Cell { track: 0, beat: 0, step: 0 });
    if s.selected.is_none() {
        s.click(Hit::Cell { track: 0, beat: 0, step: 0 });
    }
    let start = s.editor.library().to_file_text();
    let (width, height, scale) = WINDOW;
    let mut turned = 0;
    for tab in 0..editors::TABS.len() as u8 {
        for sub_tab in 0..4 {
            s.editors.tab = tab;
            (s.editors.fx_tab, s.editors.generation_tab, s.editors.lfo_target, s.editors.note_tab) =
                (sub_tab, sub_tab % 3, sub_tab, sub_tab);
            let knobs: Vec<Id> = s
                .panel_layout(width, height, scale)
                .items
                .iter()
                .filter(|item| matches!(item.kind, Kind::Value { .. }))
                .filter_map(|item| item.id)
                .collect();
            for id in knobs {
                // Where it is now: an edit can move or hide the next ones.
                let Some(item) = panel::find(&s.panel_layout(width, height, scale).items, id).cloned() else {
                    continue;
                };
                let (x, y) = (item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2);
                // The mouse over it (the tooltip), the wheel, then a drag.
                s.grid_hover(WINDOW, x, y);
                assert!(s.wheel(WINDOW, x, y, 1, false, false));
                s.mouse_down(WINDOW, x, y, Button::Left, false);
                s.mouse_move(WINDOW, x + 10, y - 10, false);
                s.mouse_up();
                turned += 1;
            }
            run(&mut s, &mut audio, CALLBACK);
            draw(&mut s);
            // Undone, the knobs of this tab leave the library as it was
            // (fewer steps than the history keeps).
            assert!(s.editor.history().undo_len() < MAX_STEPS);
            while s.editor.history().next_undo().is_some() {
                s.run(Command::Undo);
            }
            assert_eq!(s.editor.library().to_file_text(), start, "tab {tab}, sub-tab {sub_tab}");
        }
    }
    assert!(turned > 40, "only {turned} knobs shown");
    // The kit editor's knobs: the sample's settings, not the library's.
    s.run(Command::KitEditor);
    let layout = s.panel_layout(width, height, scale);
    let kit_knobs: Vec<Rect> =
        layout.items.iter().filter(|i| matches!(i.id, Some(Id::KitKnob(_)))).map(|i| i.rect).collect();
    assert!(!kit_knobs.is_empty());
    for rect in kit_knobs {
        assert!(s.wheel(WINDOW, rect.x + rect.w / 2, rect.y + rect.h / 2, 1, false, false));
    }
    assert!(s.kits_dirty.is_some(), "the kit edits wait to be saved");
    assert_eq!(s.editor.library().to_file_text(), start);
    draw(&mut s);
    std::fs::remove_dir_all(&dir).unwrap();
}
