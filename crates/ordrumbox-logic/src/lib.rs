//! Editing layer of orDrumbox: the library being edited, every edit as an
//! undo step, the pattern generators (drum lines from a style), and Standard
//! MIDI Files.

pub mod block;
pub mod compose;
pub mod composition;
pub mod demo_catalog;
pub mod editor;
pub mod generate;
pub mod history;
pub mod humanize;
pub mod midi;
pub mod new_demos;
pub mod quantize;
pub mod roll_block;
pub mod tools;

pub use editor::{Editor, Nudge, Played};
pub use history::History;
