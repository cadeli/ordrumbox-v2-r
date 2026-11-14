//! Playback engine of orDrumbox: WAV decoding, drum kits, synth presets,
//! passes (a pattern pass as sample-accurate events, strip settings and LFO
//! steps) and the engine that plays them through strips and the master bus.
//!
//! Threads: the UI side ([`Controller`]) builds passes; the audio side
//! ([`AudioSide`]) renders them without allocating, locking or blocking.
//! Ported from `src/audio/` and `src/logic/sequencer.js` of ordrumbox-v2.

pub mod analysis;
pub mod assign;
pub mod clock_in;
pub mod controller;
pub mod controls;
pub mod engine;
pub mod kit;
pub mod metronome;
pub mod midi;
pub mod offline;
pub mod pass;
pub mod strip_settings;
pub mod synth_model;
pub mod synth_preset;
pub mod wav;

pub use assign::TrackSound;
pub use clock_in::ClockFollower;
pub use controller::{AudioSide, Controller, Mode, SCOPE_SIZE};
pub use controls::{Control, ControlMap, Knob};
pub use engine::{Engine, HOST_TRACKS, HostMix, HostTrack, HostTransport, Position, host_muted, host_pass};
pub use kit::{Kit, KitSample, KitsRead, SampleBank, kits_from_json, kits_to_json};
pub use midi::{BlockClock, BlockNote, ClockMessage, DRUM_CHANNEL, KeyMap};
pub use ordrumbox_dsp::{MasterSettings, SampleBuffer, gain_reduction, slope};
pub use pass::Pass;
pub use synth_preset::SynthPresets;
