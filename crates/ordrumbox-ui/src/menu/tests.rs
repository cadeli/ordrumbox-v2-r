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
    // Redo, copy and paste cannot run: Quantize is the next, then Humanize, Select all, Preferences, MIDI controls, then Undo again.
    assert_eq!(open.step(&menus, 1).command(&menus), Some(Command::Quantize));
    let humanize = open.step(&menus, 1).step(&menus, 1);
    assert_eq!(humanize.command(&menus), Some(Command::Humanize));
    assert_eq!(humanize.step(&menus, 1).command(&menus), Some(Command::SelectAll));
    let preferences = humanize.step(&menus, 1).step(&menus, 1);
    assert_eq!(preferences.command(&menus), Some(Command::Preferences));
    let controls = preferences.step(&menus, 1);
    assert_eq!(controls.command(&menus), Some(Command::MidiControls));
    assert_eq!(controls.step(&menus, 1).command(&menus), Some(Command::Undo));
    let file = Open::new(&menus, 0, true);
    // Save, Open project and Save project are greyed out: the first item highlighted is Save project as.
    assert_eq!(file.command(&menus), Some(Command::SaveProjectAs));
    assert_eq!(file.step(&menus, 1).command(&menus), Some(Command::ExportProject));
    assert_eq!(file.switch(&menus, -1).menu, menus.len() - 1);
}

#[test]
fn the_song_and_file_menus_have_the_song_panel_commands_and_the_project_ones() {
    let menus = menus(&MenuState { has_songs: true, can_save_project: true, ..state() });
    let runnable = |title: &str| -> Vec<Command> {
        menus.iter().find(|m| m.title == title).unwrap().items.iter().filter_map(Item::runnable).collect()
    };
    let (song, file) = (runnable("Song"), runnable("File"));
    use Command as C;
    // Play song lives in the Play menu only.
    for command in [C::SongView, C::NewSong, C::RemoveSong, C::PreviousSong, C::NextSong] {
        assert!(song.contains(&command), "{command:?}");
    }
    assert!(!song.contains(&C::PlaySong) && runnable("Play").contains(&C::PlaySong));
    for command in
        [C::SaveProject, C::SaveProjectAs, C::ExportProject, C::ImportProject, C::RenameProject, C::ProjectDescription]
    {
        assert!(file.contains(&command), "{command:?}");
    }
    // Open needs saved projects.
    assert!(!file.contains(&C::LoadProject));
}

#[test]
fn every_file_command_is_in_the_file_menu() {
    let all = MenuState { can_save_project: true, saved_projects: true, kit_sample: true, ..state() };
    let menus = menus(&MenuState { has_starter: true, user_folder: true, ..all });
    use Command as C;
    let file_commands = [
        C::SaveProject,
        C::SaveProjectAs,
        C::LoadProject,
        C::ExportProject,
        C::ImportProject,
        C::ImportMidi,
        C::ExportMidi,
        C::ExportLibrary,
        C::ImportLibrary,
        C::StarterLibrary,
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
fn the_help_menu_has_the_factory_reset_command() {
    let menus = menus(&state());
    let help = menus.iter().find(|m| m.title == "Help").unwrap();
    let commands: Vec<Command> = help.items.iter().filter_map(Item::runnable).collect();
    assert!(commands.contains(&Command::FactoryReset));
}
