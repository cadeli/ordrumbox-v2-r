//! Tests of menu.

use super::*;

use crate::canvas::char_width;

use crate::scale::Scale;

fn state() -> MenuState {
    MenuState {
        has_pattern: true,
        kits: vec!["real".into(), "808".into()],
        undo: Some("Add note".into()),
        ..MenuState::default()
    }
}

#[test]
fn the_library_keeps_its_place_right_of_the_titles() {
    let menus = menus(&state());
    let titles_end = titles(&menus, Scale::ONE).last().map(|&(x, w)| x + w).unwrap();
    for width in [200, 400, 700, 1100, 2000] {
        let (x, field) = library_field(&menus, width, Scale::ONE);
        assert!(field >= 0 && x + field <= width, "{width}: {x} {field}");
        assert!(field == 0 || x > titles_end, "{width}: over the titles");
    }
    // Wide enough: the full field, whatever the name and the state.
    let (_, field) = library_field(&menus, 1100, Scale::ONE);
    assert_eq!(field, LIBRARY_CHARS * char_width(Scale::ONE));
}

#[test]
fn keyboard_skips_what_cannot_run() {
    let menus = menus(&state());
    let edit = menus.iter().position(|m| m.title == "Edit").unwrap();
    let open = Open::new(&menus, edit, true);
    assert_eq!(open.command(&menus), Some(Command::Undo));
    // Redo, copy and paste cannot run: Select all is the next, then Preferences, then MIDI controls, then Undo again.
    assert_eq!(open.step(&menus, 1).command(&menus), Some(Command::SelectAll));
    let preferences = open.step(&menus, 1).step(&menus, 1);
    assert_eq!(preferences.command(&menus), Some(Command::Preferences));
    let controls = preferences.step(&menus, 1);
    assert_eq!(controls.command(&menus), Some(Command::MidiControls));
    assert_eq!(controls.step(&menus, 1).command(&menus), Some(Command::Undo));
    let file = Open::new(&menus, 0, true);
    // Save, Save song and Load song are greyed out: the first item highlighted is Save song as.
    assert_eq!(file.command(&menus), Some(Command::SaveSongAs));
    assert_eq!(file.step(&menus, 1).command(&menus), Some(Command::ExportSong));
    assert_eq!(file.switch(&menus, -1).menu, menus.len() - 1);
}

#[test]
fn the_song_and_file_menus_have_the_song_panel_commands() {
    let menus = menus(&MenuState { has_songs: true, can_save_song: true, ..state() });
    let runnable = |title: &str| -> Vec<Command> {
        menus.iter().find(|m| m.title == title).unwrap().items.iter().filter_map(Item::runnable).collect()
    };
    let (song, file) = (runnable("Song"), runnable("File"));
    use Command as C;
    // Play song lives in the Play menu only.
    for command in [C::SongView, C::NewSong, C::RemoveSong, C::PreviousSong, C::NextSong] {
        assert!(song.contains(&command), "{command:?}");
    }
    assert!(song.contains(&C::RenameSong) && song.contains(&C::SongDescription));
    assert!(!song.contains(&C::PlaySong) && runnable("Play").contains(&C::PlaySong));
    for command in [C::SaveSong, C::SaveSongAs, C::ExportSong, C::ImportSong] {
        assert!(file.contains(&command), "{command:?}");
    }
    // Load needs saved songs.
    assert!(!file.contains(&C::LoadSong));
}

#[test]
fn every_file_command_is_in_the_file_menu() {
    let all = MenuState { can_save_song: true, saved_songs: true, kit_sample: true, ..state() };
    let menus = menus(&MenuState { has_demo: true, user_folder: true, ..all });
    use Command as C;
    let file_commands = [
        C::SaveSong,
        C::SaveSongAs,
        C::LoadSong,
        C::ExportSong,
        C::ImportSong,
        C::ImportMidi,
        C::ExportMidi,
        C::ExportLibrary,
        C::ImportLibrary,
        C::DemoLibrary,
        C::BackUpUserData,
        C::RestoreUserData,
        C::ChangeUserFolder,
        C::DefaultUserFolder,
        C::OpenUserFolder,
    ];
    for menu in &menus {
        for item in &menu.items {
            if let Item::Command { command, .. } = item
                && file_commands.contains(command)
            {
                assert_eq!(menu.title, "File", "{command:?}");
            }
        }
    }
    let file: Vec<Command> = menus[0].items.iter().filter_map(Item::runnable).collect();
    for command in file_commands {
        assert!(file.contains(&command), "{command:?}");
    }
}

