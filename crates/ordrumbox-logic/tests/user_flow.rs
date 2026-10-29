//! The scenario tests of ordrumbox-v2 that run on the logic alone:
//! `tests/e2e_user_flow.test.js` (a beat built command by command, its
//! triggers and track settings, its flat notes, its MIDI file) and
//! `tests/wav_midi_roundtrip.test.js` (a pattern exported to MIDI and imported
//! back plays the same notes).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::json::Value;
use ordrumbox_core::steps::{TICK, absolute_step, step_to_tick};
use ordrumbox_core::{Hits, Library, Pattern, Rng, Scales, expand_hits};
use ordrumbox_logic::Editor;
use ordrumbox_logic::midi::{self, PPQN, smf};

/// MIDI ticks per engine tick (96 PPQN, 32 ticks a beat).
const MIDI_RATIO: i64 = PPQN as i64 / TICK as i64;

fn flat(pattern: &Pattern, pass: u32) -> Hits {
    expand_hits(pattern, pass, &mut Rng::new(1), &Scales::default())
}

/// Ticks of every flat note of track `t`.
fn ticks_of(flat: &Hits, t: usize) -> Vec<u32> {
    flat.iter().flat_map(|(&tick, notes)| notes.iter().filter(|n| n.track == t).map(move |_| tick)).collect()
}

fn note_on_ticks(bytes: &[u8]) -> Vec<i64> {
    let mut ticks: Vec<i64> = smf::parse(bytes).note_ons().iter().map(|n| n.tick).collect();
    ticks.sort_unstable();
    ticks
}

/// Flow 1: a pattern at 140 bpm, KICK four on the floor, SNARE backbeat,
/// HIHAT eighths, BASS with its own pitches and velocities.
fn build_beat(editor: &mut Editor) -> usize {
    let p = editor.add_pattern(Some("Beat"));
    assert!(editor.set_pattern_bpm(p, 140.0, false));
    for name in ["KICK", "SNARE", "HIHAT", "BASS"] {
        editor.add_track(p, name, 4).expect("a track");
    }
    for beat in 0..4 {
        editor.add_note(p, 0, beat, 0, 0.0);
        editor.add_note(p, 2, beat, 0, 0.0);
        editor.add_note(p, 2, beat, 2, 0.0);
    }
    editor.add_note(p, 1, 1, 0, 0.0);
    editor.add_note(p, 1, 3, 0, 0.0);
    for (beat, pitch, velocity) in [(0, 0.0, 1.0), (1, 7.0, 0.6), (2, 12.0, 0.4)] {
        let n = editor.add_note(p, 3, beat, 0, 0.0).expect("a note");
        editor.update_note(p, 3, n, &[("pitch", Value::from(pitch)), ("velocity", Value::from(velocity))], None, false);
    }
    p
}

#[test]
fn flow_1_a_beat_built_command_by_command() {
    let mut editor = Editor::new(Library::default());
    let p = build_beat(&mut editor);
    let pattern = &editor.library().patterns[p];
    assert_eq!(pattern.name.as_deref(), Some("Beat"));
    assert_eq!(pattern.bpm, 140.0);
    let tracks: Vec<(&str, usize)> = pattern.tracks.iter().map(|t| (t.name.as_str(), t.notes.len())).collect();
    assert_eq!(tracks, [("KICK", 4), ("SNARE", 2), ("HIHAT", 8), ("BASS", 3)]);
    let bass: Vec<(f64, f64)> = pattern.tracks[3].notes.iter().map(|n| (n.pitch, n.velocity)).collect();
    assert_eq!(bass, [(0.0, 1.0), (7.0, 0.6), (12.0, 0.4)]);
}

#[test]
fn flow_2_triggers_every_and_retrigger() {
    let mut editor = Editor::new(Library::default());
    let p = editor.add_pattern(Some("Triggers"));
    editor.add_track(p, "KICK", 4);
    editor.add_track(p, "SNARE", 4);
    editor.add_note(p, 0, 0, 0, 0.0);
    editor.add_note(p, 1, 1, 0, 0.0);
    // The kick every second pass, the snare repeated 4 times.
    editor.update_note(p, 0, 0, &[("every", Value::from(2.0)), ("pos", Value::from(0.0))], None, false);
    editor.update_note(p, 1, 0, &[("retriggerCount", Value::from(4.0)), ("rate", Value::from(2.0))], None, false);
    let pattern = &editor.library().patterns[p];
    let kick: Vec<usize> = (0..4).map(|pass| ticks_of(&flat(pattern, pass), 0).len()).collect();
    assert_eq!(kick, [1, 0, 1, 0]);
    assert_eq!(ticks_of(&flat(pattern, 0), 1).len(), 4);
}

