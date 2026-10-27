//! Playback engine of orDrumbox: sample-accurate sequencer scheduled inside
//! the audio callback, mixer, song (arrangement) playback, and one render path
//! shared by real time and offline export (WAV).
//!
//! Ported from `src/audio/` and `src/logic/sequencer.js` of ordrumbox-v2 in
//! phases 3 and 6.
