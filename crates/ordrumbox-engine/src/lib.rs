//! Playback engine of orDrumbox: WAV decoding, drum kits, synth presets,
//! passes (a pattern pass as sample-accurate events, strip settings and LFO
//! steps) and the engine that plays them through strips and the master bus.
//!
//! Threads: the UI side ([`Controller`]) builds passes; the audio side
//! ([`AudioSide`]) renders them without allocating, locking or blocking.
//! Ported from `src/audio/` and `src/logic/sequencer.js` of ordrumbox-v2.

pub mod assign;
pub mod controller;
pub mod engine;
pub mod kit;
pub mod offline;
pub mod pass;
pub mod strip_settings;
pub mod synth_preset;
pub mod wav;

pub use controller::{AudioSide, Controller, Mode};
pub use engine::{Engine, Position};
pub use kit::{Kit, SampleBank, kits_from_json};
pub use pass::Pass;
pub use synth_preset::SynthPresets;
