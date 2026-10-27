//! Core of orDrumbox: data model, `song.json` compatibility layer and pattern
//! engine. No I/O, no audio, no third-party dependency.
//!
//! Ported from `src/core/`, `src/model/` and `src/patterns/` of ordrumbox-v2.
//! Loading follows `patterns/fixer.js`, saving follows `patterns/exporter.js`,
//! checked against files produced by the JS app (`tests/fixtures/`).

pub mod drum_taxonomy;
pub mod ids;
pub mod js;
pub mod json;
pub mod library;
pub mod note;
pub mod pattern;
mod record;
pub mod rng;
pub mod song;
pub mod track;

pub use library::{Library, LoadReport};
pub use note::Note;
pub use pattern::Pattern;
pub use record::{Issue, JsonField};
pub use song::{DroppedClip, Song, SongClip};
pub use track::Track;
