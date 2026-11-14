//! The sound sources of a synth voice: the band-limited oscillators, their
//! FM routings and the noise generator.

use std::f64::consts::TAU;
use std::sync::OnceLock;

use crate::lfo::{self, Wave};

pub(super) const SINE_TABLE_SIZE: usize = 4096;
pub(super) type SineTable = [f32; SINE_TABLE_SIZE];

/// Initial state of the noise generator.
pub(super) const NOISE_SEED: u32 = 54_321;
/// Frequency deviation of a full FM amount, as a multiple of the carrier's
/// frequency: the timbre stays the same over the keyboard.
pub const FM_DEVIATION: f64 = 2.0;

/// How much of oscillators 1, 2 and 3 modulates oscillator 1, then
/// oscillator 2, for each FM algorithm: 2 → 1; 3 → 1; 2 → 1 and 3 → 2;
/// 2 + 3 → 1; 2 → 1 and 1 → 2.
const FM_ROUTINGS: [[[f64; 3]; 2]; 5] = [
    [[0.0, 1.0, 0.0], [0.0, 0.0, 0.0]],
    [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0]],
    [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    [[0.0, 1.0, 1.0], [0.0, 0.0, 0.0]],
    [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
];

/// Initial noise state of the note numbered `serial`: every note has its own,
/// the same from one render to the next. A xorshift never leaves a state of 0.
pub(super) fn noise_seed(serial: u64) -> u32 {
    // The low 32 bits of the product.
    let seed = NOISE_SEED ^ serial.wrapping_sub(1).wrapping_mul(0x9E37_79B1) as u32;
    if seed == 0 { NOISE_SEED } else { seed }
}

/// Frequencies of oscillators 1 and 2 under FM of `amount`, routed by
/// `algorithm` (0 to 4, already rounded) from the raw `oscillators`.
#[inline]
pub(super) fn fm_frequencies(amount: f64, algorithm: f64, (f1, f2): (f64, f64), [o1, o2, o3]: [f64; 3]) -> (f64, f64) {
    // `as` saturates and maps NaN to 0: the first routing.
    let [to_1, to_2] = FM_ROUTINGS[(algorithm as usize).min(FM_ROUTINGS.len() - 1)];
    let (d1, d2) = (f1 * amount * FM_DEVIATION, f2 * amount * FM_DEVIATION);
    (f1 + (to_1[0] * o1 + to_1[1] * o2 + to_1[2] * o3) * d1, f2 + (to_2[0] * o1 + to_2[1] * o2 + to_2[2] * o3) * d2)
}

/// The sine table, built on first use: [`super::SynthVoice::default`] asks
/// for it, so the audio thread only reads it.
pub(super) fn sine_table() -> &'static SineTable {
    static TABLE: OnceLock<SineTable> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| (TAU * i as f64 / SINE_TABLE_SIZE as f64).sin() as f32))
}

/// Sine of `phase` cycles from the 4096-entry table, linearly interpolated.
#[inline]
pub(super) fn sin_lookup(table: &SineTable, phase: f64) -> f64 {
    let p = crate::wrap(phase, 1.0);
    let index = p * SINE_TABLE_SIZE as f64;
    let i = index as usize;
    let fraction = index - i as f64;
    f64::from(table[i & (SINE_TABLE_SIZE - 1)]) * (1.0 - fraction)
        + f64::from(table[(i + 1) & (SINE_TABLE_SIZE - 1)]) * fraction
}

/// PolyBLEP correction at phase `t` for a phase increment `dt`.
#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if dt <= 0.00001 || dt >= 0.5 {
        return 0.0;
    }
    if t < dt {
        let n = t / dt;
        return n + n - n * n - 1.0;
    }
    if t > 1.0 - dt {
        let n = (t - 1.0) / dt;
        return n * n + n + n + 1.0;
    }
    0.0
}

/// Marsaglia's xorshift32: the next state of the noise generator.
#[inline]
pub(super) fn xorshift32(mut state: u32) -> u32 {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    state
}

/// The noise sample of generator state `state`, in [-1, 1).
#[inline]
pub(super) fn noise_sample(state: u32) -> f64 {
    f64::from(state) / 2_147_483_648.0 - 1.0
}

/// The triangle wave at `phase` (0–1).
#[inline]
pub(super) fn triangle(phase: f64) -> f64 {
    if phase < 0.25 {
        phase * 4.0
    } else if phase < 0.75 {
        2.0 - phase * 4.0
    } else {
        phase * 4.0 - 4.0
    }
}

/// The band-limited sawtooth at `phase`, moving by `dt` a sample.
#[inline]
pub(super) fn sawtooth(phase: f64, dt: f64) -> f64 {
    phase * 2.0 - 1.0 - poly_blep(phase, dt)
}

/// The band-limited square at `phase`, moving by `dt` a sample.
#[inline]
pub(super) fn square(phase: f64, dt: f64) -> f64 {
    let level = if phase < 0.5 { 1.0 } else { -1.0 };
    level + poly_blep(phase, dt) - poly_blep((phase + 0.5) % 1.0, dt)
}

/// An oscillator of `wave` at `phase` (0–1) of its cycle number `cycle`,
/// moving by `dt` a sample.
///
/// The `match` runs for every sample: three oscillators of five waves each
/// are too many cases to give each its own loop, and the wave of a note never
/// changes, so the branch is always the same one.
#[inline]
pub(super) fn oscillator(table: &SineTable, wave: Wave, phase: f64, dt: f64, cycle: f64) -> f64 {
    match wave {
        Wave::Sine => sin_lookup(table, phase),
        Wave::Triangle => triangle(phase),
        Wave::Sawtooth => sawtooth(phase, dt),
        Wave::Square => square(phase, dt),
        Wave::Random => lfo::sample_and_hold(cycle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_table_lookup() {
        let table = sine_table();
        assert!((sin_lookup(table, 0.25) - 1.0).abs() < 1e-6);
        assert!((sin_lookup(table, -0.25) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn fm_routes_the_modulators_of_each_algorithm() {
        // Carriers at 100 and 200 Hz, a full amount: deviations of 200 and 400 Hz.
        let fm = |algorithm: f64| fm_frequencies(1.0, algorithm, (100.0, 200.0), [0.5, 0.25, -1.0]);
        assert_eq!(fm(0.0), (150.0, 200.0));
        assert_eq!(fm(1.0), (-100.0, 200.0));
        assert_eq!(fm(2.0), (150.0, -200.0));
        assert_eq!(fm(3.0), (-50.0, 200.0));
        assert_eq!(fm(4.0), (150.0, 400.0));
        assert_eq!(fm(f64::NAN), fm(0.0));
        assert_eq!(fm(9.0), fm(4.0));
    }
}
