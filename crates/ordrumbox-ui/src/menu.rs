//! The menu bar: every command of the app, always at the top of the window,
//! with its shortcut, greyed out when it cannot run, ticked when it is on.
//!
//! Drawn by the app itself (no native menus: the app has no GUI toolkit).
//! Mouse: a click opens a menu, moving over the titles switches menus, a
//! click runs a command. Keyboard: F10 or Alt opens the first menu, arrows
//! move, Enter runs, Escape closes.

use crate::scale::Scale;
use crate::ui_input::MACOS;
use std::borrow::Cow;

use crate::canvas::{Canvas, Rgb, char_width, fit, text_width};
use crate::theme;

/// What a menu item (or its shortcut) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TrackEditor,
    NoteEditor,
    MasterStrip,
    Grid,
    SongView,
    PianoRoll,
    SynthEditor,
    NewSong,
    RemoveSong,
    RenameSong,
    SongDescription,
    SaveSong,
    SaveSongAs,
    LoadSong,
    ExportSong,
    ImportSong,
    Save,
    ImportMidi,
    ExportWav,
    ExportSongWav,
    ExportMidi,
    Quit,
    Undo,
    Redo,
    CopyStep,
    CopyTrack,
    /// The zone's notes, else the cursor step's, copied and cleared.
    Cut,
    Paste,
    PasteTrack,
    ClearStep,
    ToggleAccent,
    /// Ctrl+A: every note of the pattern as a zone.
    SelectAll,
    Deselect,
    NewPattern,
    DuplicatePattern,
    RenamePattern,
    PatternDescription,
    PreviousPattern,
    NextPattern,
    TempoDown,
    TempoUp,
    LengthDown,
    LengthUp,
    /// A new track named KICK (Ctrl+T).
    AddTrack,
    /// The "add track" menus: an instrument category, then the instrument.
    ChooseTrack,
    RemoveTrack,
    NextGrid,
    ShortenLoop,
    CompactTracks,
    ClearPattern,
    GeneratePattern,
    GenerateDrums,
    GenerateBass,
    GenerateChords,
    TrackAuto,
    RandomTrack,
    PlayPattern,
    PlaySong,
    /// Home: back to the first beat or measure.
    Rewind,
    /// F8: the tempo from the time between taps.
    TapTempo,
    /// F9: a click on every beat.
    Metronome,
    /// Ctrl+R: the row keys put notes on the step that plays.
    Record,
    /// The song's loop region removed.
    ClearLoop,
    PreviousSong,
    NextSong,
    Mute,
    Solo,
    Kit(usize),
    /// Ctrl+Shift+B: a new pattern in the chosen style.
    NewGeneratedPattern,
    /// AUTO: every track of the pattern in auto mode, or out of it.
    AutoAll,
    /// The focused track locked against the generation, or unlocked.
    LockTrack,
    PreviousKit,
    NextKit,
    KitEditor,
    NormalizeKit,
    ExportMapping,
    /// The kit with its samples, to a folder of its own.
    ExportKitFolder,
    LoadMapping,
    AddSample,
    ReplaceSample,
    ImportFolder,
    SelectRandomPattern,
    SelectRandomDrumkit,
    ToggleVus,
    ToggleToasts,
    NextColorScheme,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ZoomToFit,
    ExportPatternJson,
    ConvertToSynth,
    AutoAssignAll,
    RandomSamples,
    PreviewTrack,
    Shortcuts,
    About,
    Manual,
    FactoryReset,
    /// The library written to a file of its own (`library.json` format).
    ExportLibrary,
    /// A library file in the place of the library (one undo step).
    ImportLibrary,
    /// The assets' library in the place of the library (one undo step).
    DemoLibrary,
    /// The user's files copied to a folder of their own.
    BackUpUserData,
    /// A backup's files in the place of the user's (those moved aside).
    RestoreUserData,
    /// Another user data folder, from the next load on.
    ChangeUserFolder,
    /// Back to the default user data folder.
    DefaultUserFolder,
    /// The user data folder in the Finder or the Explorer.
    OpenUserFolder,
    CheckUpdates,
    DownloadPage,
    /// Edit > Preferences: the sound output, the MIDI input and what is shown.
    Preferences,
    /// F11: the window full screen, or back.
    Fullscreen,
}

