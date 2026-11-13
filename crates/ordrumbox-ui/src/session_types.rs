//! The small types the session's modules share: what the window shows, and the keys it reads.

/// What the main editor (top-left) shows under the header. The kit editor
/// opens over any of them; the track editor (right) and the output strip
/// (bottom) are always available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Grid,
    /// The piano roll (F4): the track editor's track.
    PianoRoll,
    /// The song panel (F3): Space plays the song.
    Song,
    /// The synth editor (F6): the track editor's track's preset.
    Synth,
}

/// A direction (arrow key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

/// Keys a panel over the window reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Enter,
    /// Backspace: the parent folder.
    Back,
    Move(isize),
    /// ← (-1) / → (1): the value of a setting.
    Side(isize),
    /// Home (false) / End (true).
    Edge(bool),
    Letter(char),
}
