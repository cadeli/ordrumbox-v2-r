//! Core of orDrumbox: data model, `song.json` compatibility layer and pattern
//! engine. No I/O, no audio, no third-party dependency.
//!
//! It reads the files of the web version as well (`tests/fixtures/`).

pub mod arp;
pub mod automation;
pub mod chance;
pub mod drum_taxonomy;
pub mod effect_types;
pub mod euclidean;
pub mod ids;
pub mod instruments;
pub mod json;
pub mod lfo;
pub mod library;
pub mod note;
mod note_params;
pub mod note_positions;
pub mod num;
pub mod pattern;
pub mod pattern_engine;
mod record;
pub mod retrigger_spacing;
pub mod rng;
pub mod scales;
pub mod song;
pub mod steps;
pub mod style;
pub mod tags;
pub mod track;
mod variation;

pub use automation::{AutomationKind, AutomationLane, AutomationPoint, AutomationStroke, AutomationTarget};
pub use chance::Chance;
pub use effect_types::{DelayType, FilterType};
pub use library::{Library, LoadReport, ProjectRecord};
pub use note::Note;
pub use pattern::Pattern;
pub use pattern_engine::{Hit, Hits, expand_hits};
pub use record::{Issue, JsonField, SetField, refusal};
pub use retrigger_spacing::RetriggerSpacing;
pub use rng::Rng;
pub use scales::{ScaleList, Scales};
pub use song::{ActiveClip, DroppedClip, Song, SongClip};
pub use style::{Macros, Style, StylePart, Styles};
pub use track::Track;