/// Why a plugin leaves the user data folder alone.
pub const USER_DATA_IN_THE_APP: &str = "user data folder: in the app only, not in a plugin";

/// Why a plugin has no song commands.
pub const SONGS_IN_THE_APP: &str = "songs: in the app only, a plugin plays patterns";

impl Command {
    /// Why a plugin does not run this command (the host does it, or it is
    /// the app's), `None` when it does.
    pub fn hosted_refusal(self) -> Option<&'static str> {
        match self {
            Self::Save => Some("the host saves the library with its project"),
            Self::Quit => Some("the host closes the plugin's window"),
            Self::TempoDown | Self::TempoUp => Some("the tempo is the host's"),
            Self::PlayPattern => Some("the host's transport plays the pattern"),
            Self::Rewind => Some("the host's transport rewinds"),
            Self::TapTempo => Some("the tempo is the host's"),
            Self::CheckUpdates | Self::DownloadPage => Some("update check: in the app only, not in a plugin"),
            Self::FactoryReset => Some("factory reset: in the app only, not in a plugin"),
            Self::MasterStrip => Some("the master strip: in the app only, not in a plugin"),
            Self::Fullscreen => Some("the host owns the window"),
            Self::BackUpUserData
            | Self::RestoreUserData
            | Self::ChangeUserFolder
            | Self::DefaultUserFolder
            | Self::OpenUserFolder => Some(USER_DATA_IN_THE_APP),
            Self::SongView
            | Self::PlaySong
            | Self::ClearLoop
            | Self::PreviousSong
            | Self::NextSong
            | Self::NewSong
            | Self::RemoveSong
            | Self::RenameSong
            | Self::SongDescription
            | Self::SaveSong
            | Self::SaveSongAs
            | Self::LoadSong
            | Self::ExportSong
            | Self::ImportSong
            | Self::ExportSongWav => Some(SONGS_IN_THE_APP),
            _ => None,
        }
    }
}

/// One line of a menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Command {
        label: String,
        shortcut: Cow<'static, str>,
        command: Command,
        enabled: bool,
        /// `Some` for an on/off command.
        checked: Option<bool>,
    },
    Separator,
}

impl Item {
    pub fn runnable(&self) -> Option<Command> {
        match self {
            Self::Command { command, enabled: true, .. } => Some(*command),
            _ => None,
        }
    }

    /// What the item is called, `None` for a separator.
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Command { label, .. } => Some(label),
            Self::Separator => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub title: &'static str,
    pub items: Vec<Item>,
}

/// What the menus depend on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuState {
    pub can_save: bool,
    pub can_render: bool,
    pub has_pattern: bool,
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub has_cursor: bool,
    /// The track the cursor or selection is on: (auto, mute, solo).
    pub focus_track: Option<(bool, bool, bool)>,
    pub can_remove_track: bool,
    pub clipboard_step: bool,
    pub clipboard_track: bool,
    /// A zone's notes copied.
    pub clipboard_block: bool,
    /// A zone drawn over the grid (Shift+drag).
    pub has_zone: bool,
    pub has_selection: bool,
    pub playing: bool,
    pub metronome: bool,
    pub recording: bool,
    pub fullscreen: bool,
    pub has_songs: bool,
    /// Generation on: drums, bass, chords.
    pub generation: [bool; 3],
    /// AUTO lit: the pattern's tracks play in auto mode.
    pub auto_all: bool,
    /// The focused track is locked against the generation.
    pub locked: bool,
    pub kits: Vec<String>,
    pub kit_index: usize,
    pub kit_editor: bool,
    pub track_editor: bool,
    pub note_editor: bool,
    /// The master strip along the bottom (View > Master).
    pub master_strip: bool,
    pub grid: bool,
    /// VU meters on the track stripes (V).
    pub show_vus: bool,
    /// Messages pop up as toasts (View > Toasts).
    pub show_toasts: bool,
    pub song_view: bool,
    pub piano_roll: bool,
    pub synth: bool,
    /// A folder to save songs in (Save song).
    pub can_save_song: bool,
    /// Song files saved (Load song).
    pub saved_songs: bool,
    /// The kit editor has a sample selected.
    pub kit_sample: bool,
    /// An update check under way.
    pub checking_updates: bool,
    /// The version of a newer release found.
    pub newer_release: Option<String>,
    /// Shown by a plugin: what it leaves to the host or the app is left out.
    pub hosted: bool,
    /// The assets' library can be loaded (Load the demo library).
    pub has_demo: bool,
    /// A user data folder to back up, restore, change or open.
    pub user_folder: bool,
}

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
const CONTROL_PREFIX: &str = if MACOS { "Cmd+" } else { "Ctrl+" };

