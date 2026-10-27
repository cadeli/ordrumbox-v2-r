//! Playback engine of orDrumbox: WAV decoding, drum kits, passes (a pattern
//! pass as sample-accurate events) and the engine that plays them.
//!
//! Threads: the UI side ([`Controller`]) builds passes; the audio side
//! ([`AudioSide`]) renders them without allocating, locking or blocking.
//! Ported from `src/audio/` and `src/logic/sequencer.js` of ordrumbox-v2.

pub mod controller;
pub mod engine;
pub mod kit;
pub mod pass;
pub mod wav;

pub use controller::{AudioSide, Controller};
pub use engine::{Engine, Position};
pub use kit::{Kit, SampleBank, kits_from_json};
pub use pass::Pass;
