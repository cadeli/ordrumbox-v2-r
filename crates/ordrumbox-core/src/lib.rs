//! Core of orDrumbox: data model, `song.json` compatibility layer and pattern
//! engine. No I/O, no audio, no third-party dependency.
//!
//! Ported from `src/core/`, `src/model/` and `src/patterns/` of ordrumbox-v2.
//! Loading follows `patterns/fixer.js`, saving follows `patterns/exporter.js`,
//! checked against files produced by the JS app (`tests/fixtures/`).

pub mod arp;
pub mod drum_taxonomy;
pub mod euclidean;
pub mod ids;
pub mod instruments;
pub mod js;
pub mod json;
pub mod legacy;
pub mod lfo;
pub mod library;
pub mod note;
mod note_params;
pub mod note_positions;
pub mod pattern;
pub mod pattern_engine;
mod record;
pub mod rng;
pub mod scales;
pub mod song;
pub mod steps;
mod timsort;
pub mod track;
mod variation;

pub use library::{Library, LoadReport, SongRecord};
pub use note::Note;
pub use pattern::Pattern;
pub use pattern_engine::{FlatNote, FlatNotes, recompute_flat_notes};
pub use record::{Issue, JsonField, SetField};
pub use rng::Rng;
pub use scales::Scales;
pub use song::{ActiveClip, DroppedClip, Song, SongClip};
pub use track::Track;
