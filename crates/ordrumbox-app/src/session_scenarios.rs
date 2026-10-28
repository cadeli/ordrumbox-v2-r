//! The Playwright scenarios of ordrumbox-v2 that go through the session:
//! `e2e/playback`, `playback_tempo`, `song-playback` (the view decides what
//! plays and at which tempo) and `pattern-lifecycle` (a pattern built in the
//! grid, saved, reloaded, exported to MIDI and WAV).
//!
//! The session gets a real [`Controller`]; its audio side is rendered here as
//! the audio callback would, with `update` before each callback as the UI does.

use std::sync::Arc;

use ordrumbox_core::{Library, Rng, Scales};
use ordrumbox_engine::kit::{BankSample, Instrument};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{AudioSide, Controller, Mode, Position, SampleBank, SampleBuffer, SynthPresets};
use ordrumbox_logic::midi::{self, PPQN, smf};

use super::{Session, Workspace};
use crate::kits::KitSet;
use crate::store;
use crate::view::Hit;

const RATE: f32 = 8000.0;
/// Frames per audio callback.
const CALLBACK: usize = 256;

fn bank(rate: u32) -> Arc<SampleBank> {
    let instrument = Instrument {
        key: "KICK".into(),
        display_name: "k".into(),
        url: "k".into(),
        decay: Some(100.0),
        ..Default::default()
    };
    let buffer = SampleBuffer::mono(vec![0.5; 400], rate);
    Arc::new(SampleBank { samples: vec![BankSample { kit: "x".into(), instrument, buffer: buffer.into() }] })
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
