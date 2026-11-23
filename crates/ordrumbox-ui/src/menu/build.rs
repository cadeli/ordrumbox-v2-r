//! The menus' content: which items each menu holds, how they read the
//! state, and what a plugin keeps of them.

use super::{Command, Item, Menu, MenuState};
use crate::ui_input::MACOS;
use std::borrow::Cow;

fn command(label: impl Into<String>, shortcut: &'static str, command: Command, enabled: bool) -> Item {
    Item::Command { label: label.into(), shortcut: shown(shortcut), command, enabled, checked: None }
}

fn check(label: &str, shortcut: &'static str, command: Command, enabled: bool, on: bool) -> Item {
    Item::Command { label: label.into(), shortcut: shown(shortcut), command, enabled, checked: Some(on) }
}

/// The prefix of the shortcuts as the sources write them.
const WRITTEN_PREFIX: &str = "Ctrl+";

/// The prefix of a shortcut whose key a plugin leaves to the host: the
/// platform's Ctrl/Cmd key, which Alt replaces there (`Ui::key`).
pub(super) const CONTROL_PREFIX: &str = if MACOS { "Cmd+" } else { "Ctrl+" };

/// The shortcut as the platform shows it: Cmd in place of Ctrl on macOS.
fn shown(shortcut: &'static str) -> Cow<'static, str> {
    if MACOS && let Some(rest) = shortcut.strip_prefix(WRITTEN_PREFIX) {
        return format!("Cmd+{rest}").into();
    }
    shortcut.into()
}

/// The keys of a context menu's lines (as the sources write them, empty for
/// none) as the window shows them: Cmd for Ctrl on macOS, Alt in a plugin.
#[must_use]
pub fn shown_shortcuts(keys: &[&'static str], hosted: bool) -> Vec<String> {
    keys.iter()
        .map(|&key| {
            let key = shown(key);
            if hosted { hosted_shortcuts(&key).into_owned() } else { key.into_owned() }
        })
        .collect()
}

/// What the panels say the Ctrl/Cmd key is: Cmd on macOS.
#[must_use]
pub fn command_label() -> &'static str {
    if MACOS { "Cmd" } else { "Ctrl" }
}

/// `text` (a list of shortcuts) as the platform shows it: Cmd in place of
/// Ctrl on macOS.
#[must_use]
pub fn platform_shortcuts(text: &str) -> Cow<'_, str> {
    if MACOS { text.replace(WRITTEN_PREFIX, "Cmd+").into() } else { text.into() }
}

/// The menus, as they stand.
pub fn menus(s: &MenuState) -> Vec<Menu> {
    let mut menus = vec![
        file_menu(s),
        edit_menu(s),
        view_menu(s),
        pattern_menu(s),
        generate_menu(s),
        play_menu(s),
        song_menu(s),
        kit_menu(s),
        help_menu(s),
    ];
    if s.hosted {
        keep_hosted(&mut menus);
    }
    menus
}

/// A plugin does not show what it cannot run: drop those entries instead of
/// greying them out. Clean up the separators they leave behind, and the menus
/// that end up empty.
fn keep_hosted(menus: &mut Vec<Menu>) {
    for menu in menus.iter_mut() {
        menu.items.retain(|item| match item {
            Item::Command { command, .. } => command.hosted_refusal().is_none(),
            Item::Separator => true,
        });
        let mut cleaned: Vec<Item> = Vec::with_capacity(menu.items.len());
        for item in std::mem::take(&mut menu.items) {
            let after_item = matches!(cleaned.last(), Some(Item::Command { .. }));
            if after_item || matches!(item, Item::Command { .. }) {
                cleaned.push(item);
            }
        }
        while matches!(cleaned.last(), Some(Item::Separator)) {
            cleaned.pop();
        }
        menu.items = cleaned;
    }
    menus.retain(|menu| !menu.items.is_empty());
    for item in menus.iter_mut().flat_map(|menu| &mut menu.items) {
        host_item(item);
    }
}