#[test]
fn the_kit_menu_has_the_track_sound_commands() {
    let menus = menus(&state());
    let kit = menus.iter().find(|m| m.title == "Kit").unwrap();
    let commands: Vec<Command> = kit.items.iter().filter_map(Item::runnable).collect();
    use Command as C;
    for command in [C::AutoAssignAll, C::ConvertToSynth, C::ImportFolder] {
        assert!(commands.contains(&command), "{command:?}");
    }
}

#[test]
fn the_kit_menu_has_the_sample_and_mapping_commands() {
    let all = MenuState { kit_sample: true, ..state() };
    let kit = menus(&all).into_iter().find(|m| m.title == "Kit").unwrap();
    let commands: Vec<Command> = kit.items.iter().filter_map(Item::runnable).collect();
    use Command as C;
    for command in [C::AddSample, C::ReplaceSample, C::LoadMapping, C::ExportMapping, C::ExportKitFolder] {
        assert!(commands.contains(&command), "{command:?}");
    }
    // Not in the File menu any more.
    let file: Vec<Command> = menus(&all)[0].items.iter().filter_map(Item::runnable).collect();
    assert!(!file.contains(&C::AddSample));
}

#[test]
fn a_plugin_menu_drops_what_it_leaves_to_the_host_or_the_app() {
    let state = MenuState { can_save: true, has_songs: true, hosted: true, ..state() };
    let menus = menus(&state);
    let app = super::menus(&MenuState { hosted: false, ..state.clone() });
    // A whole menu whose every command is refused is dropped (the Song menu).
    assert!(!menus.iter().any(|m| m.title == "Song"), "the Song menu too");
    assert_eq!(menus.len(), app.len() - 1);
    for menu in &menus {
        let app_menu = app.iter().find(|m| m.title == menu.title).unwrap();
        for item in &menu.items {
            let Item::Command { shortcut, command, .. } = item else { continue };
            // Nothing a plugin refuses is left in the menus.
            assert!(command.hosted_refusal().is_none(), "{command:?}");
            assert!(!shortcut.starts_with(CONTROL_PREFIX) && !shortcut.contains("Space"));
            // The item existed in the app's menu.
            assert!(
                app_menu.items.iter().any(|i| matches!(i, Item::Command { command: c, .. } if *c == *command)),
                "{command:?}"
            );
        }
    }
    let item = |command| {
        menus.iter().flat_map(|m| &m.items).find_map(|i| match i {
            Item::Command { shortcut, command: c, enabled, .. } if *c == command => Some((shortcut.as_ref(), *enabled)),
            _ => None,
        })
    };
    // Kept commands show their Alt shortcut.
    assert_eq!(item(Command::KitEditor), Some(("Alt+K", true)));
    assert_eq!(item(Command::Undo), Some(("Alt+Z", true)));
    assert_eq!(item(Command::Shortcuts), Some(("F1", true)));
    // Dropped commands are nowhere to be found.
    assert!(item(Command::Save).is_none());
    assert!(item(Command::SongView).is_none());
    assert!(item(Command::PlayPattern).is_none(), "the host's transport plays");
    assert!(item(Command::TempoUp).is_none());
}

#[test]
fn the_help_menu_has_the_factory_reset_command() {
    let menus = menus(&state());
    let help = menus.iter().find(|m| m.title == "Help").unwrap();
    let commands: Vec<Command> = help.items.iter().filter_map(Item::runnable).collect();
    assert!(commands.contains(&Command::FactoryReset));
}
