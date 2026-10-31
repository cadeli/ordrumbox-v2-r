//! Measures what a sound does, for whoever cannot only listen to it: levels,
//! envelope, spectrum, harmonics, and how pitch and brightness move. A synth
//! preset is rendered by the real voice, before the channel strip and the
//! master bus; a WAV file is measured as it is.

pub mod fft;
pub mod kit;
pub mod measure;
pub mod mix;
pub mod plot;
pub mod render;
pub mod report;
