//! Tests of ui.

use super::*;

/// A `Ui` on `library`, not hosted by a plugin.
fn standalone_ui(library: ordrumbox_core::Library) -> Ui {
    Ui::new(Session::for_tests(library, false))
}

#[test]
fn shortcuts_follow_the_command_modifier() {
    assert_eq!(shortcut(Key::Letter('Z'), false, true), Some(Command::Undo));
    assert_eq!(shortcut(Key::Letter('Z'), true, true), Some(Command::Redo));
    assert_eq!(shortcut(Key::Letter('Z'), false, false), None, "on the play keys of AZERTY");
    assert_eq!(shortcut(Key::Letter('A'), false, false), None, "on the play keys of AZERTY");
    // Ctrl+- and Ctrl+= zoom, main row and keypad alike; the pattern length has no key.
    assert_eq!(shortcut(Key::Minus, false, true), Some(Command::ZoomOut));
    assert_eq!(shortcut(Key::Equal, false, true), Some(Command::ZoomIn));
    assert_eq!(shortcut(Key::NumpadSubtract, false, true), Some(Command::ZoomOut));
    assert_eq!(shortcut(Key::NumpadAdd, false, true), Some(Command::ZoomIn));
    assert_eq!(shortcut(Key::Minus, true, true), Some(Command::ZoomOut));
    assert_eq!(shortcut(Key::Equal, true, true), Some(Command::ZoomIn));
    assert_eq!(shortcut(Key::Digit(0), false, true), Some(Command::ZoomReset));
    // The views F1-F7 switch, on the keyboards whose F keys want Fn.
    for n in 1..=7 {
        assert_eq!(shortcut(Key::F(n), false, false), shortcut(Key::Digit(n), false, true), "F{n}");
    }
    assert_eq!(shortcut(Key::F(3), false, false), Some(Command::SongView));
    assert_eq!(shortcut(Key::Letter('L'), false, false), Some(Command::ToggleAccent));
    assert_eq!(shortcut(Key::Letter('S'), false, true), Some(Command::Save));
    assert_eq!(shortcut(Key::Letter('S'), true, true), Some(Command::ExportPatternJson));
    // Ctrl+O loads a song (O alone hears track 9); importing a MIDI file has no key.
    assert_eq!(shortcut(Key::Letter('O'), false, true), Some(Command::LoadSong));
    assert_eq!(shortcut(Key::Letter('O'), false, false), None);
    // Solo and mute of the focused track, as every DAW spells them.
    assert_eq!(shortcut(Key::Letter('S'), false, false), Some(Command::Solo));
    assert_eq!(shortcut(Key::Letter('M'), false, false), Some(Command::Mute));
    for (letter, command) in [
        ('B', Command::GeneratePattern),
        ('C', Command::NextColorScheme),
        ('D', Command::GenerateDrums),
        ('G', Command::SelectRandomDrumkit),
        ('H', Command::ConvertToSynth),
        ('J', Command::AutoAssignAll),
        ('K', Command::RandomSamples),
        ('V', Command::ToggleVus),
        ('X', Command::RandomTrack),
    ] {
        assert_eq!(shortcut(Key::Letter(letter), true, false), Some(command), "Shift+{letter}");
        assert_eq!(shortcut(Key::Letter(letter), false, false), None, "{letter} alone");
        assert!(shift_hint(Key::Letter(letter)).is_some(), "{letter} says to hold Shift");
    }
}

#[test]
fn the_play_rows_stay_put_and_other_keys_follow_the_layout() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    let press = |key, c| KeyEvent { key, pressed: true, repeat: false, text: None, unmodified: Some(c) };
    let bpm = ui.session.tempo();
    // AZERTY's `-` is on the 6: alone, it mutes track 6; with Ctrl, it zooms out.
    assert!(ui.key(&press(Key::Digit(6), '-')));
    assert_eq!(ui.session.tempo(), bpm);
    ui.set_modifiers(Modifiers { control: true, ..Modifiers::default() });
    assert!(ui.key(&press(Key::Digit(6), '-')));
    assert_eq!(ui.session.message(), "zoom x1, the limit", "the zoom out ran, not the length");
    // QWERTZ's `-`, in the place of `/`: the tempo.
    ui.set_modifiers(Modifiers::default());
    assert!(ui.key(&press(Key::Other, '-')));
    assert_eq!(ui.session.tempo(), bpm - 1.0);
}

