//! Audio processing of orDrumbox: sample voice, synth voice (3 VCO, FM,
//! noise, TPT SVF, ADSR, LFO), channel strip and master bus.
//!
//! Rule for the whole crate: nothing in a `process()` path allocates, locks or
//! blocks. Ported from `src/audio/worklets/processors/` of ordrumbox-v2 in
//! phases 3 and 4.
