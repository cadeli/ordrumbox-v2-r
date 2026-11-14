//! Following a MIDI clock: the pulses, Start, Continue, Stop and song
//! positions of another machine, as the [`HostTransport`] the engine follows
//! under a plugin host. A pure state machine: whoever reads the MIDI port
//! says when each message came (seconds on any clock), and the audio callback
//! asks where the transport is now.

use crate::engine::HostTransport;
use crate::midi::ClockMessage;

/// Pulses in a beat.
const PULSES_PER_BEAT: f64 = 24.0;
/// Pulses in a song position unit (a sixteenth note), as a fraction of a beat.
const POSITION_BEATS: f64 = 0.25;
/// How far a pulse interval pulls the tempo estimate (0 to 1).
const SMOOTHING: f64 = 0.05;
/// A gap between two pulses longer than this is a pause, not a tempo (a
/// tempo of 20 beats a minute has a pulse every 0.125 s).
const LONGEST_INTERVAL: f64 = 0.5;
/// Without a pulse for this long, the clock is lost and the transport stops.
const STALE_SECONDS: f64 = 1.0;
/// The tempo reported moves only past this many beats a minute, to a tenth:
/// the jitter of the pulses does not rebuild every pass.
const TEMPO_STEP: f64 = 0.3;

/// What the clock of another machine says.
#[derive(Debug, Clone, Default)]
pub struct ClockFollower {
    /// Start or Continue came, and no Stop since.
    running: bool,
    /// Started, and the first pulse is not there yet: it is the position's.
    awaiting: bool,
    /// Beats at the last pulse (or at the song position set).
    beat: f64,
    last_pulse: Option<f64>,
    /// Seconds a pulse lasts, smoothed.
    interval: Option<f64>,
    reported_bpm: Option<f64>,
}

impl ClockFollower {
    #[must_use]
    pub const fn new() -> Self {
        Self { running: false, awaiting: false, beat: 0.0, last_pulse: None, interval: None, reported_bpm: None }
    }

    /// `message` came at `at` seconds.
    pub fn message(&mut self, message: ClockMessage, at: f64) {
        match message {
            ClockMessage::Pulse => self.pulse(at),
            ClockMessage::Start => {
                (self.running, self.awaiting, self.beat) = (true, true, 0.0);
            }
            ClockMessage::Continue => (self.running, self.awaiting) = (true, true),
            ClockMessage::Stop => (self.running, self.awaiting) = (false, false),
            ClockMessage::Position(sixteenths) => self.beat = f64::from(sixteenths) * POSITION_BEATS,
        }
    }

    fn pulse(&mut self, at: f64) {
        if let Some(last) = self.last_pulse {
            let gap = at - last;
            if gap > 0.0 && gap < LONGEST_INTERVAL {
                self.interval = Some(self.interval.map_or(gap, |i| i + (gap - i) * SMOOTHING));
            }
        }
        self.last_pulse = Some(at);
        if self.running && !std::mem::take(&mut self.awaiting) {
            self.beat += 1.0 / PULSES_PER_BEAT;
        }
    }

