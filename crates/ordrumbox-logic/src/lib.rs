//! Editing layer of orDrumbox: the library being edited, every edit as an
//! undo step; later the pattern generators and Standard MIDI Files.
//!
//! Ported from `src/logic/` of ordrumbox-v2 (`commands/`, `history_manager.js`).

pub mod editor;
pub mod history;

pub use editor::{Editor, Nudge};
pub use history::History;
