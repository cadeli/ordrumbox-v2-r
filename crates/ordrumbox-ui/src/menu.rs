//! The menu bar: every command of the app, always at the top of the window,
//! with its shortcut, greyed out when it cannot run, ticked when it is on.
//!
//! Drawn by the app itself (no native menus: the app has no GUI toolkit).
//! Mouse: a click opens a menu, moving over the titles switches menus, a
//! click runs a command. Keyboard: F10 or Alt opens the first menu, arrows
//! move, Enter runs, Escape closes.

use std::borrow::Cow;

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
    ExportStems,
    ExportSongStems,
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
    /// Recorded notes keep the timing played, or go on the step.
    RecordTiming,
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
    /// Edit > MIDI controls: bind the knobs of a MIDI controller.
    MidiControls,
    /// Edit > Quantize: the notes onto a grid.
    Quantize,
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
            | Self::ExportSongWav
            | Self::ExportSongStems => Some(SONGS_IN_THE_APP),
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
    pub record_timing: bool,
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

#[path = "menu/build.rs"]
mod build;
#[path = "menu/draw.rs"]
mod draw;
pub use build::{command_label, hosted_shortcuts, menus, platform_shortcuts};
pub use draw::*;

#[cfg(test)]
#[path = "menu/tests.rs"]
mod tests;
