use super::*;
use crate::generate::MelodicKind;
use ordrumbox_core::note::ACCENTED_VELOCITY;
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_core::{Note, Scales, Styles};

#[test]
fn an_accent_toggles_on_the_notes_of_a_step_in_one_undo_step() {
    let mut e = editor();
    let velocity = |e: &Editor| e.library().patterns[0].tracks[0].notes[0].velocity;
    assert_eq!(e.toggle_accent(0, 0, 0, 0), Some(true));
    assert_eq!(velocity(&e), ACCENTED_VELOCITY);
    assert_eq!(e.toggle_accent(0, 0, 0, 0), Some(false));
    assert_eq!(velocity(&e), Note::default().velocity);
    assert_eq!(e.toggle_accent(0, 0, 1, 0), None, "no note on that step");
    assert_eq!(e.toggle_accent(0, 0, 0, 0), Some(true));
    assert_eq!(e.undo().as_deref(), Some("Accent on KICK"));
    assert_eq!(velocity(&e), Note::default().velocity);
}

#[test]
fn a_track_added_or_renamed_as_a_bass_is_mono() {
    let mut e = editor();
    let t = e.add_track(0, "BASS", 4).unwrap();
    assert!(pattern(&e).tracks[t].mono);
    assert!(!pattern(&e).tracks[1].mono);
    assert!(e.change_track_name(0, 1, "SUB BASS"));
    assert!(pattern(&e).tracks[1].mono);
    assert!(e.change_track_name(0, 1, "SNARE"));
    assert!(pattern(&e).tracks[1].mono, "renaming away from a bass keeps mono");
    e.undo();
    e.undo();
    assert!(!pattern(&e).tracks[1].mono);
}

fn editor() -> Editor {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[
            {"name":"KICK","notes":[{"beat":0},{"beat":2}]},
            {"name":"SNARE","stepsPerBeat":8,"loopAtStep":32,"notes":[{"beat":1,"step":7}]}]}]}"#,
    )
    .unwrap();
    let mut editor = Editor::new(library);
    editor.set_clock(Some(0.0));
    editor
}

fn pattern(editor: &Editor) -> &Pattern {
    &editor.library().patterns[0]
}

#[test]
fn add_and_delete_notes_undo_redo() {
    let mut e = editor();
    let original = pattern(&e).clone();
    assert_eq!(e.add_note(0, 0, 1, 2, 0.0), Some(2));
    assert_eq!(e.history().next_undo(), Some("Add note on KICK in \"rock\""));
    assert!(e.delete_note(0, 0, 0, 0, 0.0));
    assert!(!e.delete_note(0, 0, 3, 3, 0.0));
    assert_eq!(pattern(&e).tracks[0].notes.len(), 2);
    assert_eq!(e.history().undo_len(), 2);
    e.undo();
    e.undo();
    assert_eq!(*pattern(&e), original);
    e.redo();
    assert_eq!(pattern(&e).tracks[0].notes.len(), 3);
    assert_eq!(e.revision(), 5);
}

#[test]
fn update_track_clamps_and_filters() {
    let mut e = editor();
    let updates = [
        ("velocity", Value::Number(-3.0)),
        ("scaleName", Value::from("major")),
        ("mute", Value::from("yes")),
        ("filterCutoff", Value::Number(50_000.0)),
    ];
    assert!(e.update_track(0, 0, &updates, None, false));
    let track = &pattern(&e).tracks[0];
    assert_eq!(
        (track.volume, track.filter_cutoff, track.mute, track.scale_name.as_deref()),
        (0.0, 20_000.0, false, None)
    );
    assert_eq!(e.history().next_undo(), Some("Update KICK"));
    // No change, no step.
    assert!(e.update_track(0, 0, &[("velocity", Value::Number(0.0))], None, false));
    assert_eq!(e.history().undo_len(), 1);
    assert!(e.toggle_track(0, 1, "mute"));
    assert!(pattern(&e).tracks[1].mute);
}

