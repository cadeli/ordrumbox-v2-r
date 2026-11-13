//! A whole session, through the menus, and every knob of the editors undone.

use super::*;

// ─── A whole session ────────────────────────────────────────────────────────

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
    let tempo = |s: &Session| (s.editor.library().bpm, s.pattern().unwrap().beat_count);
    let (bpm, beats) = tempo(&s);
    s.run(Command::TempoUp);
    s.run(Command::LengthUp);
    assert_eq!(tempo(&s), (bpm + 1.0, beats + 1));
    s.run(Command::TempoDown);
    s.run(Command::LengthDown);
    assert_eq!(tempo(&s), (bpm, beats));
    draw(&mut s);

    // The cursor and the step it is on: a note in, the grid and loop of its track changed.
    s.move_cursor(Move::Down);
    s.move_cursor(Move::Right);
    s.move_cursor(Move::Left);
    s.move_cursor(Move::Down);
    let c = s.selection.cursor.unwrap();
    let notes = |s: &Session| s.pattern().unwrap().tracks[c.track].notes.len();
    let before = notes(&s);
    if s.selection.selected.is_some() {
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
    assert!(!s.confirm_open(), "Ctrl+Z undoes it: not asked");
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
    for command in [
        Command::SongView,
        Command::PianoRoll,
        Command::SynthEditor,
        Command::Grid,
        Command::TrackEditor,
        Command::NoteEditor,
        Command::MasterStrip,
    ] {
        s.run(command);
        run(&mut s, &mut audio, CALLBACK);
        draw(&mut s);
    }
    s.run(Command::TrackEditor);
    s.show_generation_tab();
    assert!(s.chrome.show_track_editor);
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
    s.overlay_key(OverlayKey::Move(1));
    s.overlay_key(OverlayKey::Enter);
    assert_eq!(s.kit_list(), Some((0, KIT_KEYS.len() - 1)));
    s.drop_path(Path::new("notes.txt"), false);
    assert_eq!(s.message, "notes.txt: not a MIDI, WAV, song or kit file");
    s.run(Command::KitEditor);
    assert!(!s.kit_editor_open());

    // The kit editor names the tracks that play each sample.
    s.run(Command::KitEditor);
    let users = s.kit_view(0).map(|k| k.users).unwrap_or_default();
    assert!(users.iter().any(|u| !u.is_empty()), "{users:?}");
    s.run(Command::KitEditor);

    // The shortcuts, longer than the window, scroll.
    s.run(Command::Shortcuts);
    let rows = crate::view::notice_rows(WINDOW.1, WINDOW.2);
    assert!(rows < s.shortcut_lines().len());
    s.overlay_key(OverlayKey::Edge(true));
    assert!(
        matches!(s.chrome.overlay, Some(crate::session::Overlay::Shortcuts { scroll }) if scroll == s.shortcut_lines().len() - rows)
    );
    s.overlay_key(OverlayKey::Move(-2));
    assert!(
        matches!(s.chrome.overlay, Some(crate::session::Overlay::Shortcuts { scroll }) if scroll == s.shortcut_lines().len() - rows - 2)
    );
    draw(&mut s);
    s.overlay_key(OverlayKey::Escape);

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
    let text = include_str!("../../../ordrumbox-core/tests/fixtures/song.json");
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
    s.select_step_note(0, 0, 0);
    if s.selection.selected.is_none() {
        s.click(Hit::Cell { track: 0, beat: 0, step: 0 });
    }
    let start = s.editor.library().to_file_text();
    let (width, height, scale) = WINDOW;
    let mut turned = 0;
    for tab in 0..editors::TABS.len() as u8 {
        for sub_tab in 0..4 {
            s.editors.tab = tab;
            (s.editors.generation_tab, s.editors.note_tab) = (sub_tab % 3, sub_tab);
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
    assert!(s.saving.kits.pending(), "the kit edits wait to be saved");
    assert_eq!(s.editor.library().to_file_text(), start);
    draw(&mut s);
    std::fs::remove_dir_all(&dir).unwrap();
}
