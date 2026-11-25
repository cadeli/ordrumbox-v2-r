//! The transport the engine follows (a MIDI clock's) and the live mix
//! on top of the tracks' (the MIDI controls' knobs, the stems' split).

use ordrumbox_core::steps::TICK;
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_dsp::StripSettings;

/// The transport followed (a MIDI clock's) at the start of a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTransport {
    pub playing: bool,
    pub bpm: f64,
    /// Beats since the transport started.
    pub beat: f64,
}

impl HostTransport {
    /// Playing at a place the transport can follow (not in a pre-roll before beat 0).
    pub fn rolling(&self) -> bool {
        self.playing && self.beat.is_finite() && self.beat >= 0.0
    }
}

/// The pass that beat `beat` of the transport followed falls in, passes of `ticks` ticks each, and how
/// far into it (0 to 1); `None` before beat 0 or for an empty pass.
pub fn host_pass(beat: f64, ticks: u32) -> Option<(u64, f64)> {
    if !beat.is_finite() || beat < 0.0 || ticks == 0 {
        return None;
    }
    let pass_beats = f64::from(ticks) / f64::from(TICK);
    let pass = (beat / pass_beats).floor();
    Some((pass as u64, (beat / pass_beats - pass).clamp(0.0, 1.0)))
}

// One bit per track in a mute mask.
const _: () = assert!(MAX_TRACKS <= u64::BITS as usize);

/// Whether the mute mask `muted` (bit `t` for track `t`) mutes `track`;
/// the same for any track mask.
pub fn host_muted(muted: u64, track: usize) -> bool {
    u32::try_from(track).ok().and_then(|t| muted.checked_shr(t)).is_some_and(|m| m & 1 == 1)
}

/// Tracks the live mix changes ([`HostTrack`]), from the first.
pub const HOST_TRACKS: usize = 16;

/// The live mix, on top of the pattern's, reached through the
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
    /// Added to the echo return's feedback, the sum kept within its range.
    pub echo_feedback: f32,
    /// The changes to each of the first tracks.
    pub tracks: [HostTrack; HOST_TRACKS],
}

impl Default for HostMix {
    fn default() -> Self {
        Self { gain: 1.0, muted: 0, split: 0, echo_feedback: 0.0, tracks: [HostTrack::default(); HOST_TRACKS] }
    }
}

/// The live mix's changes to one track, on top of its own settings: at
/// their defaults, none. The sends are added to the track's and kept within
/// 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTrack {
    /// A gain on the volume (linear).
    pub gain: f32,
    /// Added to the pan, the sum kept within -1 to 1.
    pub pan: f32,
    /// Octaves the filter cutoff moves by.
    pub cutoff_octaves: f32,
    /// Added to the sends to the echo and reverb returns.
    pub echo_send: f32,
    pub reverb_send: f32,
}

impl Default for HostTrack {
    fn default() -> Self {
        Self { gain: 1.0, pan: 0.0, cutoff_octaves: 0.0, echo_send: 0.0, reverb_send: 0.0 }
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
    /// multiplied by `cutoff_ratio` (computed when the mix changes: it costs an `exp2`).
    #[must_use]
    pub(super) fn strip(&self, target: &StripSettings, track: usize, cutoff_ratio: f32) -> StripSettings {
        let mut s = *target;
        s.volume = self.volume(target.volume, track);
        let Some(live) = self.tracks.get(track) else { return s };
        let add = |value: f32, offset: f32| (value + offset).clamp(0.0, 1.0);
        s.pan = (s.pan + live.pan).clamp(-1.0, 1.0);
        if cutoff_ratio != 1.0 {
            s.cutoff = (s.cutoff * cutoff_ratio).clamp(MIN_CUTOFF, MAX_CUTOFF);
        }
        if live.echo_send != 0.0 {
            s.echo_send = add(s.echo_send, live.echo_send);
        }
        if live.reverb_send != 0.0 {
            s.reverb_send = add(s.reverb_send, live.reverb_send);
        }
        s
    }
}

/// The filter's range, as the strip takes it.
pub(super) const MIN_CUTOFF: f32 = ordrumbox_dsp::MIN_FILTER_FREQ as f32;
pub(super) const MAX_CUTOFF: f32 = ordrumbox_dsp::MAX_FILTER_FREQ as f32;