fn file_menu(s: &MenuState) -> Menu {
    use Command as C;
    let p = s.has_pattern;
    Menu {
        title: "File",
        items: vec![
            command("Save", "Ctrl+S", C::Save, s.can_save),
            Item::Separator,
            command("Open project...", "Ctrl+O", C::LoadProject, s.saved_projects),
            command("Save project", "", C::SaveProject, s.can_save_project),
            command("Save project as...", "", C::SaveProjectAs, true),
            command("Export project...", "", C::ExportProject, true),
            command("Import project...", "", C::ImportProject, true),
            command("Rename project...", "", C::RenameProject, true),
            command("Project description...", "", C::ProjectDescription, true),
            Item::Separator,
            command("Export library...", "", C::ExportLibrary, true),
            command("Import library...", "", C::ImportLibrary, true),
            command("Load the demo library...", "", C::DemoLibrary, s.has_demo),
            Item::Separator,
            command("Import MIDI file...", "", C::ImportMidi, true),
            command("Export pattern as JSON...", "Ctrl+Shift+S", C::ExportPatternJson, p),
            command("Export pattern as MIDI...", if MACOS { "Cmd+Shift+M" } else { "Ctrl+M" }, C::ExportMidi, p),
            command("Export pattern as WAV...", "Ctrl+E", C::ExportWav, p && s.can_render),
            command("Export song as WAV...", "Ctrl+Shift+E", C::ExportSongWav, s.has_songs && s.can_render),
            command("Export pattern stems...", "", C::ExportStems, p && s.can_render),
            command("Export song stems...", "", C::ExportSongStems, s.has_songs && s.can_render),
            Item::Separator,
            command("Back up user data...", "", C::BackUpUserData, s.user_folder),
            command("Restore user data...", "", C::RestoreUserData, s.user_folder),
            command("Change user data folder...", "", C::ChangeUserFolder, s.user_folder),
            command("Use the default user data folder...", "", C::DefaultUserFolder, s.user_folder),
            command("Open user data folder", "", C::OpenUserFolder, s.user_folder),
            Item::Separator,
            command("Quit", "Ctrl+Q, Ctrl+W", C::Quit, true),
        ],
    }
}

fn edit_menu(s: &MenuState) -> Menu {
    use Command as C;
    let p = s.has_pattern;
    let track = s.focus_track.is_some();
    let zone = s.has_zone;
    let undo = s.undo.as_ref().map_or_else(|| "Undo".to_owned(), |d| format!("Undo: {d}"));
    let redo = s.redo.as_ref().map_or_else(|| "Redo".to_owned(), |d| format!("Redo: {d}"));
    Menu {
        title: "Edit",
        items: vec![
            command(undo, "Ctrl+Z", C::Undo, s.undo.is_some()),
            command(redo, if MACOS { "Cmd+Shift+Z" } else { "Ctrl+Y" }, C::Redo, s.redo.is_some()),
            Item::Separator,
            command(if zone { "Copy zone" } else { "Copy step" }, "Ctrl+C", C::CopyStep, s.has_cursor || zone),
            command(if zone { "Cut zone" } else { "Cut step" }, "Ctrl+X", C::Cut, s.has_cursor || zone),
            command("Copy track", "Ctrl+Shift+C", C::CopyTrack, track),
            command("Paste", "Ctrl+V", C::Paste, p && (s.clipboard_step || s.clipboard_track || s.clipboard_block)),
            command("Paste track", "Ctrl+Shift+V", C::PasteTrack, p && s.clipboard_track),
            Item::Separator,
            command(if zone { "Clear zone" } else { "Clear step" }, "Delete", C::ClearStep, s.has_cursor || zone),
            command("Accent on / off", "L", C::ToggleAccent, s.has_cursor),
            command("Quantize...", "", C::Quantize, p),
            command("Humanize...", "", C::Humanize, p),
            command("Select all", "Ctrl+A", C::SelectAll, p),
            command("Deselect", "Esc", C::Deselect, s.has_cursor || s.has_selection || zone),
            Item::Separator,
            command("Preferences...", "Ctrl+,", C::Preferences, true),
            command("MIDI controls...", "", C::MidiControls, p),
        ],
    }
}