#[test]
fn coalesced_drag_is_one_step() {
    let mut e = editor();
    for (i, v) in [0.2, 0.3, 0.4].iter().enumerate() {
        e.set_clock(Some(i as f64 * 100.0));
        e.update_track(0, 0, &[("velocity", Value::Number(*v))], Some("Volume on KICK"), true);
    }
    assert_eq!(e.history().undo_len(), 1);
    e.undo();
    assert_eq!(pattern(&e).tracks[0].volume, 1.0);
}

#[test]
fn a_painted_lane_is_one_step() {
    let mut e = editor();
    assert!(e.set_note_values(0, 0, "velocity", &[(0, 0.5)], "Edit velocity", true));
    e.set_clock(Some(100.0));
    assert!(e.set_note_values(0, 0, "velocity", &[(0, 0.4), (1, 0.3), (9, 1.0)], "Edit velocity", true));
    assert!(e.set_note_values(0, 0, "retriggerCount", &[(1, 3.0)], "Edit retrigger", false));
    let notes = &pattern(&e).tracks[0].notes;
    assert_eq!((notes[0].velocity, notes[1].velocity, notes[1].retrigger_count), (0.4, 0.3, 3));
    // The same values: no change, no step.
    assert!(!e.set_note_values(0, 0, "velocity", &[(0, 0.4)], "Edit velocity", true));
    assert_eq!(e.history().undo_len(), 2);
    e.undo();
    e.undo();
    assert_eq!(pattern(&e).tracks[0].notes[1].velocity, 0.8);
}

#[test]
fn steps_per_beat_keeps_positions_by_percent() {
    let mut e = editor();
    // SNARE: beat 1 step 7 of 8 (88 %), loop at 32 > 4 beats × 3.
    assert!(e.set_steps_per_beat(0, 1, 3.0, false));
    let track = &pattern(&e).tracks[1];
    assert_eq!((track.notes[0].step, track.loop_length), (2, Some(12)));
    assert!(e.set_steps_per_beat(0, 1, 8.0, false));
    assert_eq!(pattern(&e).tracks[1].notes[0].step, 7, "back to its step, as stepPercent is kept");
    assert!(!e.set_steps_per_beat(0, 1, 8.0, false));
    assert!(e.incr_steps_per_beat(0, 1));
    assert_eq!(pattern(&e).tracks[1].steps_per_beat, 1);
}

#[test]
fn every_step_comes_back_after_going_down_to_one_and_up_again() {
    let mut e = editor();
    // SNARE at 8 steps per beat: a note on every step of beats 0 and 3.
    for beat in [0, 3] {
        for step in 0..8 {
            e.add_note(0, 1, beat, step, 0.0);
        }
    }
    let places = |e: &Editor| pattern(e).tracks[1].notes.iter().map(|n| (n.beat, n.step)).collect::<Vec<_>>();
    let original = places(&e);
    for grid in (1..8).rev() {
        assert!(e.set_steps_per_beat(0, 1, f64::from(grid), true));
    }
    assert!(places(&e).iter().all(|&(_, step)| step == 0), "one step per beat");
    // Saved and opened again at one step per beat: the positions are in the file.
    let (library, report) = Library::parse(&e.library().to_file_text()).unwrap();
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    let mut e = Editor::new(library);
    for _ in 1..8 {
        assert!(e.incr_steps_per_beat(0, 1));
    }
    assert_eq!(pattern(&e).tracks[1].steps_per_beat, 8);
    assert_eq!(places(&e), original);
}

#[test]
fn a_moved_note_forgets_its_place_in_the_beat() {
    let mut e = editor();
    // SNARE: beat 1 step 7 of 8, brought down to 1 step per beat, moved to beat 2.
    assert!(e.set_steps_per_beat(0, 1, 1.0, false));
    assert!(e.update_note(0, 1, 0, &[("beat", Value::from(2))], None, false));
    assert!(e.set_steps_per_beat(0, 1, 8.0, false));
    let note = &pattern(&e).tracks[1].notes[0];
    assert_eq!((note.beat, note.step), (2, 0), "where it was put, not at step 7");
}

