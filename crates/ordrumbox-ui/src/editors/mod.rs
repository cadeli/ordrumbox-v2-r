//! The track editor (`ui/track_editor.js` and its sections), the note editor
//! (`ui/note_editor.js`, `note_editor/sync_section.js`) and the kit editor's
//! controls (`ui/drumkit_manager/detail_section.js`), as [`panel`] items:
//! what each shows, in the JS order, with the JS ranges, defaults and
//! formats. What the widgets do is in the session.

use ordrumbox_core::{Note, Track};

use crate::panel::Kind;
pub use crate::sample_overlay::{Overlay, Shape};

// ─── Views ───────────────────────────────────────────────────────────────────

/// What the sample bar shows.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBar {
    pub info: String,
    pub envelope: Vec<f64>,
    /// The sample's playback drawn over its waveform.
    pub overlay: Option<Overlay>,
}

/// A select's options and the one shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Choice {
    pub values: Vec<String>,
    pub labels: Vec<String>,
    pub selected: usize,
}

impl Choice {
    pub fn kind(&self) -> Kind {
        let labels = if self.labels.is_empty() { self.values.clone() } else { self.labels.clone() };
        Kind::Select { values: self.values.clone(), labels, selected: self.selected }
    }
}

/// What the track editor shows of a track, beyond its settings.
#[derive(Debug, Clone)]
pub struct TrackView<'a> {
    pub track: &'a Track,
    /// `Track: NAME - kit/sample`.
    pub title: String,
    pub sample: Option<SampleBar>,
    /// The sample's decay (0 without one).
    pub decay: f64,
    /// How the sample is read and shaped (its length 0 without a sample).
    pub shape: Shape,
    pub instruments: Choice,
    /// The tracks of the pattern that can key this one (a sidechain), `none` first.
    pub sidechains: Choice,
    pub samples: Choice,
    pub synths: Choice,
    /// LFO values while playing, by setting.
    pub lfo: Vec<(&'static str, f64)>,
    /// Tempo the track plays at (the delay's beats may not fit at it).
    pub bpm: f64,
    /// A drum track: how it plays its pattern's style.
    pub compose: Option<ComposeView>,
}

/// What the Compose sub-tab shows of a drum track.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposeView {
    /// The pattern's style.
    pub style: String,
    /// The part the track plays in it, its settings with and without the
    /// pattern's macros, the hits a measure can hold.
    pub shown: ordrumbox_logic::compose::TrackCompose,
}

/// What the note editor shows.
#[derive(Debug, Clone)]
pub struct NoteView {
    /// `NAME [beat b step s]`, `— no note` when the step has none.
    pub title: String,
    /// The note (a default one when the track has none).
    pub note: Option<Note>,
    pub scales: Choice,
    pub arp_direction: Choice,
    pub arp_range: usize,
    pub arp_octaves: usize,
}

/// What the kit editor's controls show.
#[derive(Debug, Clone)]
pub struct KitControls {
    /// The kit to move the sample to (the kit it is in, selected).
    pub kits: Choice,
    pub gain: f64,
    pub transpose: f64,
    pub decay: f64,
    pub shape: Shape,
}

mod layout;
mod tables;
#[cfg(test)]
mod tests;
mod values;

pub use layout::*;
pub use tables::*;
pub use values::*;
