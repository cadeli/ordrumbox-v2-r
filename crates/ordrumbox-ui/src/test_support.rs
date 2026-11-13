//! What the unit tests of several modules share.

use std::path::PathBuf;

use crate::scale::Scale;
use crate::session::Session;
use crate::ui_input::Button;

/// A folder of its own for test `name`, empty: removed first if an earlier run left it.
pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordrumbox-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The window size of the session tests.
pub(crate) const SIZE: (i32, i32, Scale) = (1100, 680, Scale::ONE);

/// A press of the left button at `x`, `y` in a window of `SIZE`; whether the session took it.
pub(crate) fn click(session: &mut Session, x: i32, y: i32) -> bool {
    session.mouse_down(SIZE, x, y, Button::Left, false)
}