fn view_menu(s: &MenuState) -> Menu {
    use Command as C;
    Menu {
        title: "View",
        items: vec![
            check("Track editor", "F2, Ctrl+2", C::TrackEditor, true, s.track_editor),
            check("Note editor", "F7, Ctrl+7", C::NoteEditor, true, s.note_editor),
            check("Simple view", "", C::SimpleView, true, !s.track_editor && !s.note_editor),
            Item::Separator,
            check("Grid", "F5, Ctrl+5", C::Grid, true, s.grid),
            check("Piano roll", "F4, Ctrl+4", C::PianoRoll, true, s.piano_roll),
            check("Song", "F3, Ctrl+3", C::SongView, true, s.song_view),
            check("Synth", "F6, Ctrl+6", C::SynthEditor, true, s.synth),
            check("Mixer", "F12", C::Mixer, true, s.mixer),
            check("Kit editor", "Ctrl+K", C::KitEditor, true, s.kit_editor),
            Item::Separator,
            check("VU meters", "Shift+V", C::ToggleVus, true, s.show_vus),
            check("Toasts", "Ctrl+Shift+T", C::ToggleToasts, true, s.show_toasts),
            command("Next colour scheme", "Shift+C", C::NextColorScheme, true),
            Item::Separator,
            command("Zoom in", "Ctrl+=", C::ZoomIn, true),
            command("Zoom out", "Ctrl+-", C::ZoomOut, true),
            command("Actual size", "Ctrl+0", C::ZoomReset, true),
            command("Zoom: fit to window", "", C::ZoomToFit, true),
            check("Full screen", "F11", C::Fullscreen, true, s.fullscreen),
        ],
    }
}

fn pattern_menu(s: &MenuState) -> Menu {
    use Command as C;
    let p = s.has_pattern;
    let track = s.focus_track.is_some();
    Menu {
        title: "Pattern",
        items: vec![
            command("New pattern", "Ctrl+N", C::NewPattern, true),
            command("Duplicate pattern", "Ctrl+D", C::DuplicatePattern, p),
            command("Rename pattern...", "", C::RenamePattern, p),
            command("Pattern description...", "", C::PatternDescription, p),
            command("Pattern tags...", "", C::PatternTags, p),
            Item::Separator,
            command("Previous pattern", "[", C::PreviousPattern, p),
            command("Next pattern", "]", C::NextPattern, p),
            command("Random pattern", "F", C::SelectRandomPattern, p),
            Item::Separator,
            command("Tempo -1", "-", C::TempoDown, p),
            command("Tempo +1", "=", C::TempoUp, p),
            command("Length -1 beat", "", C::LengthDown, p),
            command("Length +1 beat", "", C::LengthUp, p),
            Item::Separator,
            command("Add track...", "", C::ChooseTrack, p),
            command("Add KICK track", "Ctrl+T", C::AddTrack, p),
            command("Remove track", "Ctrl+Backspace", C::RemoveTrack, s.can_remove_track),
            command("Next grid", "Ctrl+G", C::NextGrid, track),
            command("Loop one step shorter", "", C::ShortenLoop, track),
            Item::Separator,
            command("Compact tracks", "", C::CompactTracks, p),
            command("Clear pattern", "", C::ClearPattern, p),
        ],
    }
}

fn generate_menu(s: &MenuState) -> Menu {
    use Command as C;
    let p = s.has_pattern;
    let track = s.focus_track.is_some();
    let (auto, _, _) = s.focus_track.unwrap_or_default();
    Menu {
        title: "Generate",
        items: vec![
            command("Generate the pattern again", "Shift+B", C::GeneratePattern, true),
            command("New pattern in the style", "Ctrl+Shift+B", C::NewGeneratedPattern, true),
            check("Auto (every track)", "", C::AutoAll, p, s.auto_all),
            Item::Separator,
            check("Drums (auto)", "Shift+D", C::GenerateDrums, p, s.generation[0]),
            check("Bass (auto)", "Shift+S", C::GenerateBass, p, s.generation[1]),
            check("Chords (auto)", "", C::GenerateChords, p, s.generation[2]),
            Item::Separator,
            check("Track in auto mode", "", C::TrackAuto, track, auto),
            check("Lock the track", "", C::LockTrack, track, s.locked),
            command("Random notes on the track", "Shift+X", C::RandomTrack, track),
        ],
    }
}