#[test]
fn loop_point_and_clean() {
    let mut e = editor();
    assert!(e.shorten_loop(0, 0));
    assert_eq!(pattern(&e).tracks[0].loop_length, Some(15));
    e.set_steps_per_beat(0, 0, 1.0, false); // 4 steps
    for _ in 0..4 {
        e.shorten_loop(0, 0);
    }
    assert_eq!(pattern(&e).tracks[0].loop_length, Some(4), "wrapped from 1 to the track end");
    assert!(e.clean_pattern(0));
    assert!(pattern(&e).tracks.iter().all(|t| t.notes.is_empty()));
    assert_eq!(e.history().next_undo(), Some("Clean \"rock\""));
}

#[test]
fn a_full_pattern_takes_no_more_tracks() {
    let mut e = editor();
    while e.has_track_room(0) {
        e.add_track(0, "T", 4).expect("room");
    }
    assert_eq!(pattern(&e).tracks.len(), MAX_TRACKS);
    let kick = pattern(&e).tracks[0].clone();
    assert_eq!(e.add_track(0, "T", 4), None);
    assert_eq!(e.paste_track(0, 0, &kick), None);
}

#[test]
fn tracks_paste_and_remove() {
    let mut e = editor();
    let kick = pattern(&e).tracks[0].clone();
    assert_eq!(e.paste_track(0, 1, &kick), Some(1));
    assert_eq!(e.paste_track(0, 99, &kick), Some(3));
    let names: Vec<_> = pattern(&e).tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["KICK", "KICK copy", "SNARE", "KICK copy 2"]);
    assert!(e.remove_track(0, 0));
    assert_eq!(e.add_track(0, "CHH", 4), Some(3));
    let chh = &pattern(&e).tracks[3];
    assert_eq!(
        (chh.beat_count, chh.loop_length, chh.pan),
        (4, Some(16), ordrumbox_core::drum_taxonomy::pan_from_track_name("CHH"))
    );
}

#[test]
fn tracks_move_as_one_step() {
    let mut e = editor();
    let names = |e: &Editor| pattern(e).tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
    e.add_track(0, "CHH", 4).expect("room");
    assert!(!e.move_track(0, 1, 1));
    assert!(!e.move_track(0, 5, 0));
    assert!(e.move_track(0, 0, 99));
    assert_eq!(names(&e), ["SNARE", "CHH", "KICK"]);
    assert!(e.move_track(0, 1, 0));
    assert_eq!(names(&e), ["CHH", "SNARE", "KICK"]);
    assert_eq!(e.undo().as_deref(), Some("Move track CHH"));
    assert_eq!(names(&e), ["SNARE", "CHH", "KICK"]);
}

#[test]
fn patterns_move_as_one_step() {
    let mut e = editor();
    e.add_pattern(Some("verse"));
    e.add_pattern(Some("chorus"));
    let ids = |e: &Editor| e.library().patterns.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert!(!e.move_pattern(1, 1));
    assert!(!e.move_pattern(5, 0));
    assert!(e.move_pattern(0, 99));
    assert_eq!(ids(&e), ["verse", "chorus", "rock"]);
    assert!(e.move_pattern(1, 0));
    assert_eq!(ids(&e), ["chorus", "verse", "rock"]);
    assert_eq!(e.undo().as_deref(), Some("Move pattern \"chorus\""));
    assert_eq!(ids(&e), ["verse", "chorus", "rock"]);
}

