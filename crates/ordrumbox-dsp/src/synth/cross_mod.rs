//! How oscillator 1 shapes oscillator 2: hard sync, which restarts the cycle
//! of oscillator 2 with each cycle of oscillator 1, and ring modulation,
//! which multiplies the two.

use super::MAX_UNISON;
use super::oscillator::phase_step;
use super::unison::Layer;

/// Where the layers of an oscillator stood before a sample and stand after it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Moved<'a> {
    pub phases: &'a [f64],
    pub cycles: &'a [f64],
    pub cycles_before: &'a [f64],
}

/// For each layer of `osc1` (at `frequency` Hz, spread by `layers`) that
/// began a cycle during the last sample: how long ago, as a share of the
/// sample (0–1).
#[inline]
#[must_use]
pub(super) fn cycle_starts(
    osc1: Moved,
    layers: &[Layer],
    frequency: f64,
    sample_rate: f64,
) -> [Option<f64>; MAX_UNISON] {
    let mut starts = [None; MAX_UNISON];
    let moved = osc1.phases.iter().zip(osc1.cycles).zip(osc1.cycles_before);
    for ((start, ((&phase, &cycle), &before)), layer) in starts.iter_mut().zip(moved).zip(layers) {
        if cycle == before {
            continue;
        }
        let dt = phase_step(frequency * layer.ratio, sample_rate).abs();
        // Forwards, the cycle began `phase` ago; backwards (through-zero FM), `1 - phase` ago.
        let since = if cycle > before { phase } else { 1.0 - phase };
        *start = Some(if dt > 0.0 { (since / dt).min(1.0) } else { 0.0 });
    }
    starts
}

/// Restart each layer of oscillator 2 (`phases` and `cycles`, at `frequency`
/// Hz) whose layer of oscillator 1 began a cycle (`starts`): it is set where
/// its next step brings it to the share of a cycle it has played since.
#[inline]
pub(super) fn restart(
    (phases, cycles): (&mut [f64], &mut [f64]),
    starts: &[Option<f64>; MAX_UNISON],
    layers: &[Layer],
    frequency: f64,
    sample_rate: f64,
) {
    for (((phase, cycle), start), layer) in phases.iter_mut().zip(cycles.iter_mut()).zip(starts).zip(layers) {
        if let Some(since) = *start {
            let dt = phase_step(frequency * layer.ratio, sample_rate);
            *phase = (since - 1.0) * dt;
            *cycle += 1.0;
        }
    }
}

/// Oscillator 2's `value` ring-modulated by oscillator 1's `carrier` at
/// `amount` (0: unchanged, 1: the product of the two).
#[inline]
#[must_use]
pub(super) fn ring(value: f64, carrier: f64, amount: f64) -> f64 {
    value * (1.0 - amount + amount * carrier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_that_began_a_cycle_restarts_the_other() {
        // Oscillator 1 at 100 Hz on 1000 Hz: a tenth of a cycle a sample, past its start by 0.05.
        let osc1 = Moved { phases: &[0.05, 0.5], cycles: &[3.0, 2.0], cycles_before: &[2.0, 2.0] };
        let layers = [Layer::SINGLE; 2];
        let starts = cycle_starts(osc1, &layers, 100.0, 1000.0);
        assert!((starts[0].unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(starts[1], None);
        let (mut phases, mut cycles) = ([0.7, 0.7], [5.0, 5.0]);
        restart((&mut phases, &mut cycles), &starts, &layers, 300.0, 1000.0);
        // Its next step (0.3) brings it to half a step into its new cycle.
        assert!((phases[0] + 0.3 - 0.15).abs() < 1e-12);
        assert_eq!((phases[1], cycles), (0.7, [6.0, 5.0]));
    }

    #[test]
    fn ring_goes_from_the_oscillator_to_the_product() {
        assert_eq!(ring(0.5, -0.4, 0.0), 0.5);
        assert_eq!(ring(0.5, -0.4, 1.0), -0.2);
    }
}
