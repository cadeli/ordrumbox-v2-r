//! The sound sources of a synth voice: the band-limited oscillators, their
//! FM routings and the noise generator.

use std::f64::consts::TAU;
use std::sync::OnceLock;

use crate::lfo::{self, Wave};

pub(super) const SINE_TABLE_SIZE: usize = 4096;
pub(super) type SineTable = [f32; SINE_TABLE_SIZE];

/// What the oscillators read their waves from: the sine table, and the
/// sample whose frames the random wave holds, one a cycle.
#[derive(Debug, Clone, Copy)]
pub(super) struct WaveTables<'a> {
    pub sine: &'static SineTable,
    /// Frames of the sample; empty when there is none.
    pub source: &'a [f32],
}

/// Share of the sample the random wave moves on by a cycle: the golden
/// ratio's, so the frames it reads spread evenly over the whole sample and
/// two cycles in a row read frames far apart.
const GOLDEN_STEP: f64 = 0.618_033_988_749_894_8;

/// The value the random wave holds for its cycle number `cycle`: a frame of
/// `source`, [`GOLDEN_STEP`] of it further on each cycle and looping over it
/// (a cycle before the first, under through-zero FM, steps back), or a
/// pseudo-random value when there is no sample. One frame on a cycle would
/// read only the first milliseconds of the sample, mostly the silence before
/// its sound, and frames so close that the wave hardly moves.
#[inline]
pub(super) fn held_value(source: &[f32], cycle: f64) -> f64 {
    if source.is_empty() {
        return lfo::sample_and_hold(cycle);
    }
    let position = (cycle * GOLDEN_STEP).rem_euclid(1.0) * source.len() as f64;
    // `as` saturates and maps NaN to 0; `get` keeps a rounding at the end in bounds.
    let index = position as usize;
    source.get(index).map_or(0.0, |frame| f64::from(*frame))
}

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

/// Initial state of the shift register of the console noise.
pub(super) const LFSR_SEED: u16 = 1;

/// The next state of the shift register of the console noise: the two lowest
/// bits (`short`: bit 0 and bit 6, a loop of 93 steps) are combined and fed
/// back in at the top (bit 14), as the NES's noise channel does.
#[inline]
pub(super) fn lfsr_step(state: u16, is_short: bool) -> u16 {
    let tap = if is_short { 6 } else { 1 };
    let feedback = (state ^ (state >> tap)) & 1;
    (state >> 1) | (feedback << 14)
}

/// The sample of shift register state `state`: its lowest bit, as -1 or 1.
#[inline]
pub(super) fn lfsr_sample(state: u16) -> f64 {
    if state & 1 == 0 { 1.0 } else { -1.0 }
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

/// Narrowest and widest pulse, share of a cycle: the edges stay apart, so
/// the band-limiting corrections of the two never overlap.
pub(super) const WIDTH_MIN: f64 = 0.02;
pub(super) const WIDTH_MAX: f64 = 0.98;

/// The band-limited pulse at `phase`, high for the first `width` of the cycle
/// (0.5: the square), moving by `dt` a sample. A width other than a half
/// leaves a constant offset, as a console's pulse does.
#[inline]
pub(super) fn pulse(phase: f64, dt: f64, width: f64) -> f64 {
    let level = if phase < width { 1.0 } else { -1.0 };
    level + poly_blep(phase, dt) - poly_blep((phase + 1.0 - width) % 1.0, dt)
}

/// An oscillator of `wave` at `phase` (0–1) of its cycle number `cycle`,
/// moving by `dt` a sample; a square is high for `width` of the cycle.
///
/// The `match` runs for every sample: three oscillators of five waves each
/// are too many cases to give each its own loop, and the wave of a note never
/// changes, so the branch is always the same one.
#[inline]
pub(super) fn oscillator(tables: WaveTables, wave: Wave, phase: f64, dt: f64, cycle: f64, width: f64) -> f64 {
    match wave {
        Wave::Sine => sin_lookup(tables.sine, phase),
        Wave::Triangle => triangle(phase),
        Wave::Sawtooth => sawtooth(phase, dt),
        Wave::Square => pulse(phase, dt, width),
        Wave::Random => held_value(tables.source, cycle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_wave_spreads_over_the_sample() {
        // Each frame holds its own index.
        let source: Vec<f32> = (0..100u8).map(f32::from).collect();
        let held = |cycle: f64| held_value(&source, cycle);
        assert_eq!([0.0, 1.0, 2.0, 3.0, 4.0, -1.0].map(held), [0.0, 61.0, 23.0, 85.0, 47.0, 38.0]);
        let mut frames: Vec<u8> = (0..100).map(|c| held(f64::from(c)) as u8).collect();
        frames.sort_unstable();
        frames.dedup();
        assert!(frames.len() > 60, "{} frames of 100", frames.len());
        assert_eq!(held(f64::NAN), 0.0);
        assert_eq!(held_value(&[], 7.0), lfo::sample_and_hold(7.0));
    }

    #[test]
    fn sine_table_lookup() {
        let table = sine_table();
        assert!((sin_lookup(table, 0.25) - 1.0).abs() < 1e-6);
        assert!((sin_lookup(table, -0.25) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_pulse_is_high_for_its_width() {
        let share = |width: f64| (0..1000).filter(|i| pulse(f64::from(*i) / 1000.0, 0.0, width) > 0.0).count();
        assert_eq!([share(0.5), share(0.125), share(0.25)], [500, 125, 250]);
        assert_eq!(pulse(0.3, 0.01, 0.5), square_half(0.3, 0.01));
    }

    /// The square as it was before the width: a pulse at a half.
    fn square_half(phase: f64, dt: f64) -> f64 {
        let level = if phase < 0.5 { 1.0 } else { -1.0 };
        level + poly_blep(phase, dt) - poly_blep((phase + 0.5) % 1.0, dt)
    }

    #[test]
    fn the_shift_register_loops_after_32767_or_93_steps() {
        let period = |is_short: bool| {
            let mut state = lfsr_step(LFSR_SEED, is_short);
            let mut steps = 1;
            while state != LFSR_SEED {
                state = lfsr_step(state, is_short);
                steps += 1;
            }
            steps
        };
        assert_eq!(period(false), 32_767);
        assert_eq!(period(true), 93);
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
