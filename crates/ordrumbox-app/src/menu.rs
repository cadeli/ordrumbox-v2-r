//! The menu bar: every command of the app, always at the top of the window,
//! with its shortcut, greyed out when it cannot run, ticked when it is on.
//!
//! Drawn by the app itself (no native menus: the app has no GUI toolkit).
//! Mouse: a click opens a menu, moving over the titles switches menus, a
//! click runs a command. Keyboard: F10 or Alt opens the first menu, arrows
//! move, Enter runs, Escape closes.

use crate::canvas::{Canvas, Rgb, text_width};
use crate::theme;

/// What a menu item (or its shortcut) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TrackEditor,
    Grid,
    SongView,
    PianoRoll,
    SynthEditor,
    NewSong,
    RemoveSong,
    RenameSong,
    SongDescription,
    SaveSong,
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
    Paste,
    PasteTrack,
    ClearStep,
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
    AddTrack,
    RemoveTrack,
    NextGrid,
    ShortenLoop,
    CompactTracks,
    GeneratePattern,
    GenerateDrums,
    GenerateBass,
    GenerateChords,
    TrackAuto,
    RandomTrack,
    PlayPattern,
    PlaySong,
    PreviousSong,
    NextSong,
    Mute,
    Solo,
    Kit(usize),
    PreviousKit,
    NextKit,
    KitEditor,
    NormalizeKit,
    ExportMapping,
    LoadMapping,
    AddSample,
    ReplaceSample,
    ImportFolder,
    Reassign,
    SelectRandomPattern,
    SelectRandomDrumkit,
    ToggleVus,
    ExportPatternJson,
    ConvertToSynth,
    AutoAssignAll,
    RandomSamples,
    PreviewTrack,
    Shortcuts,
    About,
    FactoryReset,
}

/// One line of a menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Command {
        label: String,
        shortcut: &'static str,
        command: Command,
        enabled: bool,
        /// `Some` for an on/off command.
        checked: Option<bool>,
    },
    Separator,
}