#[test]
fn patterns() {
    let mut e = editor();
    assert!(!e.remove_pattern(0), "the last pattern stays");
    assert_eq!(e.add_pattern(None), 1);
    assert_eq!(e.add_pattern(Some("rock")), 2);
    let ids: Vec<_> = e.library().patterns.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["rock", "newpat-1", "rock-2"]);
    assert!(e.set_pattern_beat_count(0, 2.0));
    assert!(pattern(&e).tracks.iter().all(|t| t.beat_count == 2));
    assert_eq!(pattern(&e).tracks[1].loop_length, Some(16));
    assert!(e.set_pattern_beat_count(0, 40.0));
    assert_eq!(pattern(&e).beat_count, 4);
    // The tempo is the library's, from the file's first pattern.
    assert_eq!(e.library().bpm, 100.0);
    assert!(e.set_bpm(f64::NAN, false));
    assert_eq!(e.library().bpm, 120.0);
    assert!(!e.set_bpm(400.0, false) || e.library().bpm == 300.0);
    assert!(e.rename_pattern(0, "  "));
    assert_eq!(pattern(&e).name.as_deref(), Some("rock"));
    assert!(e.rename_pattern(0, "  Rock II "));
    assert_eq!((pattern(&e).id.as_str(), pattern(&e).name.as_deref()), ("rock", Some("Rock II")));
    assert_eq!(e.history().next_undo(), Some("Rename pattern → \"Rock II\""));
    e.undo();
    assert!(e.remove_pattern(1));
    e.undo();
    assert_eq!(e.library().patterns.len(), 3);
    assert_eq!(e.duplicate_pattern(0), Some(3));
    let copy = &e.library().patterns[3];
    assert_eq!((copy.id.as_str(), copy.name.as_deref(), copy.tracks.len()), ("rock-copy", Some("rock copy"), 2));
}

/// The JS undo loses note edits once a command restored copies of the
/// notes (removing the track, a steps-per-beat change): its closures then
/// edit detached objects. Steps holding patterns have no such link.
#[test]
fn note_edits_survive_track_level_undo() {
    let mut e = editor();
    e.update_note(0, 1, 0, &[("velocity", Value::Number(0.3))], None, false);
    e.set_steps_per_beat(0, 1, 4.0, false);
    e.remove_track(0, 1);
    while e.undo().is_some() {}
    assert_eq!(pattern(&e).tracks[1].notes[0].velocity, 0.8);
    while e.redo().is_some() {}
    assert_eq!(pattern(&e).tracks.len(), 1);
    e.undo();
    e.undo();
    assert_eq!(pattern(&e).tracks[1].notes[0].velocity, 0.3);
}

#[test]
fn nudges_and_paste_step() {
    let mut e = editor();
    assert!(e.nudge_note(0, 0, 0, Nudge::VelocityUp));
    assert!(e.nudge_note(0, 0, 0, Nudge::VelocityUp));
    assert_eq!(pattern(&e).tracks[0].notes[0].velocity, 0.9);
    assert_eq!(e.history().undo_len(), 1, "held nudges coalesce");
    assert_eq!(nudge_velocity(1.0, true), 1.0);
    assert_eq!(nudge_pitch(48.0, true, 0.0), 48.0);
    assert_eq!(nudge_pitch(2.4, false, 0.0), 1.0);
    let notes = pattern(&e).tracks[0].notes[..1].to_vec();
    assert!(e.paste_step_notes(0, 1, 3, 1, &notes));
    let pasted = pattern(&e).tracks[1].notes.last().unwrap().clone();
    assert_eq!((pasted.beat, pasted.step, pasted.velocity), (3, 1, 0.9));
    assert!(e.delete_step(0, 1, 3, 1));
}

fn styles() -> Styles {
    let text = include_str!("../../../../assets/data/styles.json");
    Styles::from_json(&ordrumbox_core::json::parse(text).unwrap()).0
}

