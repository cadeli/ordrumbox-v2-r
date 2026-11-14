//! Pause, rewind, loop, metronome, recording, MIDI input and preferences.

use super::super::Session;
use super::{CALLBACK, LIBRARY, position, run, session};
use crate::menu::Command;

// ─── pause, rewind, loop, metronome, recording ─────────────────────────────

/// Frames of a song measure at 120 BPM.
const MEASURE: usize = 16_000;

#[test]
fn a_song_pauses_where_it_is_and_home_goes_back() {
    let (mut s, mut audio) = session(LIBRARY);
    s.toggle_song_view();
    s.toggle_play();
    run(&mut s, &mut audio, MEASURE * 5 / 2);
    s.toggle_play();
    assert!(!s.playing());
    assert_eq!(s.transport.song_cursor, 2, "paused in measure 3");
    assert!(s.message.starts_with("paused at measure 3"), "{}", s.message);
    // Space goes on from there.
    s.toggle_play();
    run(&mut s, &mut audio, CALLBACK);
    assert_eq!(position(&s).map(|p| p.pass), Some(2));
    // Home goes back to the first measure, and plays on from it.
    s.rewind();
    run(&mut s, &mut audio, CALLBACK);
    assert_eq!(position(&s).map(|p| p.pass), Some(0));
    assert!(s.playing());
    // Stopped, it only moves the cursor.
    s.toggle_play();
    s.transport.song_cursor = 3;
    s.rewind();
    assert!(!s.playing());
    assert_eq!(s.transport.song_cursor, 0);
}

#[test]
fn a_pattern_plays_again_from_its_first_beat_on_home() {
    let (mut s, mut audio) = session(LIBRARY);
    s.toggle_play();
    run(&mut s, &mut audio, 6000);
    assert!(position(&s).is_some_and(|p| p.frame > 4000));
    s.rewind();
    run(&mut s, &mut audio, CALLBACK);
    assert!(position(&s).is_some_and(|p| p.frame <= CALLBACK as u64 + 8), "{:?}", position(&s));
    assert!(s.playing());
}

#[test]
fn a_loop_keeps_the_song_in_its_measures_and_playing_on() {
    let (mut s, mut audio) = session(LIBRARY);
    s.toggle_song_view();
    s.set_song_cursor(1);
    s.loop_to(2);
    assert_eq!(s.song_loop_region(), Some((1, 3)));
    assert!(s.message.starts_with("loop: measures 2-3"), "{}", s.message);
    s.toggle_play();
    let mut heard = 0.0;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..(MEASURE * 7 / 1000) {
        heard += run(&mut s, &mut audio, 1000);
        let tick = s.playback.tick().expect("playing");
        let measure = s.song().unwrap().measure_at_tick(tick as f64, f64::from(ordrumbox_core::steps::TICK)).floor();
        seen.insert(measure as u32);
    }
    assert!(s.playing());
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), [1, 2], "only the looped measures play, again and again");
    assert!(heard > 0.0);
    // Cleared, the song goes on to its end.
    s.clear_loop();
    assert_eq!(s.song_loop_region(), None);
    s.clear_loop();
    assert!(s.message.starts_with("no loop"));
    // A loop of one song does not apply to another.
    s.loop_to(0);
    s.editor.add_song("other");
    s.select_song(1);
    assert_eq!(s.song_loop_region(), None);
}

