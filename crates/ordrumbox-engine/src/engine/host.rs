//! A plugin host's transport and mix, as the engine follows them.

use super::*;

/// A plugin host's transport at the start of a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTransport {
    pub playing: bool,
    pub bpm: f64,
    /// Beats since the start of the host's timeline.
    pub beat: f64,
}

impl HostTransport {
    /// Playing at a place the transport can follow (not in a pre-roll before beat 0).
    pub fn rolling(&self) -> bool {
        self.playing && self.beat.is_finite() && self.beat >= 0.0
    }
}

/// The pass host beat `beat` falls in, passes of `ticks` ticks each, and how
/// far into it (0 to 1); `None` before beat 0 or for an empty pass.
pub fn host_pass(beat: f64, ticks: u32) -> Option<(u64, f64)> {
    if !beat.is_finite() || beat < 0.0 || ticks == 0 {
        return None;
    }
    let pass_beats = f64::from(ticks) / f64::from(TICK);
    let pass = (beat / pass_beats).floor();
    Some((pass as u64, (beat / pass_beats - pass).clamp(0.0, 1.0)))
}

// One bit per track in the host's mute mask.
const _: () = assert!(MAX_TRACKS <= u64::BITS as usize);

/// Whether a plugin host's mute mask `muted` (bit `t` for track `t`) mutes
/// `track`; the same for any of its track masks.
pub fn host_muted(muted: u64, track: usize) -> bool {
    u32::try_from(track).ok().and_then(|t| muted.checked_shr(t)).is_some_and(|m| m & 1 == 1)
}

/// Tracks a plugin host changes ([`HostTrack`]), from the first.
pub const HOST_TRACKS: usize = 16;

/// A plugin host's mix, on top of the pattern's, reached through the
/// strips' ramps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostMix {
    /// A gain on the master volume (linear).
    pub gain: f32,
    /// The tracks muted: bit `t` for track `t`.
    pub muted: u64,
    /// The tracks left out of the mix and given apart, each on an output of
    /// its own ([`Engine::render_split`]): bit `t` for track `t`.
    pub split: u64,
    /// The changes to each of the first tracks.
    pub tracks: [HostTrack; HOST_TRACKS],
}

impl Default for HostMix {
    fn default() -> Self {
        Self { gain: 1.0, muted: 0, split: 0, tracks: [HostTrack::default(); HOST_TRACKS] }
    }
}

/// A plugin host's changes to one track, on top of its own settings: at
/// their defaults, none. The effect amounts are added to the track's (0 when
/// its LED is off) and kept within 0 to 1; an effect whose type is `none`,
/// or a filter of type `allpass`, stays off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTrack {
    /// A gain on the volume (linear).
    pub gain: f32,
    /// Added to the pan, the sum kept within -1 to 1.
    pub pan: f32,
    /// Octaves the filter cutoff moves by.
    pub cutoff_octaves: f32,
    /// Added to the filter's Q.
    pub q: f32,
    /// Added to the reverb amount.
    pub reverb: f32,
    /// Added to the delay amount.
    pub delay: f32,
    /// Added to the delay feedback (kept within 0 to 0.95; a slap delay has none).
    pub feedback: f32,
    /// Added to the saturation amount.
    pub saturation: f32,
    /// Added to the compressor's mix (0: off, 1: fully compressed).
    pub compressor: f32,
    /// Decibels added to the compressor's threshold, the sum kept within its range.
    pub threshold_db: f32,
    /// Semitones the notes started move by (not the MIDI notes sent).
    pub transpose: i32,
}

impl Default for HostTrack {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            cutoff_octaves: 0.0,
            q: 0.0,
            reverb: 0.0,
            delay: 0.0,
            feedback: 0.0,
            saturation: 0.0,
            compressor: 0.0,
            threshold_db: 0.0,
            transpose: 0,
        }
    }
}

/// The highest delay feedback a host can reach (the editor's).
pub(super) const HOST_FEEDBACK_MAX: f32 = 0.95;

/// What a [`HostTrack`] multiplies, computed when the host's mix changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HostRatios {
    pub(super) cutoff: f32,
    pub(super) pitch: f64,
}

impl HostRatios {
    pub(super) const NONE: Self = Self { cutoff: 1.0, pitch: 1.0 };

    pub(super) fn of(track: &HostTrack) -> Self {
        Self { cutoff: track.cutoff_octaves.exp2(), pitch: (f64::from(track.transpose) / 12.0).exp2() }
    }
}

impl HostMix {
    /// The volume the strip of `track`, at `volume`, goes to: none when
    /// muted, else scaled by the track's gain.
    #[must_use]
    pub(super) fn volume(&self, volume: f32, track: usize) -> f32 {
        if host_muted(self.muted, track) { 0.0 } else { volume * self.tracks.get(track).map_or(1.0, |t| t.gain) }
    }

    /// The settings the strip of `track`, at `target`, goes to, its cutoff
    /// multiplied by `ratios.cutoff`.
    #[must_use]
    pub(super) fn strip(&self, target: &StripSettings, track: usize, ratios: &HostRatios) -> StripSettings {
        let mut s = *target;
        s.volume = self.volume(target.volume, track);
        let Some(host) = self.tracks.get(track) else { return s };
        let add = |value: f32, offset: f32, max: f32| (value + offset).clamp(0.0, max);
        s.pan = (s.pan + host.pan).clamp(-1.0, 1.0);
        if ratios.cutoff != 1.0 {
            s.cutoff = (s.cutoff * ratios.cutoff).clamp(MIN_CUTOFF, MAX_CUTOFF);
        }
        if host.q != 0.0 {
            s.q = (s.q + host.q).clamp(MIN_Q, MAX_Q);
        }
        if host.reverb != 0.0 {
            s.reverb_mix = add(s.reverb_mix, host.reverb, 1.0);
        }
        if host.delay != 0.0 {
            s.delay_mix = add(s.delay_mix, host.delay, 1.0);
        }
        if host.feedback != 0.0 && !is_slap(&s) {
            s.delay_feedback = add(s.delay_feedback, host.feedback, HOST_FEEDBACK_MAX);
        }
        if host.saturation != 0.0 {
            let amount = (saturation_amount(&s) + f64::from(host.saturation)).clamp(0.0, 1.0);
            set_saturation(&mut s, amount);
        }
        if host.compressor != 0.0 {
            s.comp_mix = add(s.comp_mix, host.compressor, 1.0);
        }
        if host.threshold_db != 0.0 {
            s.comp_threshold_db = (s.comp_threshold_db + host.threshold_db).clamp(MIN_THRESHOLD_DB, MAX_THRESHOLD_DB);
        }
        s
    }
}

/// The filter's range, as the strip takes it.
pub(super) const MIN_CUTOFF: f32 = ordrumbox_dsp::MIN_FILTER_FREQ as f32;
pub(super) const MAX_CUTOFF: f32 = ordrumbox_dsp::MAX_FILTER_FREQ as f32;
pub(super) const MIN_Q: f32 = 0.707;
pub(super) const MAX_Q: f32 = 18.707;
/// The compressor's threshold range, as the strip takes it.
pub(super) const MIN_THRESHOLD_DB: f32 = StripSettings::MIN.comp_threshold_db;
pub(super) const MAX_THRESHOLD_DB: f32 = StripSettings::MAX.comp_threshold_db;
