//! A track's settings as strip parameters (`strip_sync.js` and `strip.js`),
//! and its LFOs as per-tick parameter values (`step_lfo.js`).
//!
//! It departs from the JS where the names did not hold: `slap` is one clean
//! echo (the JS played it as `tape`), `tape` has wow and a darker feedback,
//! `pingpong` starts on the left and alternates at one time (the JS crossed
//! two times, 2/3 apart), `spring` disperses, `gated` is gated, `hall` and
//! `plate` have a pre-delay, and `allpass` skips the filter. A delay longer
//! than the line plays at the longest halving of its beats that fits.

use ordrumbox_core::Track;
use ordrumbox_core::json::Object;
use ordrumbox_core::lfo;
use ordrumbox_dsp::StripSettings;

use crate::synth_preset::FILTER_TYPES;

/// A reverb type's settings: room, damp and width (`REVERB_PRESETS`), then
/// pre-delay (seconds), dispersion and gate (seconds), which the JS lacks.
#[derive(Debug, Clone, Copy)]
struct ReverbPreset {
    room: f32,
    damping: f32,
    width: f32,
    predelay: f32,
    dispersion: f32,
    gate: f32,
}

fn reverb_preset(name: &str) -> Option<ReverbPreset> {
    let preset = |room, damping, width, predelay, dispersion, gate| ReverbPreset {
        room,
        damping,
        width,
        predelay,
        dispersion,
        gate,
    };
    Some(match name {
        "none" => preset(0.0, 0.5, 0.0, 0.0, 0.0, 0.0),
        "room" => preset(0.5, 0.5, 0.8, 0.0, 0.0, 0.0),
        "hall" => preset(0.85, 0.3, 1.0, 0.03, 0.0, 0.0),
        "plate" => preset(0.7, 0.4, 0.9, 0.01, 0.0, 0.0),
        "spring" => preset(0.45, 0.6, 0.5, 0.0, 0.6, 0.0),
        // A large bright room cut short: the drum sound of the 1980s.
        "gated" => preset(0.9, 0.2, 1.0, 0.0, 0.0, 0.25),
        _ => return None,
    })
}

/// The saturation drive added at full amount, over 1.
const SATURATION_DRIVE_RANGE: f64 = 6.0;
/// The output level taken off at full amount, to make up for the drive.
const SATURATION_OUTPUT_DROP: f64 = 0.15;

/// Longest delay the strip holds, seconds (its `delay_time_*` range).
const MAX_DELAY_SECONDS: f64 = 2.0;
/// Wow depth (seconds) and feedback tone of the `tape` delay.
const TAPE_WOW_SECONDS: f32 = 0.0015;
const TAPE_TONE: f32 = 0.3;

/// The beats a delay of `beats` plays at `bpm`: halved until it fits the
/// delay line (the editor shows the value used).
pub fn delay_beats_used(beats: f64, bpm: f64) -> f64 {
    let mut used = beats;
    while used > 0.0 && 60.0 / bpm * used > MAX_DELAY_SECONDS {
        used /= 2.0;
    }
    used
}

fn filter_mode(name: &str) -> Option<f32> {
    FILTER_TYPES.iter().position(|filter_type| *filter_type == name).map(|code| code as f32)
}

/// The strip's `filter_slope` for a track's `filterSlope`: 24 from 18 dB/oct
/// up, else 12 (the editor offers only these two).
fn filter_slope(db_per_octave: f64) -> f32 {
    if db_per_octave >= 18.0 { 24.0 } else { 12.0 }
}

/// The strip's `delay_mode` of the slap delay, which plays one echo.
const SLAP_MODE: f32 = 0.0;

/// Whether `s` plays a slap delay (one echo, no feedback).
#[must_use]
pub fn is_slap(s: &StripSettings) -> bool {
    s.delay_mode < 0.5
}

fn delay_mode(name: &str) -> Option<f32> {
    Some(match name {
        "slap" => SLAP_MODE,
        "tape" => 1.0,
        "pingpong" => 2.0,
        _ => return None,
    })
}