#[test]
fn the_metronome_clicks_an_empty_pattern_and_stops_when_off() {
    let (mut s, mut audio) = session(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"}]}]}"#);
    s.toggle_play();
    assert_eq!(run(&mut s, &mut audio, 8000), 0.0, "an empty pattern is silent");
    s.run(Command::Metronome);
    assert!(s.transport.metronome && s.message == "metronome on");
    // The pass playing is rebuilt with the clicks, the next ones follow.
    assert!(run(&mut s, &mut audio, 16_000) > 0.0, "clicks");
    s.run(Command::Metronome);
    run(&mut s, &mut audio, 16_000);
    assert_eq!(run(&mut s, &mut audio, 8000), 0.0, "off again");
}

#[test]
fn recording_puts_a_note_on_the_step_the_pattern_plays() {
    let (mut s, mut audio) = session(r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK"}]}]}"#);
    // Not playing: told, nothing recorded.
    s.run(Command::Record);
    assert!(s.recording());
    s.record_note(0);
    assert!(s.message.contains("Space plays"), "{}", s.message);
    assert!(s.pattern().unwrap().tracks[0].notes.is_empty());
    s.toggle_play();
    // 4 beats of 4 steps at 120 BPM: a step is 2000 frames; step 4 (beat 1) is at 8000.
    run(&mut s, &mut audio, 8000);
    s.record_note(0);
    let notes: Vec<_> = s.pattern().unwrap().tracks[0].notes.iter().map(|n| (n.beat, n.step)).collect();
    assert_eq!(notes.len(), 1, "{}", s.message);
    assert!(s.message.starts_with("recorded KICK @"), "{}", s.message);
    // The same step again: told, not doubled.
    let before = s.pattern().unwrap().tracks[0].notes.len();
    s.record_note(0);
    assert_eq!(s.pattern().unwrap().tracks[0].notes.len(), before);
    assert!(s.message.contains("a note is there already"), "{}", s.message);
    // Off: nothing recorded.
    s.run(Command::Record);
    run(&mut s, &mut audio, 6000);
    let count = s.pattern().unwrap().tracks[0].notes.len();
    s.record_note(0);
    assert_eq!(s.pattern().unwrap().tracks[0].notes.len(), count);
}

#[test]
fn recording_off_the_grid_keeps_how_late_the_note_was_played() {
    let (mut s, mut audio) = session(r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK"}]}]}"#);
    s.run(Command::Record);
    s.toggle_play();
    // On the grid, the default: a note played between two steps goes on the nearest, unshifted.
    run(&mut s, &mut audio, 8500);
    s.record_note(0);
    let shifts = |s: &Session| s.pattern().unwrap().tracks[0].notes.iter().map(|n| n.shift_steps).collect::<Vec<_>>();
    assert_eq!(shifts(&s), [0.0], "{}", s.message);
    // Off the grid: the next one keeps its place between the steps.
    s.run(Command::RecordTiming);
    run(&mut s, &mut audio, 4000);
    s.record_note(0);
    let recorded = shifts(&s);
    assert_eq!(recorded.len(), 2, "{}", s.message);
    assert!(recorded[1] != 0.0 && recorded[1].abs() <= 0.5, "{recorded:?}");
    assert_eq!(recorded[1], (recorded[1] * 100.0).round() / 100.0, "two decimals, as it is saved");
}

#[test]
fn f11_asks_for_full_screen_and_ctrl_w_quits() {
    use crate::ui::Ui;
    use crate::ui_input::{Key, KeyEvent, Modifiers};
    let (s, _) = session(LIBRARY);
    let mut ui = Ui::new(s);
    let press = |ui: &mut Ui, modifiers: Modifiers, key: Key| {
        ui.set_modifiers(modifiers);
        let event = KeyEvent { key, pressed: true, repeat: false, text: None, unmodified: None };
        assert!(ui.key(&event), "{key:?}");
        ui.set_modifiers(Modifiers::default());
    };
    press(&mut ui, Modifiers::default(), Key::F(11));
    assert_eq!(ui.session.take_fullscreen_request(), Some(true));
    assert_eq!(ui.session.take_fullscreen_request(), None, "asked once");
    press(&mut ui, Modifiers::default(), Key::F(11));
    assert_eq!(ui.session.take_fullscreen_request(), Some(false));
    assert!(!ui.session.quit_requested());
    press(&mut ui, Modifiers { control: true, ..Modifiers::default() }, Key::Letter('W'));
    assert!(ui.session.quit_requested());
}

// ─── MIDI input, preferences ────────────────────────────────────────────────

#[test]
fn a_midi_note_is_recorded_on_the_track_that_key_plays_at_its_velocity() {
    let (mut s, mut audio) =
        session(r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK"},{"name":"SNARE"}]}]}"#);
    s.set_midi_open(true);
    // Not recording: heard, never written.
    s.midi_note(9, 36, 0.5);
    assert!(s.pattern().unwrap().tracks[0].notes.is_empty());
    s.run(Command::Record);
    s.toggle_play();
    run(&mut s, &mut audio, 8000);
    // GM 36 is the kick, 38 the snare.
    s.midi_note(9, 36, 0.5);
    let kick = &s.pattern().unwrap().tracks[0].notes;
    assert_eq!(kick.len(), 1, "{}", s.message);
    assert!((kick[0].velocity - 0.5).abs() < 1e-9);
    s.midi_note(9, 38, 1.0);
    assert_eq!(s.pattern().unwrap().tracks[1].notes.len(), 1);
    // A key no track plays is said.
    s.midi_note(9, 100, 1.0);
    assert!(s.message.contains("no track plays MIDI key 100"), "{}", s.message);
    s.undo();
    assert!(s.pattern().unwrap().tracks[1].notes.is_empty(), "one undo step a note");
}

#[test]
fn the_preferences_keep_what_ok_applies_and_esc_leaves() {
    use crate::session::OverlayKey;
    let (mut s, _audio) = session(LIBRARY);
    s.run(Command::Preferences);
    assert!(s.overlay_open());
    // Rows with no sound device or MIDI port offered: metronome, VU meters, toasts, OK.
    s.overlay_key(OverlayKey::Enter);
    assert!(!s.transport.metronome, "a draft: nothing changes before OK");
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Side(1));
    s.overlay_key(OverlayKey::Escape);
    assert!(!s.overlay_open() && !s.transport.metronome && s.chrome.show_vus, "Esc leaves everything");

    s.run(Command::Preferences);
    s.overlay_key(OverlayKey::Enter);
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Enter);
    s.overlay_key(OverlayKey::Move(2));
    s.overlay_key(OverlayKey::Enter);
    assert!(!s.overlay_open());
    assert!(s.transport.metronome && !s.chrome.show_vus);
    assert_eq!(s.message, "preferences kept");
}

#[test]
fn the_preferences_list_the_devices_and_ports_the_program_offers() {
    use crate::session::OverlayKey;
    let (mut s, _audio) = session(LIBRARY);
    s.offer_devices(|| vec!["Speakers".into()]);
    s.offer_midi_inputs(|| vec!["Pad".into(), "Keys".into()]);
    s.run(Command::Preferences);
    let lines = match &s.chrome.overlay {
        Some(crate::session::session_overlay::Overlay::Preferences(prefs)) => prefs.lines(),
        _ => panic!("not open"),
    };
    assert!(lines[0].starts_with("> Audio output"), "{lines:?}");
    assert!(lines[1].contains("MIDI input"), "{lines:?}");
    // The modal keeps its width whichever value is shown.
    let width = |s: &Session| match &s.chrome.overlay {
        Some(crate::session::session_overlay::Overlay::Preferences(prefs)) => {
            prefs.lines().iter().map(|l| l.chars().count()).collect::<Vec<_>>()
        }
        _ => vec![],
    };
    let before = width(&s);
    s.overlay_key(OverlayKey::Move(1));
    for _ in 0..4 {
        s.overlay_key(OverlayKey::Side(1));
        assert_eq!(width(&s), before, "no layout shift");
    }
    s.overlay_key(OverlayKey::Escape);
}

#[test]
fn a_midi_note_on_the_pattern_channel_leaves_the_pattern_playing_for_the_chosen_one_at_once() {
    let (mut s, mut audio) = session(
        r#"{"patterns":[{"name":"a","bpm":120,"tracks":[{"name":"KICK","notes":[{}]}]},
            {"name":"b","bpm":120,"beatCount":2,"tracks":[{"name":"KICK","notes":[{}]}]}]}"#,
    );
    s.toggle_play();
    run(&mut s, &mut audio, 3000);
    super::assert_pass_frames(&s, 4.0, 120.0);
    assert!(s.select_pattern_now(1));
    run(&mut s, &mut audio, CALLBACK * 2);
    assert!(s.playing());
    assert_eq!(s.pattern_index, 1);
    super::assert_pass_frames(&s, 2.0, 120.0);
    assert!(!s.select_pattern_now(9), "no such pattern");
    assert!(s.message.contains("no pattern 10"), "{}", s.message);
    assert_eq!(s.pattern_index, 1);
}

