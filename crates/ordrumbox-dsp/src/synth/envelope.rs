//! The ADSR of a synth voice: one state for the amplitude envelope and one
//! for the modulation envelope, which differ by their peak and attack curve.

use super::clamp;
use super::patch::Adsr;

/// A time (seconds) at or below this is no time at all: the segment is skipped.
const INSTANT: f64 = 0.0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Segment {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// An envelope as it runs.
#[derive(Debug, Clone, Copy)]
pub(super) struct Envelope {
    level: f64,
    segment: Segment,
    /// When the segment started, seconds from the note's start.
    segment_start: f64,
    release_start_level: f64,
}

impl Envelope {
    /// An envelope in `segment`, at level 0.
    pub(super) const fn new(segment: Segment) -> Self {
        Self { level: 0.0, segment, segment_start: 0.0, release_start_level: 0.0 }
    }

    pub(super) fn is_idle(&self) -> bool {
        self.segment == Segment::Idle
    }

    /// Start the release at `t` (seconds from the note's start), from the
    /// level reached; an envelope already released or over is left alone.
    #[inline]
    pub(super) fn release(&mut self, t: f64) {
        if matches!(self.segment, Segment::Attack | Segment::Decay | Segment::Sustain) {
            self.release_start_level = self.level;
            self.segment = Segment::Release;
            self.segment_start = t;
        }
    }

    /// The level at `t`, for an envelope that peaks at `peak`; `attack_level`
    /// gives the level at a fraction (0–1) of the attack.
    #[inline]
    pub(super) fn step(&mut self, t: f64, adsr: Adsr, peak: f64, attack_level: impl Fn(f64) -> f64) -> f64 {
        if t < 0.0 || self.segment == Segment::Idle {
            return 0.0;
        }
        let Adsr { attack, decay, sustain, release } = adsr;
        if self.segment == Segment::Attack {
            let dt = t - self.segment_start;
            if attack <= INSTANT || dt >= attack {
                self.level = peak;
                self.segment = Segment::Decay;
                self.segment_start = t;
            } else {
                self.level = attack_level((dt / attack).max(0.0));
            }
        }
        if self.segment == Segment::Decay {
            let dt = t - self.segment_start;
            if decay <= INSTANT || dt >= decay {
                self.level = peak * sustain;
                self.segment = Segment::Sustain;
            } else {
                let norm = clamp(1.0 - dt / decay, 0.0, 1.0);
                let curve = norm * norm * norm;
                self.level = peak * (sustain + (1.0 - sustain) * curve);
            }
        }
        if self.segment == Segment::Sustain {
            self.level = peak * sustain;
        }
        if self.segment == Segment::Release {
            let dt = t - self.segment_start;
            if release <= INSTANT || dt >= release {
                self.level = 0.0;
                self.segment = Segment::Idle;
            } else {
                let norm = clamp(1.0 - dt / release, 0.0, 1.0);
                self.level = self.release_start_level * (norm * norm * norm);
            }
        }
        if self.level.is_finite() && self.level > 0.0 { self.level } else { 0.0 }
    }
}

/// The filter envelope: up over the attack, down over the decay, then off.
/// It follows the times of the amplitude envelope.
#[derive(Debug, Clone, Copy)]
pub(super) struct FilterEnvelope {
    level: f64,
    segment: Segment,
}

impl FilterEnvelope {
    pub(super) const fn new(segment: Segment) -> Self {
        Self { level: 0.0, segment }
    }

    pub(super) fn is_idle(&self) -> bool {
        self.segment == Segment::Idle
    }

    /// The level (0–1) at `t` seconds from the note's start.
    #[inline]
    pub(super) fn step(&mut self, t: f64, attack: f64, decay: f64) -> f64 {
        if self.segment == Segment::Attack {
            if attack > INSTANT && t < attack {
                self.level = t / attack;
            } else {
                self.level = 1.0;
                self.segment = Segment::Decay;
            }
        } else {
            let dt = t - attack;
            if decay > INSTANT && dt < decay {
                self.level = 1.0 - dt / decay;
            } else {
                self.level = 0.0;
                self.segment = Segment::Idle;
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADSR: Adsr = Adsr { attack: 0.1, decay: 0.1, sustain: 0.5, release: 0.2 };

    #[test]
    fn an_envelope_rises_holds_and_dies_after_its_release() {
        let mut envelope = Envelope::new(Segment::Attack);
        let at = |envelope: &mut Envelope, t: f64| envelope.step(t, ADSR, 2.0, |norm| 2.0 * norm);
        assert!((at(&mut envelope, 0.05) - 1.0).abs() < 1e-12);
        assert_eq!(at(&mut envelope, 0.1), 2.0);
        assert_eq!(at(&mut envelope, 0.3), 1.0);
        envelope.release(0.3);
        // Half of the release left: an eighth of the level it started from.
        assert!((at(&mut envelope, 0.4) - 0.125).abs() < 1e-12);
        assert_eq!(at(&mut envelope, 0.55), 0.0);
        assert!(envelope.is_idle());
        // Over, it stays over.
        envelope.release(0.6);
        assert_eq!(at(&mut envelope, 0.6), 0.0);
    }

    #[test]
    fn the_filter_envelope_goes_up_then_down_then_off() {
        let mut envelope = FilterEnvelope::new(Segment::Attack);
        assert!((envelope.step(0.05, 0.1, 0.2) - 0.5).abs() < 1e-12);
        assert_eq!(envelope.step(0.1, 0.1, 0.2), 1.0);
        assert!((envelope.step(0.2, 0.1, 0.2) - 0.5).abs() < 1e-12);
        assert!(!envelope.is_idle());
        assert_eq!(envelope.step(0.35, 0.1, 0.2), 0.0);
        assert!(envelope.is_idle());
    }
}