/// The strip parameters `applyTrackToStrip()` sets for `track` on a new strip
/// at `bpm`. Unknown filter, reverb and delay types fall back as in JS
/// (lowpass, none, tape). Solo is not a track setting: the caller silences
/// the other tracks.
pub fn strip_settings(track: &Track, bpm: f64) -> StripSettings {
    let mut s = StripSettings::default();

    // Filter: `allpass` skips it (the cutoff opens as in JS).
    if track.filter_type == "allpass" {
        s.cutoff = ordrumbox_dsp::MAX_FILTER_FREQ as f32;
        s.filter_on = 0.0;
    } else {
        s.filter_mode = filter_mode(&track.filter_type).unwrap_or(0.0);
        if track.filter_cutoff_lfo.is_none() {
            s.cutoff = finite_or(track.filter_cutoff, ordrumbox_dsp::MIN_FILTER_FREQ) as f32;
        }
        if track.filter_q_lfo.is_none() {
            s.q = finite_or(track.filter_q, 0.707) as f32;
        }
        s.filter_slope = filter_slope(track.filter_slope);
    }

    // Compressor: its mix is its amount, 0 skips it.
    s.comp_mix = finite_or(track.comp_mix, 0.0).clamp(0.0, 1.0) as f32;
    s.comp_threshold_db = finite_or(track.comp_threshold_db, -18.0) as f32;
    s.comp_ratio = finite_or(track.comp_ratio, 4.0) as f32;
    s.comp_attack_seconds = (finite_or(track.comp_attack_ms, 10.0) / 1000.0) as f32;
    s.comp_release_seconds = (finite_or(track.comp_release_ms, 100.0) / 1000.0) as f32;

    // Sidechain ducking (the engine gives the strip its key).
    s.duck_amount = finite_or(track.duck_amount, 0.0).clamp(0.0, 1.0) as f32;
    s.duck_release_seconds = (finite_or(track.duck_release_ms, 150.0) / 1000.0) as f32;

    // Saturation.
    let amount = if track.saturation_on { finite_or(track.saturation_mix, 0.0).clamp(0.0, 1.0) } else { 0.0 };
    s.saturation_type = match track.saturation_type.as_str() {
        "hard" => 1.0,
        "tape" => 2.0,
        _ => 0.0,
    };
    set_saturation(&mut s, amount);
    // Bitcrusher and hard clipper, in the saturation's panel: its LED counts.
    let in_panel = |amount: f64| if track.saturation_on { finite_or(amount, 0.0).clamp(0.0, 1.0) as f32 } else { 0.0 };
    s.crush = in_panel(track.bitcrush);
    s.clip = in_panel(track.hard_clip);

    // Reverb.
    let amount = if track.reverb_on { finite_or(track.reverb_mix, 0.0).clamp(0.0, 1.0) } else { 0.0 };
    let reverb = reverb_preset(&track.reverb_type).map_or("none", |_| track.reverb_type.as_str());
    if let Some(p) = reverb_preset(reverb) {
        s.reverb_room = p.room;
        s.reverb_damping = p.damping;
        s.reverb_width = p.width;
        s.reverb_predelay = p.predelay;
        s.reverb_dispersion = p.dispersion;
        s.reverb_gate = p.gate;
    }
    if reverb == "none" {
        s.reverb_on = 0.0;
        s.reverb_mix = 0.0;
    } else {
        s.reverb_mix = amount as f32;
    }

    // Delay: 'none' skips it. Without an amount it is still set up, silent,
    // for a plugin host to raise.
    let amount = if track.delay_on { finite_or(track.delay_mix, 0.0).clamp(0.0, 1.0) } else { 0.0 };
    if track.delay_type == "none" {
        s.delay_on = 0.0;
        s.delay_mix = 0.0;
    } else {
        let mode = delay_mode(&track.delay_type).unwrap_or(1.0);
        let beats = if track.delay_beats.is_finite() { track.delay_beats } else { 1.0 };
        let seconds = 60.0 / bpm * delay_beats_used(beats, bpm);
        s.delay_time_l = seconds as f32;
        s.delay_time_r = seconds as f32;
        s.delay_mode = mode;
        s.delay_mix = amount as f32;
        s.delay_feedback = finite_or(track.delay_feedback, 0.4).clamp(0.0, 0.95) as f32;
        if is_slap(&s) {
            // Slap: one echo.
            s.delay_feedback = 0.0;
        } else if mode < 1.5 {
            s.delay_wow = TAPE_WOW_SECONDS;
            s.delay_tone = TAPE_TONE;
        } else {
            s.delay_mono_in = 1.0;
        }
    }

    if track.volume_lfo.is_none() {
        s.volume = track.volume as f32;
    }
    if track.pan_lfo.is_none() {
        s.pan = track.pan as f32;
    }
    if track.mute {
        s.volume = 0.0;
    }
    s
}