#[test]
fn quantize_takes_the_shift_off_the_notes_of_the_zone_or_track_and_undo_puts_it_back() {
    use crate::session::OverlayKey;
    let (mut s, _audio) = session(
        r#"{"patterns":[{"name":"p","bpm":120,"tracks":[
            {"name":"KICK","notes":[{"step":1,"shiftSteps":0.3},{"beat":1,"shiftSteps":-0.4}]},
            {"name":"SNARE","notes":[{"step":2,"shiftSteps":0.2}]}]}]}"#,
    );
    let shifts = |s: &Session, t: usize| -> Vec<f64> {
        s.pattern().unwrap().tracks[t].notes.iter().map(|n| n.shift_steps).collect()
    };
    // No cursor, no zone: the pattern. Half the way first: Strength is the second line.
    s.run(Command::Quantize);
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Side(-1));
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Enter);
    assert!(!s.overlay_open());
    assert!(s.message.contains("at 75%"), "{}", s.message);
    assert_eq!(shifts(&s, 1), [0.05]);
    // Then all the way, on the cursor's track only.
    s.selection.put_cursor(crate::view::Cursor { track: 0, beat: 0, step: 0 });
    s.run(Command::Quantize);
    s.overlay_key(OverlayKey::Move(2));
    s.overlay_key(OverlayKey::Enter);
    assert_eq!(shifts(&s, 0), [0.0, 0.0]);
    assert_eq!(shifts(&s, 1), [0.05], "the other track is left");
    s.undo();
    assert_eq!(shifts(&s, 0), [0.07, -0.1]);
}
