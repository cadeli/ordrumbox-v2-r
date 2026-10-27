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
            $( $(#[$fmeta:meta])* $field:ident = $default:expr, $min:literal ..= $max:literal; )*
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

pub mod lfo;
pub mod master_bus;
pub mod sample;
pub mod strip;
pub mod synth;
pub mod voice;

pub use master_bus::{MasterBus, MasterSettings};
pub use sample::SampleBuffer;
pub use strip::{Strip, StripSettings};
pub use synth::{SynthPatch, SynthVoice};
pub use voice::{SampleVoice, VoiceStart};

/// `Math.round`: halves round up (toward +∞).
#[inline]
pub(crate) fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// ECMAScript `ToInt32` (`x | 0`).
#[inline]
pub(crate) fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let wrapped = x.trunc().rem_euclid(4_294_967_296.0);
    let signed = if wrapped >= 2_147_483_648.0 { wrapped - 4_294_967_296.0 } else { wrapped };
    signed as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_helpers() {
        assert_eq!([js_round(2.5), js_round(-2.5), js_round(-0.2), js_round(0.49)], [3.0, -2.0, 0.0, 0.0]);
        assert_eq!(to_int32(4_294_967_297.0), 1);
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
        assert_eq!(to_int32(-1.9), -1);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(1e20), (1e20f64 % 4_294_967_296.0) as i64 as i32);
    }
}