/// The saturation `amount` (0 to 1) as the strip's drive, output and mix.
pub fn set_saturation(s: &mut StripSettings, amount: f64) {
    s.saturation_drive = (1.0 + amount * SATURATION_DRIVE_RANGE) as f32;
    s.saturation_output = (1.0 - amount * SATURATION_OUTPUT_DROP) as f32;
    s.saturation_mix = if amount > 0.0 { 1.0 } else { 0.0 };
}

/// The saturation amount (0 to 1) `s` was set to by [`set_saturation`].
#[must_use]
pub fn saturation_amount(s: &StripSettings) -> f64 {
    if s.saturation_mix > 0.0 {
        ((f64::from(s.saturation_drive) - 1.0) / SATURATION_DRIVE_RANGE).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn finite_or(x: f64, fallback: f64) -> f64 {
    if x.is_finite() { x } else { fallback }
}

/// What a track's LFOs set on its strip at each tick of a pass (each list is
/// empty when that LFO is off, else one value per tick).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StripLfo {
    pub track: usize,
    pub volume: Vec<f32>,
    pub pan: Vec<f32>,
    pub cutoff: Vec<f32>,
    pub q: Vec<f32>,
}

impl StripLfo {
    /// Apply the values of `tick` to `settings`.
    pub fn apply(&self, tick: usize, settings: &mut StripSettings) {
        let at = |values: &[f32]| values.get(tick).or(values.last()).copied();
        if let Some(v) = at(&self.volume) {
            settings.volume = v;
        }
        if let Some(v) = at(&self.pan) {
            settings.pan = v;
        }
        if let Some(v) = at(&self.cutoff) {
            settings.cutoff = v;
        }
        if let Some(v) = at(&self.q) {
            settings.q = v;
        }
    }
}

/// `pushStepLfo()` for `ticks` ticks from transport tick `first_tick`; `None`
/// when the track has no strip LFO. A silent track gets no volume LFO (its
/// strip stays at 0).
pub fn strip_lfo(index: usize, track: &Track, silent: bool, first_tick: u64, ticks: u32) -> Option<StripLfo> {
    let values = |lfo: &Option<Object>, map: &dyn Fn(f64) -> f64| -> Vec<f32> {
        lfo.as_ref().map_or_else(Vec::new, |lfo| {
            (0..u64::from(ticks)).map(|t| map(lfo::value(lfo, (first_tick + t) as f64)) as f32).collect()
        })
    };
    let result = StripLfo {
        track: index,
        volume: if silent { Vec::new() } else { values(&track.volume_lfo, &|v| v.clamp(0.0, 2.0)) },
        pan: values(&track.pan_lfo, &|v| (track.pan + v).clamp(-1.0, 1.0)),
        cutoff: values(&track.filter_cutoff_lfo, &|v| {
            (track.filter_cutoff + v).clamp(ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ)
        }),
        q: values(&track.filter_q_lfo, &|v| (track.filter_q + v).clamp(0.707, 18.707)),
    };
    let any = !(result.volume.is_empty() && result.pan.is_empty() && result.cutoff.is_empty() && result.q.is_empty());
    any.then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_a_clean_strip() {
        let s = strip_settings(&Track::default(), 120.0);
        assert_eq!(s.cutoff, 20_000.0);
        assert_eq!((s.saturation_mix, s.reverb_mix, s.delay_mix), (0.0, 0.0, 0.0));
        assert_eq!((s.volume, s.pan), (1.0, 0.0));
    }

    #[test]
    fn delay_follows_the_tempo() {
        let track = Track { delay_type: "pingpong".into(), delay_beats: 0.5, delay_mix: 0.4, ..Track::default() };
        let s = strip_settings(&track, 120.0);
        assert_eq!((s.delay_time_l, s.delay_time_r, s.delay_mode, s.delay_mix), (0.25, 0.25, 2.0, 0.4));
        assert_eq!((s.delay_mono_in, s.delay_feedback), (1.0, 0.4));
    }

    #[test]
    fn a_delay_without_amount_is_set_up_silent_and_none_is_off() {
        let silent = strip_settings(&Track { delay_beats: 0.5, delay_mix: 0.0, ..Track::default() }, 120.0);
        assert_eq!((silent.delay_on, silent.delay_mix, silent.delay_time_l), (1.0, 0.0, 0.25));
        let none = Track { delay_type: "none".into(), reverb_type: "none".into(), delay_mix: 0.5, ..Track::default() };
        let none = strip_settings(&none, 120.0);
        assert_eq!((none.delay_on, none.delay_mix, none.reverb_on, none.reverb_mix), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn the_saturation_amount_reads_back() {
        let mut s = StripSettings::default();
        assert_eq!(saturation_amount(&s), 0.0);
        set_saturation(&mut s, 0.25);
        assert_eq!((s.saturation_drive, s.saturation_mix, saturation_amount(&s)), (2.5, 1.0, 0.25));
    }

    #[test]
    fn the_bitcrush_and_clip_follow_the_saturation_led() {
        let crushed = Track { bitcrush: 0.4, hard_clip: 0.6, ..Track::default() };
        let on = strip_settings(&crushed, 120.0);
        assert_eq!((on.crush, on.clip), (0.4, 0.6));
        let off = strip_settings(&Track { saturation_on: false, ..crushed }, 120.0);
        assert_eq!((off.crush, off.clip), (0.0, 0.0));
    }

    #[test]
    fn the_compressor_takes_the_tracks_settings() {
        let track = Track { comp_mix: 0.7, comp_threshold_db: -30.0, comp_attack_ms: 5.0, ..Track::default() };
        let s = strip_settings(&track, 120.0);
        assert_eq!((s.comp_mix, s.comp_threshold_db, s.comp_ratio), (0.7, -30.0, 4.0));
        assert_eq!((s.comp_attack_seconds, s.comp_release_seconds), (0.005, 0.1));
        assert_eq!(strip_settings(&Track::default(), 120.0).comp_mix, 0.0);
    }

    #[test]
    fn each_delay_type_sounds_its_own() {
        let delay = |kind: &str| {
            let track = Track { delay_type: kind.into(), delay_mix: 0.5, delay_feedback: 0.7, ..Track::default() };
            strip_settings(&track, 120.0)
        };
        let (slap, tape, ping_pong) = (delay("slap"), delay("tape"), delay("pingpong"));
        assert_eq!((slap.delay_feedback, slap.delay_wow), (0.0, 0.0));
        assert_eq!((tape.delay_feedback, tape.delay_wow, tape.delay_tone), (0.7, TAPE_WOW_SECONDS, TAPE_TONE));
        assert_eq!((ping_pong.delay_feedback, ping_pong.delay_mono_in), (0.7, 1.0));
    }

    #[test]
    fn a_delay_too_long_halves_its_beats() {
        assert_eq!(delay_beats_used(4.0, 120.0), 4.0);
        assert_eq!(delay_beats_used(4.0, 90.0), 2.0);
        assert_eq!(delay_beats_used(4.0, 20.0), 0.5);
        let track = Track { delay_beats: 4.0, delay_mix: 0.5, ..Track::default() };
        assert_eq!(strip_settings(&track, 60.0).delay_time_l, 2.0);
    }

    #[test]
    fn each_reverb_type_sounds_its_own() {
        let reverb = |kind: &str| {
            strip_settings(&Track { reverb_type: kind.into(), reverb_mix: 0.5, ..Track::default() }, 120.0)
        };
        assert!(reverb("gated").reverb_gate > 0.0 && reverb("hall").reverb_gate == 0.0);
        assert!(reverb("spring").reverb_dispersion > 0.0 && reverb("plate").reverb_dispersion == 0.0);
        assert!(reverb("hall").reverb_predelay > reverb("plate").reverb_predelay);
        assert_eq!(strip_settings(&Track::default(), 120.0).filter_on, 0.0);
    }

    #[test]
    fn lfo_values_per_tick() {
        let lfo = ordrumbox_core::json::parse(r#"{"freq":1,"min":-0.5,"max":0.5,"type":"sine"}"#).unwrap();
        let track = Track { pan: 0.8, pan_lfo: lfo.as_object().cloned(), ..Track::default() };
        let four_beats = 4 * ordrumbox_core::steps::TICK;
        let l = strip_lfo(2, &track, false, 0, four_beats).unwrap();
        assert_eq!((l.track, l.pan.len(), l.volume.len()), (2, four_beats as usize, 0));
        assert_eq!(l.pan[0], 0.3); // sine starts at its minimum
        assert_eq!(l.pan[four_beats as usize / 2], 1.0); // 0.8 + 0.5, clamped
        assert!(strip_lfo(0, &Track::default(), false, 0, four_beats).is_none());
    }
}
