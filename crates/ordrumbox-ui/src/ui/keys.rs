//! The keys: shortcuts, shift hints and the view commands.

use crate::menu::Command;
use crate::ui_input::{Key, MACOS};

/// What a letter that changes every track says when pressed without Shift.
pub(super) fn shift_hint(code: Key) -> Option<&'static str> {
    Some(match code {
        Key::Letter('B') => "Shift+B: the pattern again in its style",
        Key::Letter('C') => "Shift+C: the next colour scheme",
        Key::Letter('D') => "Shift+D: drums in auto mode",
        Key::Letter('G') => "Shift+G: a random kit",
        Key::Letter('H') => "Shift+H: every track on a synth sound",
        Key::Letter('J') => "Shift+J: every track back on auto-assign",
        Key::Letter('K') => "Shift+K: a random sample on every track",
        Key::Letter('V') => "Shift+V: the VU meters on or off",
        Key::Letter('X') => "Shift+X: random notes on the track",
        _ => return None,
    })
}

/// The command a key runs, when the menus list it (`view::HELP`).
pub(super) fn shortcut(code: Key, shift: bool, control: bool) -> Option<Command> {
    use Command as C;
    Some(if control {
        match code {
            Key::Letter('Z') if shift => C::Redo,
            Key::Letter('Z') => C::Undo,
            Key::Letter('Y') => C::Redo,
            Key::Letter('C') if shift => C::CopyTrack,
            Key::Letter('C') => C::CopyStep,
            Key::Letter('X') => C::Cut,
            Key::Letter('R') => C::Record,
            Key::Comma => C::Preferences,
            Key::Letter('A') => C::SelectAll,
            Key::Letter('V') if shift => C::PasteTrack,
            Key::Letter('V') => C::Paste,
            Key::Letter('S') if shift => C::ExportPatternJson,
            Key::Letter('S') => C::Save,
            Key::Letter('O') => C::LoadSong,
            Key::Letter('E') if shift => C::ExportSongWav,
            Key::Letter('E') => C::ExportWav,
            // Cmd+M is the Mac's "minimize the window": the export takes Shift there.
            Key::Letter('M') if shift || !MACOS => C::ExportMidi,
            Key::Letter('K') => C::KitEditor,
            Key::Letter('N') => C::NewPattern,
            Key::Letter('D') => C::DuplicatePattern,
            Key::Letter('T') if shift => C::ToggleToasts,
            Key::Letter('T') => C::AddTrack,
            Key::Backspace | Key::Delete => C::RemoveTrack,
            Key::Letter('G') => C::NextGrid,
            // The zoom of everywhere else, the keypad's keys included; the
            // pattern length has no key (the menus and the song panel).
            Key::Minus | Key::NumpadSubtract => C::ZoomOut,
            Key::Equal | Key::NumpadAdd => C::ZoomIn,
            Key::Digit(0) => C::ZoomReset,
            // The views F1–F7 switch, on the keyboards whose F keys want Fn.
            Key::Digit(n @ 1..=7) => view_command(n),
            Key::Letter('B') if shift => C::NewGeneratedPattern,
            Key::Letter('Q') | Key::Letter('W') => C::Quit,
            _ => return None,
        }
    } else {
        match code {
            Key::F(1) => C::Shortcuts,
            Key::F(2) => C::TrackEditor,
            Key::F(3) => C::SongView,
            Key::F(4) => C::PianoRoll,
            Key::F(5) => C::Grid,
            Key::F(6) => C::SynthEditor,
            Key::F(7) => C::NoteEditor,
            Key::F(8) => C::TapTempo,
            Key::F(11) => C::Fullscreen,
            Key::F(9) => C::Metronome,
            Key::BracketLeft if shift => C::PreviousSong,
            Key::BracketRight if shift => C::NextSong,
            Key::BracketLeft | Key::PageUp => C::PreviousPattern,
            Key::BracketRight | Key::PageDown => C::NextPattern,
            Key::Comma => C::PreviousKit,
            Key::Period => C::NextKit,
            Key::Letter('B') if shift => C::GeneratePattern,
            Key::Letter('F') => C::SelectRandomPattern,
            // What changes every track wants Shift: a slip off the play keys (Q-P, 1-9) does nothing.
            // No letter that some layout puts on the play keys (A and Z on AZERTY).
            Key::Letter('G') if shift => C::SelectRandomDrumkit,
            Key::Letter('V') if shift => C::ToggleVus,
            Key::Letter('C') if shift => C::NextColorScheme,
            Key::Letter('H') if shift => C::ConvertToSynth,
            Key::Letter('J') if shift => C::AutoAssignAll,
            Key::Letter('K') if shift => C::RandomSamples,
            Key::Letter('D') if shift => C::GenerateDrums,
            // Solo and mute of the track under the cursor, as every DAW spells them.
            Key::Letter('S') if shift => C::GenerateBass,
            Key::Letter('S') => C::Solo,
            Key::Letter('M') => C::Mute,
            Key::Letter('X') if shift => C::RandomTrack,
            Key::Letter('L') => C::ToggleAccent,
            _ => return None,
        }
    })
}

/// Whether `title` is the menu `c` opens with Alt: its first letter, or
/// Play (whose P is the Pattern's) by its Y.
pub(super) fn menu_letter(title: &str, c: char) -> bool {
    let lower = c.to_ascii_lowercase();
    title.to_lowercase().starts_with(lower) || (lower == 'y' && title == "Play")
}

/// The command F(n) and Ctrl+n run: the panels, in their menu order.
pub(super) fn view_command(n: u8) -> Command {
    match n {
        2 => Command::TrackEditor,
        3 => Command::SongView,
        4 => Command::PianoRoll,
        5 => Command::Grid,
        6 => Command::SynthEditor,
        7 => Command::NoteEditor,
        _ => Command::Shortcuts,
    }
}
