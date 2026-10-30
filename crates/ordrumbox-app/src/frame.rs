//! The window's fixed bands, shared by every view: the margin, the header
//! under the menu bar, where the body starts, and where the footer starts.

use crate::menu;

/// The window's outer margin.
pub fn margin(scale: i32) -> i32 {
    16 * scale
}

/// Top of the header: under the menu bar.
pub fn content_top(scale: i32) -> i32 {
    menu::bar_height(scale) + margin(scale) / 2
}

/// Height of the header's row of controls: a label, then the widget.
fn controls_height(scale: i32) -> i32 {
    28 * scale
}

/// Top of the header's second line (the last action, undo, the song).
pub fn status_line(scale: i32) -> i32 {
    content_top(scale) + controls_height(scale) + 4 * scale
}

/// `y` is on the header's second line.
pub fn on_status_line(y: i32, scale: i32) -> bool {
    (status_line(scale)..status_line(scale) + 10 * scale).contains(&y)
}

/// Top of what is under the header (the grid, the kit editor, the song panel).
pub fn body_top(scale: i32) -> i32 {
    status_line(scale) + 10 * scale + margin(scale) / 2
}

/// Top of the footer (status lines then help).
pub fn footer_top(height: i32, scale: i32, status_lines: usize) -> i32 {
    height - margin(scale) - 10 * scale - status_lines as i32 * 10 * scale - 4 * scale
}