#[test]
fn generation_commands() {
    let mut e = editor();
    let (scales, styles) = (Scales::default(), styles());
    let mut rng = ordrumbox_core::Rng::new(5);
    let mut rnd = || rng.next_f64();

    // B: a new generated pattern in the asked style, one undo step.
    let undo = e.history().undo_len();
    let p = e.generate_pattern(&scales, &styles, Some("rap"), &mut rnd);
    assert_eq!(p, 1);
    let generated = &e.library().patterns[p];
    assert!(generated.tracks.iter().any(|t| t.name == "KICK" && !t.notes.is_empty()));
    assert_eq!(generated.extra.get("autoGenre").and_then(|g| g.as_str()), Some("hiphop"));
    assert_eq!(e.history().undo_len(), undo + 1);
    assert_eq!(e.history().next_undo(), Some("Generate pattern \"NewPat_1\""));

    // ↻ Drum on: KICK and SNARE in auto mode.
    assert_eq!(e.toggle_drums(0, &scales, &styles, Some("funk"), &mut rnd), Some(true));
    assert!(pattern(&e).tracks.iter().all(|t| t.auto_mode));
    assert_eq!(e.history().next_undo(), Some("Generate drums"));
    // Funk lines vary loop after loop, with no undo step.
    let (undo, before) = (e.history().undo_len(), pattern(&e).clone());
    assert!(e.auto_loop(0, 1, &scales, &styles, &mut rnd));
    assert_eq!(e.history().undo_len(), undo);
    assert_ne!(*pattern(&e), before);
    // ↻ Drum again: auto off.
    assert_eq!(e.toggle_drums(0, &scales, &styles, None, &mut rnd), Some(false));
    assert!(pattern(&e).tracks.iter().all(|t| !t.auto_mode));
    assert!(!e.auto_loop(0, 2, &scales, &styles, &mut rnd));

    // ↻ on one track: a new line, one undo step.
    assert!(e.regenerate_track(0, 1, &scales, &styles, &mut rnd));
    assert_eq!(e.history().next_undo(), Some("Generate SNARE"));

    // ↻ Bass: a synth BASS track with a line, in auto mode.
    assert_eq!(e.toggle_melodic(0, MelodicKind::Bass, &scales, &styles, &mut rnd), Some(true));
    let bass = pattern(&e).tracks.last().unwrap();
    assert!(bass.name == "BASS" && bass.use_synth && bass.auto_mode && !bass.notes.is_empty());
    assert_eq!(e.history().next_undo(), Some("Generate bass"));
    e.undo();
    assert!(pattern(&e).tracks.iter().all(|t| t.name != "BASS"));

    assert!(e.randomize_track(0, 0, &mut rnd));
    assert_eq!(e.history().next_undo(), Some("Randomize KICK"));
    assert!(!pattern(&e).tracks[0].notes.is_empty());
}

#[test]
fn midi_import_is_one_step() {
    let mut e = editor();
    // Format 0, 96 PPQN: a kick (channel 10, note 36) on beats 1 and 3.
    let mut track = vec![0x00, 0x99, 36, 100, 0x18, 0x89, 36, 0, 0x81, 0x28, 0x99, 36, 64, 0x18, 0x89, 36, 0];
    track.extend([0x00, 0xff, 0x2f, 0x00]);
    let mut file = b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x00\x60MTrk".to_vec();
    file.extend((track.len() as u32).to_be_bytes());
    file.extend(track);
    let (index, message) = e.import_midi("groove.mid", &file, &[]).unwrap();
    assert_eq!((index, message.as_str()), (1, "MIDI imported: 1 track(s)"));
    let pattern = &e.library().patterns[1];
    assert_eq!((pattern.id.as_str(), pattern.name.as_deref(), pattern.beat_count), ("groove", Some("groove"), 32));
    let kick = &pattern.tracks[0];
    assert_eq!(kick.name, "KICK");
    let notes: Vec<_> = kick.notes.iter().map(|n| (n.beat, n.step, n.velocity)).collect();
    assert_eq!(notes, [(0, 0, 0.79), (2, 0, 0.5)]);
    assert_eq!(e.history().next_undo(), Some("Import MIDI file"));
    e.undo();
    assert_eq!(e.library().patterns.len(), 1);
    assert_eq!(e.import_midi("x.mid", b"nothing", &[]), Err("No MIDI notes found in file".into()));
}