/// The shortcut as the platform shows it: Cmd in place of Ctrl on macOS.
fn shown(shortcut: &'static str) -> Cow<'static, str> {
    if MACOS && let Some(rest) = shortcut.strip_prefix(WRITTEN_PREFIX) {
        return format!("Cmd+{rest}").into();
    }
    shortcut.into()
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
    use Command as C;
    let p = s.has_pattern;
    let track = s.focus_track.is_some();
    let zone = s.has_zone;
    let (auto, mute, solo) = s.focus_track.unwrap_or_default();
    let undo = s.undo.as_ref().map_or_else(|| "Undo".to_owned(), |d| format!("Undo: {d}"));
    let redo = s.redo.as_ref().map_or_else(|| "Redo".to_owned(), |d| format!("Redo: {d}"));
    let kits: Vec<Item> = vec![
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
    ];
    let mut menus = vec![
        Menu {
            title: "File",
            items: vec![
                command("Save", "Ctrl+S", C::Save, s.can_save),
                Item::Separator,
                command("Save song", "", C::SaveSong, s.can_save_song),
                command("Save song as...", "", C::SaveSongAs, true),
                command("Load song...", "Ctrl+O", C::LoadSong, s.saved_songs),
                command("Export song...", "", C::ExportSong, true),
                command("Import song...", "", C::ImportSong, true),
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
                Item::Separator,
                command("Back up user data...", "", C::BackUpUserData, s.user_folder),
                command("Restore user data...", "", C::RestoreUserData, s.user_folder),
                command("Change user data folder...", "", C::ChangeUserFolder, s.user_folder),
                command("Use the default user data folder...", "", C::DefaultUserFolder, s.user_folder),
                command("Open user data folder", "", C::OpenUserFolder, s.user_folder),
                Item::Separator,
                command("Quit", "Ctrl+Q, Ctrl+W", C::Quit, true),
            ],
        },
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
                command("Select all", "Ctrl+A", C::SelectAll, p),
                command("Deselect", "Esc", C::Deselect, s.has_cursor || s.has_selection || zone),
                Item::Separator,
                command("Preferences...", "Ctrl+,", C::Preferences, true),
            ],
        },
        Menu {
            title: "View",
            items: vec![
                check("Track editor", "F2, Ctrl+2", C::TrackEditor, true, s.track_editor),
                check("Note editor", "F7, Ctrl+7", C::NoteEditor, true, s.note_editor),
                check("Master", "", C::MasterStrip, true, s.master_strip),
                Item::Separator,
                check("Grid", "F5, Ctrl+5", C::Grid, true, s.grid),
                check("Piano roll", "F4, Ctrl+4", C::PianoRoll, true, s.piano_roll),
                check("Song", "F3, Ctrl+3", C::SongView, true, s.song_view),
                check("Synth", "F6, Ctrl+6", C::SynthEditor, true, s.synth),
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
        },
        Menu {
            title: "Pattern",
            items: vec![
                command("New pattern", "Ctrl+N", C::NewPattern, true),
                command("Duplicate pattern", "Ctrl+D", C::DuplicatePattern, p),
                command("Rename pattern...", "", C::RenamePattern, p),
                command("Pattern description...", "", C::PatternDescription, p),
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
        },
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
        },
        Menu {
            title: "Play",
            items: vec![
                check("Play pattern", "Space", C::PlayPattern, p, s.playing),
                command("Play song", "Shift+Space", C::PlaySong, s.has_songs),
                command("Rewind", "Home", C::Rewind, true),
                command("Clear the loop", "", C::ClearLoop, s.has_songs),
                Item::Separator,
                check("Record notes", "Ctrl+R", C::Record, p, s.recording),
                check("Metronome", "F9", C::Metronome, true, s.metronome),
                command("Tap tempo", "F8", C::TapTempo, true),
                Item::Separator,
                check("Mute track", "M, 1-9", C::Mute, track, mute),
                check("Solo track", "S, Shift+1-9", C::Solo, track, solo),
                command("Hear track", "Q-P", C::PreviewTrack, track),
            ],
        },
        Menu {
            title: "Song",
            items: vec![
                check("Song view", "F3", C::SongView, true, s.song_view),
                Item::Separator,
                command("New arrangement...", "", C::NewSong, true),
                command("Remove arrangement", "", C::RemoveSong, s.has_songs),
                command("Previous arrangement", "Shift+[", C::PreviousSong, s.has_songs),
                command("Next arrangement", "Shift+]", C::NextSong, s.has_songs),
                Item::Separator,
                command("Rename song...", "", C::RenameSong, true),
                command("Song description...", "", C::SongDescription, true),
            ],
        },
        Menu { title: "Kit", items: kits },
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
        },
    ];
    if s.hosted {
        // A plugin does not show what it cannot run: drop those entries
        // instead of greying them out. Clean up the separators they leave
        // behind, and the menus that end up empty.
        for menu in &mut menus {
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
    menus
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

// ─── Open menu ───────────────────────────────────────────────────────────────

/// Which menu is open, and which of its items is highlighted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open {
    pub menu: usize,
    pub item: Option<usize>,
}

impl Open {
    /// Menu `menu`, its first runnable item highlighted when `keyboard`.
    pub fn new(menus: &[Menu], menu: usize, keyboard: bool) -> Self {
        let item = keyboard.then(|| first_runnable(&menus[menu])).flatten();
        Self { menu, item }
    }

    /// The menu `delta` to the right (wrapping).
    pub fn switch(self, menus: &[Menu], delta: isize) -> Self {
        let menu = (self.menu as isize + delta).rem_euclid(menus.len() as isize) as usize;
        Self::new(menus, menu, true)
    }

    /// The runnable item `delta` down (wrapping).
    pub fn step(self, menus: &[Menu], delta: isize) -> Self {
        let items = &menus[self.menu].items;
        let n = items.len() as isize;
        let mut i = self.item.map_or(if delta > 0 { -1 } else { n }, |i| i as isize);
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if items[i as usize].runnable().is_some() {
                return Self { item: Some(i as usize), ..self };
            }
        }
        self
    }

    /// The first item from the highlighted one on whose label starts with
    /// `letter` (wrapping; a repeat skips to the next one).
    pub fn jump(self, menus: &[Menu], letter: char) -> Self {
        let items = &menus[self.menu].items;
        let n = items.len();
        let start = self.item.map_or(0, |i| (i + 1) % n);
        for skip in 0..n {
            let i = (start + skip) % n;
            if items[i].label().is_some_and(|label| label.to_lowercase().starts_with(letter.to_ascii_lowercase())) {
                return Self { item: Some(i), ..self };
            }
        }
        self
    }

    /// The highlighted command, if it can run.
    pub fn command(self, menus: &[Menu]) -> Option<Command> {
        menus.get(self.menu)?.items.get(self.item?)?.runnable()
    }
}

fn first_runnable(menu: &Menu) -> Option<usize> {
    menu.items.iter().position(|i| i.runnable().is_some())
}

#[path = "menu/draw.rs"]
mod draw;
pub use draw::*;

#[cfg(test)]
#[path = "menu/tests.rs"]
mod tests;