fn play_menu(s: &MenuState) -> Menu {
    use Command as C;
    let p = s.has_pattern;
    let track = s.focus_track.is_some();
    let (_, mute, solo) = s.focus_track.unwrap_or_default();
    Menu {
        title: "Play",
        items: vec![
            check("Play pattern", "Space", C::PlayPattern, p, s.playing),
            command("Play song", "Shift+Space", C::PlaySong, s.has_songs),
            command("Rewind", "Home", C::Rewind, true),
            check("Loop the song", "Ctrl+L", C::ToggleLoop, s.has_songs, s.is_loop_on),
            command("Clear the loop", "", C::ClearLoop, s.has_songs),
            check("Record automation", "", C::RecordAutomation, s.has_songs, s.is_recording_automation),
            command("Re-enable automation", "", C::ReenableAutomation, s.has_overridden),
            Item::Separator,
            check("Record notes", "Ctrl+R", C::Record, p, s.recording),
            check("Record off the grid", "", C::RecordTiming, p, s.record_timing),
            check("Metronome", "F9", C::Metronome, true, s.metronome),
            command("Tap tempo", "F8", C::TapTempo, true),
            Item::Separator,
            check("Mute track", "M, 1-9", C::Mute, track, mute),
            check("Solo track", "S, Shift+1-9", C::Solo, track, solo),
            command("Hear track", "Q-P", C::PreviewTrack, track),
        ],
    }
}

fn song_menu(s: &MenuState) -> Menu {
    use Command as C;
    Menu {
        title: "Song",
        items: vec![
            check("Song view", "F3", C::SongView, true, s.song_view),
            Item::Separator,
            command("New arrangement...", "", C::NewSong, true),
            command("Remove arrangement", "", C::RemoveSong, s.has_songs),
            command("Previous arrangement", "Shift+[", C::PreviousSong, s.has_songs),
            command("Next arrangement", "Shift+]", C::NextSong, s.has_songs),
        ],
    }
}

fn help_menu(s: &MenuState) -> Menu {
    use Command as C;
    Menu {
        title: "Help",
        items: vec![
            command("Keyboard shortcuts", "F1", C::Shortcuts, true),
            command("About orDrumbox", "", C::About, true),
            command("User manual...", "", C::Manual, true),
            command("Check for updates", "", C::CheckUpdates, !s.checking_updates),
            match &s.newer_release {
                Some(version) => command(format!("Download orDrumbox {version}..."), "", C::DownloadPage, true),
                None => command("Download page...", "", C::DownloadPage, false),
            },
            Item::Separator,
            command("Factory reset...", "", C::FactoryReset, true),
        ],
    }
}

fn kit_menu(s: &MenuState) -> Menu {
    use Command as C;
    let p = s.has_pattern;
    Menu {
        title: "Kit",
        items: vec![
            command("Previous kit", ",", C::PreviousKit, s.kits.len() > 1),
            command("Next kit", ".", C::NextKit, s.kits.len() > 1),
            command("Random kit", "Shift+G", C::SelectRandomDrumkit, s.kits.len() > 1),
            Item::Separator,
            command("Auto-assign all tracks", "Shift+J", C::AutoAssignAll, p),
            command("Use synth kit", "Shift+H", C::ConvertToSynth, p),
            command("Random samples on all tracks", "Shift+K", C::RandomSamples, p && !s.kits.is_empty()),
            Item::Separator,
            check("Kit editor", "Ctrl+K", C::KitEditor, true, s.kit_editor),
            command("Normalize kit", "", C::NormalizeKit, !s.kits.is_empty()),
            Item::Separator,
            command("Add sample...", "", C::AddSample, !s.kits.is_empty()),
            command("Replace sample...", "", C::ReplaceSample, s.kit_sample),
            command("Load kit mapping...", "", C::LoadMapping, true),
            command("Export kit mapping...", "", C::ExportMapping, !s.kits.is_empty()),
            command("Export kit with its samples...", "", C::ExportKitFolder, !s.kits.is_empty()),
            Item::Separator,
            command("Import folder as kit...", "", C::ImportFolder, true),
        ],
    }
}

/// `text` (a shortcut, a tooltip) as a plugin shows it: Alt in place of Ctrl.
/// A menu's shortcut arrives as the platform shows it (Cmd+ on macOS), a
/// panel's tooltip as the sources write it (Ctrl+): both become Alt+.
#[must_use]
pub fn hosted_shortcuts(text: &str) -> Cow<'_, str> {
    let prefix = if text.contains(CONTROL_PREFIX) { CONTROL_PREFIX } else { WRITTEN_PREFIX };
    if text.contains(prefix) { text.replace(prefix, "Alt+").into() } else { text.into() }
}

/// What a plugin shows of the shortcut of `item`: Alt in place of Ctrl.
fn host_item(item: &mut Item) {
    let Item::Command { shortcut, .. } = item else { return };
    if let Cow::Owned(alt) = hosted_shortcuts(shortcut) {
        *shortcut = alt.into();
    }
}