#[test]
fn a_batch_is_one_undo_step() {
    let mut e = editor();
    e.add_note(0, 0, 3, 0, 0.0);
    let before = e.library().clone();
    let added = e.batch("Remote edit", |e| {
        let t = e.add_track(0, "HH", 4).unwrap();
        e.add_note(0, t, 0, 1, 0.0);
        e.set_bpm(140.0, false);
        e.add_song("verse");
        e.batch("inner", |e| e.set_pattern_tags(0, &[" rock ".into(), " ".into()]));
        t
    });
    assert_eq!(added, 2);
    assert_eq!(e.history().undo_len(), 2);
    assert_eq!(e.history().next_undo(), Some("Remote edit"));
    assert_eq!(pattern(&e).tags, ["rock"]);
    assert_eq!(e.selected_song(), 0);
    let after = e.library().clone();
    e.undo();
    assert_eq!(e.library(), &before);
    e.redo();
    assert_eq!(e.library(), &after);
    // A batch that changes nothing records nothing.
    e.batch("nothing", |e| e.set_bpm(140.0, false));
    assert_eq!(e.history().undo_len(), 2);
}

#[test]
fn song_settings() {
    let mut e = editor();
    assert!(e.set_song_settings(None, Some(90.0), None, None).is_err());
    e.add_song("verse");
    e.set_song_settings(None, Some(500.0), Some("intro"), Some(8.0)).unwrap();
    let song = &e.library().songs[0];
    assert_eq!((song.bpm, song.description.as_str(), song.loop_measures), (300.0, "intro", Some(8.0)));
    e.set_song_settings(Some(0), None, None, Some(0.0)).unwrap();
    assert_eq!(e.library().songs[0].loop_measures, None);
    assert_eq!(e.library().songs[0].bpm, 300.0);
}

#[test]
fn a_song_is_renamed_in_one_step() {
    let mut e = editor();
    assert!(e.rename_song(None, "chorus").is_err());
    e.add_song("verse");
    assert_eq!(e.rename_song(None, "   "), Ok(false));
    assert_eq!(e.rename_song(Some(0), " chorus "), Ok(true));
    assert_eq!(e.rename_song(None, "chorus"), Ok(false));
    let song = &e.library().songs[0];
    assert_eq!((song.name.as_str(), song.id.as_str()), ("chorus", "verse"));
    assert_eq!(e.history().next_undo(), Some("Rename arrangement → \"chorus\""));
    e.undo();
    assert_eq!(e.library().songs[0].name, "verse");
}

#[test]
fn clips_are_placed_by_id_or_name_and_refused_with_a_reason() {
    let mut e = editor();
    let no_song = Err("No arrangement to edit — create a song first".to_owned());
    assert_eq!(e.add_pattern_at_measure("rock", 0, None, None), no_song);
    e.add_song("verse");

    let clip = e.add_pattern_at_measure("  ROCK ", 2, Some(0.75), None).unwrap();
    assert_eq!((clip.pattern_id.as_str(), clip.start_measure, clip.measure_count), ("rock", 2, 0.75));
    let clip = e.add_pattern_at_measure("rock", 0, Some(0.0), None).unwrap();
    assert_eq!(clip.measure_count, 1.0, "no length given: the pattern's, 4 beats");
    assert_eq!(e.history().next_undo(), Some("Add \"rock\" at measure 1"));
    let steps = e.history().undo_len();
    assert_eq!(
        e.add_pattern_at_measure(" no such pattern ", 0, None, None),
        Err("No pattern named \"no such pattern\" in the library".to_owned())
    );
    assert_eq!(e.add_pattern_at_measure("", 0, None, None), Err("No pattern named \"\" in the library".to_owned()));
    assert_eq!(e.repeat_pattern_at_measure(998, None), Err("No clip at measure 999".to_owned()));
    assert_eq!(e.remove_pattern_at_measure(998, None), Err("No clip at measure 999".to_owned()));
    assert_eq!(e.remove_pattern_clips("ghost", None), Ok(Vec::new()));
    assert_eq!(e.remove_pattern_clips("  ", None), Err("No pattern given to remove".to_owned()));
    assert_eq!(e.history().undo_len(), steps, "a refused edit is no step");

    // Repeated right after itself, floored: 2 + 0.75 → measure 2.
    let repeat = e.repeat_pattern_at_measure(2, None).unwrap();
    assert_eq!((repeat.start_measure, repeat.measure_count), (2, 0.75));
    assert_eq!(e.library().songs[0].loop_measures, Some(2.75));

    // Indices out of range and repeated count once; none left: no step.
    assert!(e.remove_song_clips(None, &[0, 2, 2, 999]));
    assert_eq!(e.history().next_undo(), Some("Remove 2 clips"));
    assert!(!e.remove_song_clips(None, &[]));
    assert!(!e.remove_song_clips(None, &[5]));
    assert_eq!(e.library().songs[0].clips.len(), 1);
}

