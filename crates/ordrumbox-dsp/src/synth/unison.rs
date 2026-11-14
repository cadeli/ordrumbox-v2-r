//! Unison: each oscillator of a note sounds as several detuned layers, each
//! with its own phase and its place in the stereo field.

use std::f64::consts::{PI, SQRT_2};

use super::oscillator::{SineTable, sawtooth, sin_lookup, square, triangle};
use super::{cents_ratio, clamp};
use crate::lfo::{self, Wave};

/// The most layers one oscillator sounds as.
pub const MAX_UNISON: usize = 8;

/// One layer of an oscillator: how far its pitch is from the oscillator's,
/// and what it adds to each side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Layer {
    /// Frequency ratio to the oscillator's.
    pub ratio: f64,
    /// Weights of the layer in the left and right channels, its share of the
    /// level included.
    pub left: f64,
    pub right: f64,
}

impl Layer {
    /// The only layer of an oscillator that is not in unison.
    pub const SINGLE: Self = Self { ratio: 1.0, left: 1.0, right: 1.0 };
}

/// The layers of `count` (1 to [`MAX_UNISON`]) spread over `detune` cents on
/// each side of the pitch, and over `spread` (0–1) of the stereo field.
///
/// Layer 0 sits on the pitch, in the middle; the others go to either side of
/// it, one after the other (up, down, up, ...), the last one reaching `detune`
/// and `spread`. The level of the whole stays that of one layer, for layers
/// that do not sum in phase.
pub(super) fn layers(count: usize, detune: f64, spread: f64) -> [Layer; MAX_UNISON] {
    let count = count.clamp(1, MAX_UNISON);
    let mut layers = [Layer::SINGLE; MAX_UNISON];
    if count == 1 {
        return layers;
    }
    // Ranks run to count / 2 on the side where the odd layers go.
    let outermost = (count / 2) as f64;
    let level = 1.0 / (count as f64).sqrt();
    for (index, layer) in layers.iter_mut().enumerate().take(count).skip(1) {
        let rank = index.div_ceil(2) as f64;
        let position = (if index % 2 == 1 { rank } else { -rank }) / outermost;
        let angle = (position * clamp(spread, 0.0, 1.0) + 1.0) * PI / 4.0;
        *layer = Layer {
            ratio: cents_ratio(position * detune),
            left: angle.cos() * SQRT_2 * level,
            right: angle.sin() * SQRT_2 * level,
        };
    }
    layers[0] = Layer { left: level, right: level, ..Layer::SINGLE };
    layers
}

/// One sample of every layer of an oscillator at `frequency` Hz, moved on by
/// a sample and mixed to left and right. `wave` gives a layer's value from
/// its phase, its phase step and its cycle number.
#[inline]
fn sum_layers(
    phases: &mut [f64],
    cycles: &mut [f64],
    layers: &[Layer],
    frequency: f64,
    sample_rate: f64,
    wave: impl Fn(f64, f64, f64) -> f64,
) -> [f64; 2] {
    let mut sum = [0.0; 2];
    for ((phase, cycle), layer) in phases.iter_mut().zip(cycles.iter_mut()).zip(layers) {
        // A negative frequency runs the phase backwards, as for a single layer.
        let dt = clamp(frequency * layer.ratio / sample_rate, -0.49, 0.49);
        let advanced = *phase + dt;
        *cycle += advanced.floor();
        *phase = advanced.rem_euclid(1.0);
        let value = wave(*phase, dt.abs(), *cycle);
        sum[0] += value * layer.left;
        sum[1] += value * layer.right;
    }
    sum
}

/// The layers of an oscillator of `wave` moved on by a sample at `frequency`
/// Hz: `phases` and `cycles` hold one entry per layer of `layers`. The wave
/// is chosen once, and each choice runs its own tight loop.
#[inline]
pub(super) fn unison_oscillator(
    table: &SineTable,
    wave: Wave,
    phases: &mut [f64],
    cycles: &mut [f64],
    layers: &[Layer],
    frequency: f64,
    sample_rate: f64,
) -> [f64; 2] {
    match wave {
        Wave::Sine => sum_layers(phases, cycles, layers, frequency, sample_rate, |p, _, _| sin_lookup(table, p)),
        Wave::Triangle => sum_layers(phases, cycles, layers, frequency, sample_rate, |p, _, _| triangle(p)),
        Wave::Sawtooth => sum_layers(phases, cycles, layers, frequency, sample_rate, |p, dt, _| sawtooth(p, dt)),
        Wave::Square => sum_layers(phases, cycles, layers, frequency, sample_rate, |p, dt, _| square(p, dt)),
        Wave::Random => sum_layers(phases, cycles, layers, frequency, sample_rate, |_, _, c| lfo::sample_and_hold(c)),
    }
}