#[test]
fn flow_3_track_settings_and_their_undo() {
    let mut editor = Editor::new(Library::default());
    let p = build_beat(&mut editor);
    let before = editor.library().to_file_text();
    assert!(editor.toggle_track(p, 0, "mute"));
    editor.update_track(p, 1, &[("velocity", Value::from(1.7)), ("pan", Value::from(-0.4))], None, false);
    assert!(editor.change_track_sound(p, 2, "kits/chh.wav"));
    assert!(editor.change_track_name(p, 3, "BASS2"));
    let tracks = &editor.library().patterns[p].tracks;
    assert!(tracks[0].mute);
    assert_eq!((tracks[1].volume, tracks[1].pan), (1.0, -0.4), "the velocity clamped to 1");
    assert_eq!((tracks[2].sample_path.as_str(), tracks[2].auto_assign), ("kits/chh.wav", false));
    assert_eq!(tracks[3].name, "BASS2");
    // The muted track is left out of what plays (here, its MIDI file).
    let bytes = midi::export(&editor.library().patterns[p], 1, &[], &mut Rng::new(1), &Scales::default());
    assert_eq!(smf::parse(&bytes).track_names[1..], ["SNARE", "HIHAT", "BASS2"]);
    for _ in 0..4 {
        editor.undo().expect("an undo step");
    }
    assert_eq!(editor.library().to_file_text(), before);
}

#[test]
fn flow_4_flat_notes_on_their_ticks() {
    let mut editor = Editor::new(Library::default());
    let p = build_beat(&mut editor);
    let flat = flat(&editor.library().patterns[p], 0);
    let beat = |b: u32| b * TICK;
    assert_eq!(ticks_of(&flat, 0), [beat(0), beat(1), beat(2), beat(3)]);
    assert_eq!(ticks_of(&flat, 1), [beat(1), beat(3)]);
    assert_eq!(ticks_of(&flat, 2).len(), 8);
    assert!(ticks_of(&flat, 2).iter().all(|t| t % (TICK / 2) == 0), "eighths");
    let bass: Vec<(f64, f64)> =
        flat.values().flatten().filter(|n| n.track == 3).map(|n| (n.pitch, n.velocity)).collect();
    assert_eq!(bass, [(0.0, 1.0), (7.0, 0.6), (12.0, 0.4)]);
}

#[test]
fn flow_5_the_midi_file_of_the_beat() {
    let mut editor = Editor::new(Library::default());
    let p = editor.add_pattern(Some("MIDI"));
    editor.add_track(p, "KICK", 4);
    editor.add_track(p, "SNARE", 4);
    for (t, beat) in [(0, 0), (0, 2), (1, 1), (1, 3)] {
        editor.add_note(p, t, beat, 0, 0.0);
    }
    editor.update_note(p, 0, 0, &[("velocity", Value::from(0.5))], None, false);
    let pattern = &editor.library().patterns[p];
    let export = |loops| midi::export(pattern, loops, &[], &mut Rng::new(1), &Scales::default());

    let bytes = export(1);
    let file = smf::parse(&bytes);
    assert_eq!(file.header.format, 1);
    assert_eq!(file.tracks.len(), 3, "the tempo track and one per instrument");
    let beat = i64::from(PPQN);
    assert_eq!(note_on_ticks(&bytes), [0, beat, 2 * beat, 3 * beat]);
    let first = file.note_ons().into_iter().find(|n| n.tick == 0).expect("the first kick");
    assert_eq!(first.velocity, 64, "0.5 × 127, rounded");
    // Two loops: every note twice, the second time a pattern later.
    let twice = note_on_ticks(&export(2));
    assert_eq!(twice.len(), 8);
    assert_eq!(twice[4..], [4 * beat, 5 * beat, 6 * beat, 7 * beat]);
}

// ─── wav_midi_roundtrip.test.js ─────────────────────────────────────────────

#[test]
fn a_pattern_exported_to_midi_and_imported_back_plays_the_same_notes() {
    // KICK with velocities, SNARE with repeats, CHH eighths: drums on channel 10.
    let (library, _) = Library::parse(
        r#"{"patterns":[{"name":"rt","bpm":120,"beatCount":4,"tracks":[
            {"name":"KICK","notes":[{"velocity":1},{"beat":2,"velocity":0.6}]},
            {"name":"SNARE","notes":[{"beat":1,"retriggerCount":2,"rate":8},{"beat":3}]},
            {"name":"CHH","notes":[{},{"beatStep":2},{"beat":1},{"beat":1,"beatStep":2},{"beat":2},{"beat":3}]}]}]}"#,
    )
    .unwrap();
    let pattern = &library.patterns[0];
    let bytes = midi::export(pattern, 1, &[], &mut Rng::new(1), &Scales::default());
    let imported = midi::import("rt.mid", &bytes, &[]).expect("imports");
    let back = &imported.patterns[0];
    let expected = flat(pattern, 0);

    for (t, track) in pattern.tracks.iter().enumerate() {
        let twin =
            back.tracks.iter().find(|b| b.name == track.name).unwrap_or_else(|| panic!("{} imported", track.name));
        // Where each note sounds, in MIDI ticks, and how loud.
        let mut want: Vec<(i64, i64)> = expected
            .iter()
            .flat_map(|(&tick, notes)| {
                notes
                    .iter()
                    .filter(|n| n.track == t)
                    .map(move |n| (i64::from(tick) * MIDI_RATIO, (n.velocity * 100.0).round() as i64))
            })
            .collect();
        let mut got: Vec<(i64, i64)> = twin
            .notes
            .iter()
            .map(|n| {
                let tick = step_to_tick(absolute_step(n.beat, n.step, twin.steps_per_beat), twin.steps_per_beat);
                (tick * MIDI_RATIO, (n.velocity * 100.0).round() as i64)
            })
            .collect();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, want, "{}", track.name);
        assert!(twin.notes.iter().all(|n| n.pitch == 0.0), "{}: drum notes keep their pitch", track.name);
    }
}
