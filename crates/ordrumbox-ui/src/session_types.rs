//! The small types the session's modules share: what the window shows, and the keys it reads.

/// What the main editor (top-left) shows under the header. The kit editor
/// opens over any of them; the track editor (right) and the output strip
/// (bottom) are always available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Grid,
    /// The piano roll (F4): the track editor's track.
    PianoRoll,
    /// The song panel (F3): the song, what plays in song mode.
    Song,
    /// The synth editor (F6): the track editor's track's preset.
    Synth,
    /// The mixer (F12): every track's level, pan and sends, the shared
    /// returns and the master bus.
    Mixer,
}

/// What the mode switch chose, and Space plays: the selected pattern, in a
/// loop, or the selected song. Only the mode buttons change it; the views
/// leave it as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Pattern,
    Song,
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