#[test]
fn a_zoom_bigger_than_the_window_scrolls_it() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    let (w, h) = view::min_window_size(Scale::ONE, view::Shown::all(true));
    ui.set_size(w, h, Scale::ONE);
    ui.session.run(Command::ZoomIn);
    ui.session.run(Command::ZoomIn);
    let mut pixels = vec![0; (w * h) as usize];
    ui.draw(&mut pixels, w as usize, h as usize);
    let viewport = ui.viewport((w, h));
    assert!(viewport.scrolls(), "the UI at ×1.5 is drawn whole, the window shows a part");
    assert_eq!(ui.session.wanted_scale(), Scale::from_quarters(6), "the zoom asked for, not one that fits");
    // The vertical bar's thumb, dragged to the bottom: the window shows the UI's bottom.
    let bar_x = f64::from(viewport.view.0 + 2);
    ui.pointer_moved(bar_x, 4.0);
    assert!(ui.pointer_pressed(Button::Left));
    assert!(ui.pointer_moved(bar_x, f64::from(h)));
    assert!(ui.pointer_released(Button::Left));
    ui.pointer_moved(0.0, 0.0);
    assert_eq!(ui.at().1, viewport.content.1 - viewport.view.1);
}

#[test]
fn a_plugin_leaves_the_host_its_keys() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let session = |hosted| Session::for_tests(library.clone(), hosted);
    let press = |key| KeyEvent { key, pressed: true, repeat: false, text: None, unmodified: None };
    let command = Modifiers { logo: true, ..Modifiers::default() };

    let mut app = Ui::new(session(false));
    assert!(app.key(&press(Key::Space)));
    app.set_modifiers(command);
    assert!(app.key(&press(Key::Letter('Z'))));

    let mut plugin = Ui::new(session(true));
    let bpm = plugin.session.tempo();
    // The host's: its transport, Ctrl/Cmd keys, what a plugin leaves out.
    for key in [Key::Space, Key::F(3), Key::Minus, Key::Equal, Key::BracketLeft] {
        let shift = key == Key::BracketLeft;
        plugin.set_modifiers(Modifiers { shift, ..Modifiers::default() });
        assert!(!plugin.key(&press(key)), "{key:?}");
    }
    plugin.set_modifiers(command);
    for key in [Key::Letter('S'), Key::Letter('Z'), Key::Letter('N'), Key::Minus] {
        assert!(!plugin.key(&press(key)), "Cmd+{key:?}");
    }
    assert!(!plugin.session.playing() && !plugin.session.song_view_open());
    assert_eq!(plugin.session.tempo(), bpm);
    assert_eq!(plugin.session.pattern().unwrap().beat_count, session(false).pattern().unwrap().beat_count);
    // The plugin's own keys stay its own.
    plugin.set_modifiers(Modifiers::default());
    assert!(plugin.key(&press(Key::Digit(1))));
    assert!(plugin.key(&press(Key::ArrowRight)));
    assert!(plugin.key(&press(Key::F(4))));
    // Alt stands for the Ctrl the host keeps, but not for what the host does.
    plugin.set_modifiers(Modifiers { alt: true, ..Modifiers::default() });
    let revision = plugin.session.library_revision();
    assert!(plugin.key(&press(Key::Letter('N'))));
    assert_ne!(plugin.session.library_revision(), revision, "a new pattern");
    assert!(plugin.key(&press(Key::Letter('K'))));
    assert!(plugin.session.kit_editor_open());
    assert!(!plugin.key(&press(Key::Letter('S'))), "the host saves");
    assert!(!plugin.key(&press(Key::Letter('Q'))), "the host closes");
}

#[test]
fn alt_alone_opens_the_menus_but_not_after_a_click_or_a_wheel_turn() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    let alt = |pressed| KeyEvent { key: Key::Alt, pressed, repeat: false, text: None, unmodified: None };

    ui.key(&alt(true));
    assert!(ui.key(&alt(false)));
    assert!(ui.session.open_menu().is_some(), "Alt alone");
    ui.key(&alt(true));
    ui.key(&alt(false));
    assert!(ui.session.open_menu().is_none(), "Alt alone again closes them");

    ui.key(&alt(true));
    ui.pointer_pressed(Button::Left);
    ui.pointer_released(Button::Left);
    assert!(!ui.key(&alt(false)));
    assert!(ui.session.open_menu().is_none(), "Alt+click");

    ui.key(&alt(true));
    ui.wheel(ScrollDelta::Lines(0.0, 1.0));
    assert!(!ui.key(&alt(false)));
    assert!(ui.session.open_menu().is_none(), "Alt+wheel");
}