impl Item {
    fn runnable(&self) -> Option<Command> {
        match self {
            Self::Command { command, enabled: true, .. } => Some(*command),
            _ => None,
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
    pub has_selection: bool,
    pub playing: bool,
    pub has_songs: bool,
    /// Generation on: drums, bass, chords.
    pub generation: [bool; 3],
    pub kits: Vec<String>,
    pub kit_index: usize,
    pub kit_editor: bool,
    pub track_editor: bool,
    pub grid: bool,
    /// VU meters on the track stripes (V).
    pub show_vus: bool,
    pub song_view: bool,
    pub piano_roll: bool,
    pub synth: bool,
    /// A folder to save songs in (Save song).
    pub can_save_song: bool,
    /// Song files saved (Load song).
    pub saved_songs: bool,
    /// The kit editor has a sample selected.
    pub kit_sample: bool,
}

fn command(label: impl Into<String>, shortcut: &'static str, command: Command, enabled: bool) -> Item {
    Item::Command { label: label.into(), shortcut, command, enabled, checked: None }
}

fn check(label: &str, shortcut: &'static str, command: Command, enabled: bool, on: bool) -> Item {
    Item::Command { label: label.into(), shortcut, command, enabled, checked: Some(on) }
}

/// The menus, as they stand.
pub fn menus(s: &MenuState) -> Vec<Menu> {
    use Command as C;
    let p = s.has_pattern;
    let track = s.focus_track.is_some();
    let (auto, mute, solo) = s.focus_track.unwrap_or_default();
    let undo = s.undo.as_ref().map_or_else(|| "Undo".to_owned(), |d| format!("Undo: {d}"));
    let redo = s.redo.as_ref().map_or_else(|| "Redo".to_owned(), |d| format!("Redo: {d}"));
    let kits: Vec<Item> = vec![
        command("Previous kit", ",", C::PreviousKit, s.kits.len() > 1),
        command("Next kit", ".", C::NextKit, s.kits.len() > 1),
        command("Random kit", "G", C::SelectRandomDrumkit, s.kits.len() > 1),
        Item::Separator,
        command("Synth sounds on all tracks", "H", C::ConvertToSynth, p),
        command("Auto-assign all tracks", "J", C::AutoAssignAll, p),
        command("Random samples on all tracks", "K", C::RandomSamples, p && !s.kits.is_empty()),
        Item::Separator,
        check("Kit editor", "Ctrl+K", C::KitEditor, true, s.kit_editor),
        command("Normalize kit", "", C::NormalizeKit, !s.kits.is_empty()),
        command("Assign sounds again", "", C::Reassign, !s.kits.is_empty()),
    ];
    vec![
        Menu {
            title: "File",
            items: vec![
                command("Save", "", C::Save, s.can_save),
                Item::Separator,
                command("Save song", "", C::SaveSong, s.can_save_song),
                command("Load song...", "", C::LoadSong, s.saved_songs),
                command("Export song", "", C::ExportSong, true),
                command("Import song...", "", C::ImportSong, true),
                Item::Separator,
                command("Import MIDI file...", "Ctrl+O", C::ImportMidi, true),
                command("Export pattern as JSON", "Ctrl+S", C::ExportPatternJson, p),
                command("Export pattern as MIDI", "Ctrl+M", C::ExportMidi, p),
                command("Export pattern as WAV", "Ctrl+E", C::ExportWav, p && s.can_render),
                command("Export song as WAV", "Ctrl+Shift+E", C::ExportSongWav, s.has_songs && s.can_render),
                Item::Separator,
                command("Add sample...", "", C::AddSample, !s.kits.is_empty()),
                command("Replace sample...", "", C::ReplaceSample, s.kit_sample),
                command("Import folder as kit...", "", C::ImportFolder, true),
                command("Load kit mapping...", "", C::LoadMapping, true),
                command("Export kit mapping", "", C::ExportMapping, !s.kits.is_empty()),
                Item::Separator,
                command("Quit", "Ctrl+Q", C::Quit, true),
            ],
        },
        Menu {
            title: "Edit",
            items: vec![
                command(undo, "Ctrl+Z", C::Undo, s.undo.is_some()),
                command(redo, "Ctrl+Y", C::Redo, s.redo.is_some()),
                Item::Separator,
                command("Copy step", "Ctrl+C", C::CopyStep, s.has_cursor),
                command("Copy track", "Ctrl+Shift+C", C::CopyTrack, track),
                command("Paste", "Ctrl+V", C::Paste, p && (s.clipboard_step || s.clipboard_track)),
                command("Paste track", "Ctrl+Shift+V", C::PasteTrack, p && s.clipboard_track),
                Item::Separator,
                command("Clear step", "Delete", C::ClearStep, s.has_cursor),
                command("Deselect", "Esc", C::Deselect, s.has_cursor || s.has_selection),
            ],
        },
        Menu {
            title: "View",
            items: vec![
                check("Track editor", "F2", C::TrackEditor, true, s.track_editor),
                Item::Separator,
                check("Grid", "F5", C::Grid, true, s.grid),
                check("Piano roll", "F4", C::PianoRoll, true, s.piano_roll),
                check("Song", "F3", C::SongView, true, s.song_view),
                check("Synth", "F6", C::SynthEditor, true, s.synth),
                check("Kit editor", "Ctrl+K", C::KitEditor, true, s.kit_editor),
                Item::Separator,
                check("VU meters", "V", C::ToggleVus, true, s.show_vus),
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
                command("Length -1 beat", "Ctrl+-", C::LengthDown, p),
                command("Length +1 beat", "Ctrl+=", C::LengthUp, p),
                Item::Separator,
                command("Add track", "Ctrl+T", C::AddTrack, p),
                command("Remove track", "Ctrl+Backspace", C::RemoveTrack, s.can_remove_track),
                command("Next grid", "Ctrl+G", C::NextGrid, track),
                command("Loop one step shorter", "Ctrl+L", C::ShortenLoop, track),
                Item::Separator,
                command("Compact tracks", "", C::CompactTracks, p),
            ],
        },
        Menu {
            title: "Generate",
            items: vec![
                command("Generate a pattern", "B", C::GeneratePattern, true),
                Item::Separator,
                check("Drums (auto)", "D", C::GenerateDrums, p, s.generation[0]),
                check("Bass (auto)", "S", C::GenerateBass, p, s.generation[1]),
                check("Chords (auto)", "A", C::GenerateChords, p, s.generation[2]),
                Item::Separator,
                check("Track in auto mode", "Z", C::TrackAuto, track, auto),
                command("Random notes on the track", "X", C::RandomTrack, track),
            ],
        },
        Menu {
            title: "Play",
            items: vec![
                check("Play pattern", "Space", C::PlayPattern, p, s.playing),
                command("Play song", "Shift+Space", C::PlaySong, s.has_songs),
                Item::Separator,
                check("Mute track", "1-9", C::Mute, track, mute),
                check("Solo track", "Shift+1-9", C::Solo, track, solo),
                command("Hear track", "Q-P", C::PreviewTrack, track),
            ],
        },
        Menu {
            title: "Song",
            items: vec![
                check("Song view", "F3", C::SongView, true, s.song_view),
                Item::Separator,
                command("Play song", "Shift+Space", C::PlaySong, s.has_songs),
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
                Item::Separator,
                command("Factory reset...", "", C::FactoryReset, true),
            ],
        },
    ]
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

    /// The highlighted command, if it can run.
    pub fn command(self, menus: &[Menu]) -> Option<Command> {
        menus.get(self.menu)?.items.get(self.item?)?.runnable()
    }
}

fn first_runnable(menu: &Menu) -> Option<usize> {
    menu.items.iter().position(|i| i.runnable().is_some())
}

// ─── Geometry ────────────────────────────────────────────────────────────────

const BAR_BACKGROUND: Rgb = theme::PANEL;
const PANEL: Rgb = theme::PANEL_2;
const BORDER: Rgb = theme::LINE;
const HIGHLIGHT: Rgb = theme::HIGHLIGHT;
const TEXT: Rgb = theme::TEXT;
const DIM: Rgb = theme::CYAN;
const DISABLED: Rgb = theme::DISABLED;
const ACCENT: Rgb = theme::PINK;

/// Height of the menu bar.
pub fn bar_height(scale: i32) -> i32 {
    14 * scale
}

fn item_height(scale: i32) -> i32 {
    13 * scale
}

fn separator_height(scale: i32) -> i32 {
    7 * scale
}

/// Left edge and width of each title in the bar.
fn titles(menus: &[Menu], scale: i32) -> Vec<(i32, i32)> {
    let mut x = 8 * scale;
    menus
        .iter()
        .map(|m| {
            let w = text_width(m.title, scale) + 16 * scale;
            let at = (x, w);
            x += w;
            at
        })
        .collect()
}

/// The open menu's panel: left, top, width, and each item's top.
fn panel(menus: &[Menu], open: Open, scale: i32) -> (i32, i32, i32, Vec<i32>) {
    let (x, _) = titles(menus, scale)[open.menu];
    let menu = &menus[open.menu];
    let mut label_w = 0;
    let mut shortcut_w = 0;
    for item in &menu.items {
        if let Item::Command { label, shortcut, .. } = item {
            label_w = label_w.max(text_width(label, scale));
            shortcut_w = shortcut_w.max(text_width(shortcut, scale));
        }
    }
    let width = 18 * scale + label_w + 24 * scale + shortcut_w + 8 * scale;
    let mut y = bar_height(scale) + 2 * scale;
    let tops = menu
        .items
        .iter()
        .map(|item| {
            let top = y;
            y += if matches!(item, Item::Separator) { separator_height(scale) } else { item_height(scale) };
            top
        })
        .collect();
    (x, bar_height(scale), width, tops)
}

/// What the mouse is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Title(usize),
    /// An item of the open menu.
    Item(usize),
    /// Inside the open panel, but on no item (a separator, the border).
    Panel,
    Bar,
}

pub fn hit(menus: &[Menu], open: Option<Open>, scale: i32, x: i32, y: i32) -> Option<Hit> {
    if (0..bar_height(scale)).contains(&y) {
        let title = titles(menus, scale).iter().position(|&(tx, w)| (tx..tx + w).contains(&x));
        return Some(title.map_or(Hit::Bar, Hit::Title));
    }
    let open = open?;
    let (px, py, width, tops) = panel(menus, open, scale);
    let items = &menus[open.menu].items;
    let bottom = tops.last().map_or(py, |&t| t + item_height(scale)) + 2 * scale;
    if !(px..px + width).contains(&x) || !(py..bottom).contains(&y) {
        return None;
    }
    let found = tops.iter().zip(items).position(|(&top, item)| {
        let h = if matches!(item, Item::Separator) { separator_height(scale) } else { item_height(scale) };
        (top..top + h).contains(&y)
    });
    Some(found.filter(|&i| !matches!(items[i], Item::Separator)).map_or(Hit::Panel, Hit::Item))
}

// ─── Drawing ─────────────────────────────────────────────────────────────────

/// The bar, and the open menu over everything.
pub fn draw(canvas: &mut Canvas, menus: &[Menu], open: Option<Open>, scale: i32) {
    let bar = bar_height(scale);
    canvas.fill_rect(0, 0, canvas.width(), bar, BAR_BACKGROUND);
    canvas.fill_rect(0, bar - scale.max(1), canvas.width(), scale.max(1), BORDER);
    let text_y = (bar - 8 * scale) / 2;
    for (i, (menu, (x, w))) in menus.iter().zip(titles(menus, scale)).enumerate() {
        let is_open = open.is_some_and(|o| o.menu == i);
        if is_open {
            canvas.fill_rect(x, 0, w, bar - scale.max(1), HIGHLIGHT);
        }
        canvas.text(x + 8 * scale, text_y, menu.title, scale, if is_open { ACCENT } else { TEXT });
    }
    let Some(open) = open else { return };
    let (px, py, width, tops) = panel(menus, open, scale);
    let items = &menus[open.menu].items;
    let bottom = tops.last().map_or(py, |&t| t + item_height(scale)) + 2 * scale;
    canvas.fill_rect(px, py, width, bottom - py, BORDER);
    canvas.fill_rect(px + scale, py, width - 2 * scale, bottom - py - scale, PANEL);
    for (i, (item, &top)) in items.iter().zip(&tops).enumerate() {
        match item {
            Item::Separator => {
                canvas.fill_rect(
                    px + 6 * scale,
                    top + separator_height(scale) / 2,
                    width - 12 * scale,
                    scale.max(1),
                    BORDER,
                );
            }
            Item::Command { label, shortcut, enabled, checked, .. } => {
                let h = item_height(scale);
                if open.item == Some(i) && *enabled {
                    canvas.fill_rect(px + scale, top, width - 2 * scale, h, HIGHLIGHT);
                }
                let ty = top + (h - 8 * scale) / 2;
                if let Some(on) = checked {
                    let (bx, by, b) = (px + 5 * scale, ty + scale, 6 * scale);
                    if *on {
                        canvas.fill_rect(bx, by, b, b, if *enabled { ACCENT } else { DISABLED });
                    } else {
                        let line = scale.max(1);
                        let c = if *enabled { DIM } else { DISABLED };
                        canvas.fill_rect(bx, by, b, line, c);
                        canvas.fill_rect(bx, by + b - line, b, line, c);
                        canvas.fill_rect(bx, by, line, b, c);
                        canvas.fill_rect(bx + b - line, by, line, b, c);
                    }
                }
                let color = if !enabled {
                    DISABLED
                } else if open.item == Some(i) {
                    ACCENT
                } else {
                    TEXT
                };
                canvas.text(px + 18 * scale, ty, label, scale, color);
                let sw = text_width(shortcut, scale);
                canvas.text(px + width - 8 * scale - sw, ty, shortcut, scale, if *enabled { DIM } else { DISABLED });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> MenuState {
        MenuState {
            has_pattern: true,
            kits: vec!["real".into(), "808".into()],
            undo: Some("Add note".into()),
            ..MenuState::default()
        }
    }

    #[test]
    fn keyboard_skips_what_cannot_run() {
        let menus = menus(&state());
        let edit = menus.iter().position(|m| m.title == "Edit").unwrap();
        let open = Open::new(&menus, edit, true);
        assert_eq!(open.command(&menus), Some(Command::Undo));
        // Redo, copy and paste cannot run: the next is nothing else but Undo again.
        assert_eq!(open.step(&menus, 1).command(&menus), Some(Command::Undo));
        let file = Open::new(&menus, 0, true);
        // Save, Save song and Load song are greyed out: the first item highlighted is Export song.
        assert_eq!(file.command(&menus), Some(Command::ExportSong));
        assert_eq!(file.step(&menus, 1).command(&menus), Some(Command::ImportSong));
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
        for command in [C::SongView, C::PlaySong, C::NewSong, C::RemoveSong, C::PreviousSong, C::NextSong] {
            assert!(song.contains(&command), "{command:?}");
        }
        assert!(song.contains(&C::RenameSong) && song.contains(&C::SongDescription));
        for command in [C::SaveSong, C::ExportSong, C::ImportSong] {
            assert!(file.contains(&command), "{command:?}");
        }
        // Load needs saved songs.
        assert!(!file.contains(&C::LoadSong));
    }

    #[test]
    fn every_file_command_is_in_the_file_menu() {
        let menus = menus(&MenuState { can_save_song: true, saved_songs: true, kit_sample: true, ..state() });
        use Command as C;
        let file_commands = [
            C::SaveSong,
            C::LoadSong,
            C::ExportSong,
            C::ImportSong,
            C::ImportMidi,
            C::ExportMidi,
            C::AddSample,
            C::ReplaceSample,
            C::ImportFolder,
            C::LoadMapping,
            C::ExportMapping,
        ];
        for menu in &menus {
            for item in &menu.items {
                if let Item::Command { command, .. } = item {
                    if file_commands.contains(command) {
                        assert_eq!(menu.title, "File", "{command:?}");
                    }
                }
            }
        }
        let file: Vec<Command> = menus[0].items.iter().filter_map(Item::runnable).collect();
        for command in file_commands {
            assert!(file.contains(&command), "{command:?}");
        }
    }

    #[test]
    fn the_help_menu_has_the_factory_reset_command() {
        let menus = menus(&state());
        let help = menus.iter().find(|m| m.title == "Help").unwrap();
        let commands: Vec<Command> = help.items.iter().filter_map(Item::runnable).collect();
        assert!(commands.contains(&Command::FactoryReset));
    }
}
