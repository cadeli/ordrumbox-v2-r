//! Audio processing of orDrumbox: sample voice, channel strip (and, from
//! phase 4, the synth voice and the master bus).
//!
//! Rule for the whole crate: nothing in a render path allocates, locks or
//! blocks. Ported from `src/audio/voices/` and `src/audio/worklets/processors/`
//! of ordrumbox-v2.

pub mod sample;
pub mod strip;
pub mod voice;

pub use sample::SampleBuffer;
pub use strip::StripParams;
pub use voice::{SampleVoice, VoiceStart};
