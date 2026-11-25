//! The small types the session's modules share: what the window shows, and the keys it reads.

use crate::panel::Step;
use crate::scale::Scale;
use crate::view;

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

/// What the wheel does over the grid or the piano roll, from the keys held:
/// Ctrl zooms the beats around the pointer, Shift scrolls them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelMode {
    /// The view's own scroll (the piano roll's keyboard; none for the grid).
    Plain,
    Zoom,
    Scroll,
}

/// Keys a panel over the window reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Enter,
    /// Backspace: the parent folder.
    Back,
    Move(isize),
    /// ← / →: the value of a setting, the one before or after.
    Side(Step),
    /// Home (false) / End (true).
    Edge(bool),
    Letter(char),
}

/// What a key does in a modal of rows whose last row runs it (Enter there;
/// Enter elsewhere, like →, changes the row's value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowKey {
    Close,
    /// The row to choose.
    Row(usize),
    Change(Step),
    Run,
    Ignored,
}

/// Where a click falls on a modal of rows drawn as a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowClick {
    Outside,
    Row(usize),
    /// Inside, on no row.
    Elsewhere,
}

impl RowClick {
    /// The click at `(x, y)` on the notice `title`, `lines` of a modal of
    /// `row_count` rows, whose first line (what it acts on) is no row.
    pub(super) fn at(
        window: (i32, i32),
        (title, lines): (&str, &[String]),
        scale: Scale,
        (x, y): (i32, i32),
        row_count: usize,
    ) -> Self {
        if !view::notice_contains(window, title, lines, scale, x, y) {
            return Self::Outside;
        }
        let line = view::notice_line_at(window, title, lines, scale, x, y);
        line.and_then(|line| line.checked_sub(1)).filter(|&row| row < row_count).map_or(Self::Elsewhere, Self::Row)
    }
}

impl OverlayKey {
    /// What the key does in a modal of rows `0..=last_row`, `row` chosen.
    pub(super) fn row_key(self, row: usize, last_row: usize) -> RowKey {
        match self {
            Self::Escape => RowKey::Close,
            Self::Move(delta) => RowKey::Row(row.saturating_add_signed(delta).min(last_row)),
            Self::Edge(end) => RowKey::Row(if end { last_row } else { 0 }),
            Self::Side(step) => RowKey::Change(step),
            Self::Enter if row == last_row => RowKey::Run,
            Self::Enter => RowKey::Change(Step::Next),
            Self::Back | Self::Letter(_) => RowKey::Ignored,
        }
    }
}