#[test]
fn a_new_message_stands_out_even_the_same_words_again_and_then_goes_back() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    let refuse = |ui: &mut Ui| {
        ui.session.say("select a track first");
        false
    };

    assert!(ui.input(refuse), "a message is a redraw");
    let first = ui.session.flash_until().expect("stands out");
    assert!(ui.session.end_flash(first));
    assert_eq!(ui.session.flash_until(), None, "its time is over");

    // The same refusal again stands out again.
    assert!(ui.input(refuse));
    assert!(ui.session.flash_until().is_some_and(|until| until >= first));

    // An input that says nothing keeps the message, its toast left as it is.
    let until = ui.session.flash_until();
    assert!(!ui.input(|_| false));
    assert_eq!(ui.session.message(), "select a track first");
    assert_eq!(ui.session.flash_until(), until);

    // A message given outside an input (an MCP call) stands out at the next idle pass.
    ui.session.end_flash(Instant::now() + Duration::from_secs(60));
    ui.session.say("stopped");
    let idle = ui.idle();
    assert!(ui.session.flash_until().is_some());
    assert!(idle.wake_at.is_some(), "woken when the toast goes");
}

#[test]
fn with_toasts_off_a_message_only_goes_on_the_status_line() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    let refuse = |ui: &mut Ui| {
        ui.session.say("select a track first");
        false
    };
    ui.input(refuse);
    assert!(ui.session.flash_until().is_some());

    // Turning them off takes the ones on screen away too.
    ui.input(|ui| {
        ui.session.toggle_toasts();
        false
    });
    assert_eq!(ui.session.flash_until(), None);
    ui.input(refuse);
    assert_eq!(ui.session.flash_until(), None);
    assert_eq!(ui.session.message(), "select a track first");

    ui.input(|ui| {
        ui.session.toggle_toasts();
        false
    });
    assert_eq!(ui.session.toast_texts(), ["toasts on"]);
}

#[test]
fn a_click_on_the_toast_hides_it_at_once() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    ui.input(|ui| {
        ui.session.say("select a track first");
        false
    });
    assert!(ui.session.flash_until().is_some());
    let (width, height, scale) = ui.size();
    let toast = view::toast_rect(width, "select a track first", height - 2 * frame::margin(scale), scale);
    // Off the toast: it stays.
    assert!(!ui.session.toast_click((width, height, scale), toast.x - 1, toast.y));
    assert!(ui.session.flash_until().is_some());
    // On it: it goes, the message stays on the status line.
    let at = (toast.x + toast.w / 2, toast.y + toast.h / 2);
    assert!(ui.input(|ui| ui.session.toast_click((width, height, scale), at.0, at.1)));
    assert_eq!(ui.session.flash_until(), None);
    assert_eq!(ui.session.message(), "select a track first");
}

#[test]
fn toasts_stack_upwards_up_to_five_and_a_click_hides_one() {
    let library =
        ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
    let mut ui = standalone_ui(library);
    let say = |ui: &mut Ui, text: &'static str| {
        ui.input(|ui| {
            ui.session.say(text);
            false
        })
    };
    say(&mut ui, "one");
    say(&mut ui, "two");
    assert_eq!(ui.session.toast_texts(), ["one", "two"]);
    // The same words again get a toast of their own, which counts them.
    say(&mut ui, "two");
    say(&mut ui, "two");
    assert_eq!(ui.session.toast_texts(), ["one", "two", "two (×2)", "two (×3)"]);
    for text in ["three", "four", "five", "six"] {
        say(&mut ui, text);
    }
    assert_eq!(ui.session.toast_texts(), ["two (×3)", "three", "four", "five", "six"], "five at most");
    // Other words in between start the count again.
    say(&mut ui, "five");
    assert_eq!(ui.session.toast_texts(), ["three", "four", "five", "six", "five"]);

    // The newest at the bottom, the older ones above it, apart.
    let (width, height, scale) = ui.size();
    let bottom = height - 2 * frame::margin(scale);
    let newest = view::toast_rect(width, "five", bottom, scale);
    let above = view::toast_rect(width, "six", newest.y - view::toast_gap(scale), scale);
    assert_eq!(newest.y + newest.h, bottom);
    assert!(above.y + above.h < newest.y);

    // A click on the one above hides it alone.
    let at = (above.x + above.w / 2, above.y + above.h / 2);
    assert!(ui.input(|ui| ui.session.toast_click((width, height, scale), at.0, at.1)));
    assert_eq!(ui.session.toast_texts(), ["three", "four", "five", "five"]);

    // They go when their time is over.
    assert!(ui.session.end_flash(Instant::now() + Duration::from_secs(60)));
    assert!(ui.session.toast_texts().is_empty());
    assert_eq!(ui.session.flash_until(), None);
}
