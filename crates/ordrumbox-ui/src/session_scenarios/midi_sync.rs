//! A MIDI controller's knobs (learned, heard, kept) and a MIDI clock followed.

use ordrumbox_engine::{AudioSide, HostTransport};

use super::super::Session;
use super::{CALLBACK, LIBRARY, RATE, assert_pass_frames, position, run, session};
use crate::menu::Command;
use crate::session::OverlayKey;
use crate::test_support::scratch_dir;

/// Bind controller `controller` (channel 1) to the control `moves` choices
/// along the modal's list (the master is the first).
fn learn(s: &mut Session, moves: isize, controller: u8) {
    s.run(Command::MidiControls);
    assert!(s.overlay_open());
    s.overlay_key(OverlayKey::Side(moves));
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Enter);
    assert!(s.midi_control(0, controller, 99), "learning shows");
    assert!(s.message.contains(&format!("CC {controller}, channel 1")), "{}", s.message);
    s.overlay_key(OverlayKey::Escape);
}

#[test]
fn a_knob_learned_moves_the_master_volume_and_a_stranger_does_nothing() {
    let (mut s, mut audio) = session(LIBRARY);
    s.saving.settings_file = Some(scratch_dir("controls-master").join("settings.json"));
    learn(&mut s, 0, 7);
    s.toggle_play();
    assert!(run(&mut s, &mut audio, 4000) > 0.0);
    assert!(!s.midi_control(0, 7, 0), "nothing shown changes");
    run(&mut s, &mut audio, 2000);
    assert!(run(&mut s, &mut audio, 14_000) < 1e-9, "the master knob is down");
    s.midi_control(0, 99, 127);
    s.midi_control(1, 7, 127);
    assert!(run(&mut s, &mut audio, 14_000) < 1e-9, "another controller, another channel: not the knob");
    s.midi_control(0, 7, 127);
    run(&mut s, &mut audio, 2000);
    // A whole loop of the pattern: its two kicks are 6857 frames apart.
    assert!(run(&mut s, &mut audio, 14_000) > 0.0, "back up");
}

#[test]
fn a_mute_knob_shows_on_the_track_and_the_bindings_are_kept_in_the_settings() {
    let dir = scratch_dir("controls-mute");
    let (mut s, mut audio) = session(LIBRARY);
    s.saving.settings_file = Some(dir.join("settings.json"));
    // Master, then the first track's volume, pan, cutoff, mute.
    learn(&mut s, 4, 22);
    s.toggle_play();
    assert!(s.midi_control(0, 22, 127), "the mute box shows it");
    assert_eq!(s.host.muted, 1);
    run(&mut s, &mut audio, 2000);
    assert!(run(&mut s, &mut audio, 14_000) < 1e-9, "muted");
    assert!(s.midi_control(0, 22, 0));
    assert_eq!(s.host.muted, 0);

    let (mut again, _audio) = session(LIBRARY);
    again.saving.settings_file = Some(dir.join("settings.json"));
    again.load_controls();
    assert!(again.status.is_empty(), "{:?}", again.status);
    assert!(again.midi_control(0, 22, 127), "the same knob, in a new session");
}

#[test]
fn clear_lets_a_knob_go_and_a_bad_setting_is_reported() {
    let dir = scratch_dir("controls-clear");
    let (mut s, _audio) = session(LIBRARY);
    s.saving.settings_file = Some(dir.join("settings.json"));
    learn(&mut s, 4, 22);
    s.midi_control(0, 22, 127);
    assert_eq!(s.host.muted, 1);
    s.run(Command::MidiControls);
    s.overlay_key(OverlayKey::Side(4));
    s.overlay_key(OverlayKey::Move(2));
    s.overlay_key(OverlayKey::Enter);
    assert!(s.message.ends_with("knob let go"), "{}", s.message);
    assert_eq!(s.host.muted, 0, "unmuted with it");
    assert!(!s.midi_control(0, 22, 127), "bound to nothing");

    std::fs::write(dir.join("settings.json"), r#"{"midiControls":[{"channel":99}]}"#).unwrap();
    let (mut bad, _audio) = session(LIBRARY);
    bad.saving.settings_file = Some(dir.join("settings.json"));
    bad.load_controls();
    assert!(bad.status.iter().any(|m| m.contains("midiControls") && m.contains("left out")), "{:?}", bad.status);
}

/// `frames` of playback to the transport of a MIDI clock at `bpm`, from
/// `beat`, playing or not; the beat it ends on.
fn follow(s: &mut Session, audio: &mut AudioSide, frames: usize, bpm: f64, beat: f64, playing: bool) -> f64 {
    let (mut l, mut r) = (vec![0.0; CALLBACK], vec![0.0; CALLBACK]);
    let mut beat = beat;
    for _ in 0..frames.div_ceil(CALLBACK) {
        s.update();
        audio.render_synced(&mut l, &mut r, HostTransport { playing, bpm, beat });
        if playing {
            beat += CALLBACK as f64 / f64::from(RATE) * bpm / 60.0;
        }
    }
    beat
}

#[test]
fn the_transport_follows_a_midi_clock_and_goes_back_to_its_own_tempo_after() {
    let (mut s, mut audio) = session(LIBRARY);
    s.set_clock_follow(true);
    assert!(s.follows_clock());
    // The other machine has not started: nothing plays, a cue waits.
    follow(&mut s, &mut audio, 1000, 150.0, 0.0, false);
    assert!(!s.playing());
    // It starts, at 150 where the pattern says 140.
    s.set_clock_running(true);
    let beat = follow(&mut s, &mut audio, 4000, 150.0, 0.0, true);
    assert!(s.playing(), "{:?}", position(&s));
    assert_pass_frames(&s, 4.0, 150.0);
    assert!(beat > 0.0);
    // And stops.
    follow(&mut s, &mut audio, 1000, 150.0, beat, false);
    assert!(!s.playing());
    s.set_clock_running(false);

    s.set_clock_follow(false);
    assert!(!s.follows_clock());
    s.toggle_play();
    run(&mut s, &mut audio, CALLBACK * 2);
    assert!(s.playing());
    assert_pass_frames(&s, 4.0, 140.0);
}