    /// Whether the other machine plays (and its clock is there).
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The transport at `now` seconds: its tempo (`fallback_bpm` until two
    /// pulses gave one) and its place in beats from the song's start. It
    /// plays from the first pulse after a Start until a Stop, or until the
    /// clock stops coming.
    pub fn transport(&mut self, now: f64, fallback_bpm: f64) -> HostTransport {
        let raw = self.interval.map_or(fallback_bpm, |interval| 60.0 / (interval * PULSES_PER_BEAT));
        let bpm = match self.reported_bpm {
            Some(reported) if (reported - raw).abs() <= TEMPO_STEP => reported,
            _ => (raw * 10.0).round() / 10.0,
        };
        self.reported_bpm = Some(bpm);
        let live = self.last_pulse.filter(|&last| now - last <= STALE_SECONDS);
        match live {
            Some(last) if self.running && !self.awaiting => {
                let beat = self.beat + (now - last).max(0.0) * bpm / 60.0;
                HostTransport { playing: true, bpm, beat }
            }
            _ => HostTransport { playing: false, bpm, beat: self.beat },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 120 beats a minute: a pulse every 1/48 s.
    const PULSE: f64 = 60.0 / 120.0 / 24.0;

    fn pulses(clock: &mut ClockFollower, count: usize, from: f64) -> f64 {
        for n in 0..count {
            clock.message(ClockMessage::Pulse, from + n as f64 * PULSE);
        }
        from + count as f64 * PULSE
    }

    #[test]
    fn it_follows_the_tempo_and_the_beat_from_a_start() {
        let mut clock = ClockFollower::new();
        let t = pulses(&mut clock, 5, 0.0);
        assert!(!clock.transport(t, 100.0).playing, "not started");
        clock.message(ClockMessage::Start, t);
        assert!(!clock.transport(t, 100.0).playing, "waits for the first pulse");
        let t = pulses(&mut clock, 25, t);
        let host = clock.transport(t, 100.0);
        assert!(host.playing);
        assert_eq!(host.bpm, 120.0);
        // 25 pulses: the first is beat 0, the 25th is one beat on; `t` is a pulse past it.
        assert!((host.beat - (1.0 + 1.0 / 24.0)).abs() < 1e-9, "{}", host.beat);
    }

    #[test]
    fn the_beat_between_two_pulses_is_extrapolated_at_the_tempo() {
        let mut clock = ClockFollower::new();
        pulses(&mut clock, 4, 0.0);
        clock.message(ClockMessage::Start, 3.0 * PULSE);
        let last = pulses(&mut clock, 1, 4.0 * PULSE) - PULSE;
        let at_pulse = clock.transport(last, 100.0).beat;
        let later = clock.transport(last + 0.25, 100.0).beat;
        assert!((later - at_pulse - 0.5).abs() < 1e-9, "a quarter second at 120 is half a beat");
    }

    #[test]
    fn stop_holds_the_position_and_continue_goes_on_from_it() {
        let mut clock = ClockFollower::new();
        pulses(&mut clock, 3, 0.0);
        clock.message(ClockMessage::Start, 0.1);
        let t = pulses(&mut clock, 49, 0.1);
        clock.message(ClockMessage::Stop, t);
        let stopped = clock.transport(t, 100.0);
        assert!(!stopped.playing);
        assert!((stopped.beat - 2.0).abs() < 1e-9, "48 pulses on from the first: two beats");
        clock.message(ClockMessage::Position(16), t);
        assert_eq!(clock.transport(t, 100.0).beat, 4.0, "16 sixteenths are 4 beats");
        clock.message(ClockMessage::Continue, t);
        pulses(&mut clock, 1, t);
        let host = clock.transport(t, 100.0);
        assert!(host.playing);
        assert!((host.beat - 4.0).abs() < 0.03, "the first pulse is the position's");
    }

    #[test]
    fn a_clock_that_stops_coming_stops_the_transport() {
        let mut clock = ClockFollower::new();
        clock.message(ClockMessage::Start, 0.0);
        let t = pulses(&mut clock, 10, 0.0);
        assert!(clock.transport(t, 100.0).playing);
        assert!(!clock.transport(t + 2.0, 100.0).playing);
    }

    #[test]
    fn the_tempo_keeps_its_value_through_jitter_and_moves_with_a_change() {
        let mut clock = ClockFollower::new();
        let mut t = 0.0;
        for n in 0..100 {
            t += PULSE * if n % 2 == 0 { 1.05 } else { 0.95 };
            clock.message(ClockMessage::Pulse, t);
        }
        let bpm = clock.transport(t, 100.0).bpm;
        assert!((bpm - 120.0).abs() < 0.5, "{bpm}");
        let fast = 60.0 / 150.0 / 24.0;
        for _ in 0..300 {
            t += fast;
            clock.message(ClockMessage::Pulse, t);
        }
        let bpm = clock.transport(t, 100.0).bpm;
        assert!((bpm - 150.0).abs() < 0.5, "{bpm}");
    }

    #[test]
    fn before_two_pulses_the_fallback_tempo_is_the_clock_s() {
        let mut clock = ClockFollower::new();
        assert_eq!(clock.transport(0.0, 97.0).bpm, 97.0);
    }
}