/// The start phase (0–1) of each layer but the first, which starts at 0: from
/// a xorshift of `seed`, so a note starts the same way every time it is
/// rendered.
pub(super) fn start_phases(seed: u32) -> [f64; MAX_UNISON] {
    let mut state = seed | 1;
    let mut phases = [0.0; MAX_UNISON];
    for phase in phases.iter_mut().skip(1) {
        state = super::oscillator::xorshift32(state);
        *phase = f64::from(state) / 4_294_967_296.0;
    }
    phases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_layer_is_the_oscillator_itself() {
        assert_eq!(layers(1, 40.0, 1.0)[0], Layer::SINGLE);
        assert_eq!(layers(0, 40.0, 1.0)[0], Layer::SINGLE);
    }

    #[test]
    fn layers_spread_to_both_sides_up_to_the_detune() {
        let l = layers(7, 30.0, 1.0);
        assert_eq!(l[0].ratio, 1.0);
        // Layers 1 and 2 are the first pair, 5 and 6 the last: ±30 cents.
        assert!((l[1].ratio - cents_ratio(10.0)).abs() < 1e-12);
        assert!((l[2].ratio - cents_ratio(-10.0)).abs() < 1e-12);
        assert!((l[5].ratio - cents_ratio(30.0)).abs() < 1e-12);
        assert!((l[6].ratio - cents_ratio(-30.0)).abs() < 1e-12);
        // The outermost layers sit fully to the right and to the left.
        assert!(l[5].right > 0.99 * SQRT_2 / 7f64.sqrt() && l[5].left < 1e-9);
        assert!(l[6].left > 0.99 * SQRT_2 / 7f64.sqrt() && l[6].right < 1e-9);
    }

    #[test]
    fn two_layers_are_the_pitch_and_one_detuned_layer() {
        let l = layers(2, 20.0, 0.0);
        assert_eq!(l[0].ratio, 1.0);
        assert!((l[1].ratio - cents_ratio(20.0)).abs() < 1e-12);
    }

    #[test]
    fn no_spread_is_the_same_on_both_sides() {
        for layer in layers(MAX_UNISON, 50.0, 0.0).iter() {
            assert!((layer.left - layer.right).abs() < 1e-12);
        }
    }

    #[test]
    fn start_phases_repeat_and_differ() {
        assert_eq!(start_phases(5), start_phases(5));
        assert_ne!(start_phases(5), start_phases(6));
        assert_eq!(start_phases(5)[0], 0.0);
        assert!(start_phases(5).iter().all(|p| (0.0..1.0).contains(p)));
    }

    use super::super::SynthVoice;
    use super::super::patch::{SynthBypass, SynthPatch};

    /// A held saw of oscillator 1 at 100 Hz, unfiltered, 1 s at 48 kHz: left and right.
    fn saw(unison_count: usize, unison_detune: f64, unison_spread: f64) -> [Vec<f32>; 2] {
        let patch = SynthPatch {
            osc_freq: [100.0; 3],
            osc_gain: [1.0, 0.0, 0.0],
            osc_wave: [Wave::Sawtooth; 3],
            volume: 1.0,
            bypass: SynthBypass { filter: true, env: true, ..SynthBypass::default() },
            unison_count,
            unison_detune,
            unison_spread,
            ..SynthPatch::default()
        };
        let mut voice = SynthVoice::default();
        voice.start(&patch, 0, 0.0, 1);
        let (mut left, mut right) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        voice.render(48_000.0, 0, &mut left, &mut right);
        [left, right]
    }

    fn rms(signal: &[f32]) -> f64 {
        (signal.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / signal.len() as f64).sqrt()
    }

    #[test]
    fn one_layer_changes_nothing_and_spread_needs_layers() {
        let single = saw(1, 0.0, 0.0);
        assert_eq!(single, saw(1, 50.0, 1.0));
        assert_eq!(single[0], single[1]);
    }

    #[test]
    fn unison_keeps_the_level_of_one_oscillator() {
        let single = rms(&saw(1, 0.0, 0.0)[0]);
        for count in [3, 7] {
            let level = rms(&saw(count, 20.0, 0.0)[0]);
            assert!((level / single - 1.0).abs() < 0.2, "{count}: {level} {single}");
        }
    }

    #[test]
    fn detuned_layers_sound_at_their_own_pitches() {
        // Two layers 1200 cents apart: an octave above the pitch as well.
        let out = &saw(2, 1200.0, 0.0)[0];
        let level = |freq: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, &x) in out.iter().enumerate() {
                let phase = std::f64::consts::TAU * freq * i as f64 / 48_000.0;
                re += f64::from(x) * phase.cos();
                im += f64::from(x) * phase.sin();
            }
            (re * re + im * im).sqrt() / out.len() as f64
        };
        assert!(level(200.0) > 0.1 && level(100.0) > 0.1 && level(150.0) < 0.02);
    }

    #[test]
    fn spread_puts_different_layers_on_each_side() {
        let [left, right] = saw(7, 30.0, 1.0);
        assert_ne!(left, right);
        let [left, right] = saw(7, 30.0, 0.0);
        assert_eq!(left, right);
        // Equal power: the sides keep about the level of the middle.
        let [left, right] = saw(7, 30.0, 1.0);
        let mid = rms(&saw(7, 30.0, 0.0)[0]);
        assert!((rms(&left) / mid - 1.0).abs() < 0.3 && (rms(&right) / mid - 1.0).abs() < 0.3);
    }

    #[test]
    fn a_unison_note_replays_the_same() {
        assert_eq!(saw(7, 30.0, 1.0), saw(7, 30.0, 1.0));
    }
}