#[test]
fn arrangements_are_added_removed_and_selected_in_one_step() {
    let mut e = editor();
    assert_eq!(e.add_song("  New one "), 0);
    assert_eq!(e.add_song(""), 1);
    assert_eq!(e.add_song("Untitled"), 2);
    let songs: Vec<_> = e.library().songs.iter().map(|s| (s.id.as_str(), s.name.as_str())).collect();
    assert_eq!(songs, [("new-one", "New one"), ("untitled", "Untitled"), ("untitled-2", "Untitled")]);
    assert_eq!(e.selected_song(), 2, "a new arrangement is selected");
    assert_eq!(e.set_selected_song(99), 2, "clamped to the list");

    assert!(!e.remove_song(Some(42)));
    assert!(e.remove_song(Some(0)));
    assert_eq!(e.selected_song(), 1, "kept inside the list");
    assert!(e.remove_song(None));
    assert_eq!((e.library().songs.len(), e.selected_song()), (1, 0));
    assert_eq!(e.history().next_undo(), Some("Remove arrangement \"Untitled\""));

    // Undo selects the arrangement it brings back, redo the one after.
    e.undo();
    assert_eq!((e.library().songs.len(), e.selected_song()), (2, 1));
    e.undo();
    assert_eq!(e.selected_song(), 0);
    e.redo();
    assert_eq!(e.selected_song(), 1);
}

#[test]
fn a_moved_clip_takes_the_loop_along_in_one_step() {
    let mut e = editor();
    e.add_song("verse");
    let clip = ordrumbox_core::SongClip { pattern_id: "a".into(), start_measure: 0, measure_count: 2.0 };
    assert!(e.add_song_clip(None, clip));
    assert!(!e.move_song_clip(None, 0, 0));
    assert!(!e.move_song_clip(None, 1, 3));
    assert!(e.move_song_clip(None, 0, 3));
    let song = &e.library().songs[0];
    assert_eq!((song.clips[0].start_measure, song.loop_measures), (3, Some(5.0)));
    e.undo();
    let song = &e.library().songs[0];
    assert_eq!((song.clips[0].start_measure, song.loop_measures), (0, Some(2.0)));
}

#[test]
fn synth_preset_edits_are_undone_and_a_knob_turned_is_one_step() {
    let mut e = Editor::new(Library::default());
    e.set_clock(Some(0.0));
    let set = |e: &mut Editor, gain: f64| {
        let preset = ordrumbox_core::json::parse(&format!(r#"{{"gain":{gain}}}"#)).unwrap();
        e.edit_synths("Edit \"a\"".into(), Some("synth:a".into()), |synths| {
            synths.insert("a", preset);
        })
    };
    assert!(!e.edit_synths("nothing".into(), None, |_| {}), "nothing changed");
    assert_eq!(e.library().synths, None);
    assert!(set(&mut e, 0.5));
    assert!(set(&mut e, 0.6));
    assert!(!set(&mut e, 0.6));
    assert_eq!(e.history().undo_len(), 1, "one step");
    e.undo();
    assert_eq!(e.library().synths, None);
    e.redo();
    assert_eq!(
        e.library().synths.as_ref().and_then(|s| s.get("a")).map(Value::to_json).as_deref(),
        Some(r#"{"gain":0.6}"#)
    );
    // A song loaded keeps the presets; a library with its own brings them.
    e.load_library(Library::default(), 0);
    assert!(e.library().synths.is_some());
    e.replace_library(Library { synths: Some(Object::new()), ..Library::default() }, 0, "import");
    assert_eq!(e.library().synths, Some(Object::new()));
    e.undo();
    assert!(e.library().synths.as_ref().is_some_and(|s| s.get("a").is_some()));
}
