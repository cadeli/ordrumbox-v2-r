//! Audio processing of orDrumbox: sample voice, synth voice, channel strip,
//! master bus.
//!
//! Rule for the whole crate: nothing in a render path allocates, locks or
//! blocks. Ported from `src/audio/voices/` and `src/audio/worklets/processors/`
//! of ordrumbox-v2.
//!
//! The processors follow the JS AudioWorklet code operation for operation:
//! arithmetic in `f64` (JS numbers), delay and reverb memories in `f32`
//! (`Float32Array`), parameters as `f32` (`AudioParam` values), read once per
//! call (k-rate, a browser calls `process()` every 128 frames).

/// Parameter set of a processor: a struct of `f32` fields with their
/// `AudioParam` default and nominal range.
macro_rules! audio_params {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $( $(#[$fmeta:meta])* $field:ident = $default:expr, $min:expr, $max:expr; )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: f32, )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $( $field: $default, )* }
            }
        }

        impl $name {
            /// Every value moved toward `target` by one step of an exponential
            /// approach (`AudioParam.setTargetAtTime`): `keep` is
            /// `exp(-step / time_constant)`, 0 jumps to the target.
            pub fn approach(&mut self, target: &Self, keep: f32) {
                $( self.$field = target.$field + (self.$field - target.$field) * keep; )*
            }

            /// Values clamped to their nominal range, as a browser computes them.
            pub fn clamped(&self) -> Self {
                Self { $( $field: self.$field.clamp($min, $max), )* }
            }
        }
    };
}

/// `x` wrapped into `[0, period)` ([`f64::rem_euclid`]); a tiny negative `x`,
/// which `rem_euclid` rounds up to `period`, is `0`, so the result can index
/// a buffer of `period` frames. NaN stays NaN.
#[inline]
pub(crate) fn wrap(x: f64, period: f64) -> f64 {
    let r = x.rem_euclid(period);
    if r == period { 0.0 } else { r }
}

pub mod lfo;
pub mod master_bus;
pub mod sample;
pub mod slope;
pub mod strip;
pub mod synth;
pub mod voice;

pub use master_bus::{MasterBus, MasterSettings};
pub use sample::SampleBuffer;
pub use strip::{Strip, StripSettings};
pub use synth::{SynthPatch, SynthVoice};
pub use voice::{SampleVoice, VoiceStart};

/// Lowest frequency a filter cutoff can take, in Hz.
pub const MIN_FILTER_FREQ: f64 = 20.0;
/// Highest frequency a filter cutoff can take, in Hz.
pub const MAX_FILTER_FREQ: f64 = 20_000.0;
/// Aliases for the filter cutoff bounds.
pub const MIN_FILTER_CUTOFF: f64 = MIN_FILTER_FREQ;
pub const MAX_FILTER_CUTOFF: f64 = MAX_FILTER_FREQ;
pub const MIN_FILTER_FREQUENCY: f64 = MIN_FILTER_FREQ;
pub const MAX_FILTER_FREQUENCY: f64 = MAX_FILTER_FREQ;
pub const FILTER_FREQ_MIN: f64 = MIN_FILTER_FREQ;
pub const FILTER_FREQ_MAX: f64 = MAX_FILTER_FREQ;
pub const FILTER_CUTOFF_MIN: f64 = MIN_FILTER_FREQ;
pub const FILTER_CUTOFF_MAX: f64 = MAX_FILTER_FREQ;
