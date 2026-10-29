//! A track's settings as strip parameters (`strip_sync.js` and `strip.js`),
//! and its LFOs as per-tick parameter values (`step_lfo.js`).

use ordrumbox_core::Track;
use ordrumbox_core::json::Object;
use ordrumbox_core::lfo;
use ordrumbox_dsp::StripSettings;

/// `REVERB_PRESETS`: room, damp, width.
fn reverb_preset(name: &str) -> Option<(f32, f32, f32)> {
    Some(match name {
        "none" => (0.0, 0.5, 0.0),
        "room" => (0.5, 0.5, 0.8),
        "hall" => (0.85, 0.3, 1.0),
        "plate" => (0.7, 0.4, 0.9),
        "spring" => (0.45, 0.6, 0.5),
        "gated" => (0.4, 0.7, 0.4),
        _ => return None,
    })
}

fn filter_mode(name: &str) -> Option<f32> {
    Some(match name {
        "lowpass" => 0.0,
        "highpass" => 1.0,
        "bandpass" => 2.0,
        "notch" => 3.0,
        _ => return None,
    })
}

fn delay_mode(name: &str) -> Option<f32> {
    Some(match name {
        "slap" => 0.0,
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

    // Filter: `allpass` only opens the cutoff (the mode stays lowpass).
    if track.filter_type == "allpass" {
        s.cutoff = 20_000.0;
    } else {
        s.filter_mode = filter_mode(&track.filter_type).unwrap_or(0.0);
        if track.filter_cutoff_lfo.is_none() {
            s.cutoff = finite_or(track.filter_cutoff, 20.0) as f32;
        }
        if track.filter_q_lfo.is_none() {
            s.q = finite_or(track.filter_q, 0.707) as f32;
        }
    }

    // Saturation.
    let amount = if track.saturation_on { finite_or(track.saturation_mix, 0.0).clamp(0.0, 1.0) } else { 0.0 };
    s.saturation_type = match track.saturation_type.as_str() {
        "hard" => 1.0,
        "tape" => 2.0,
        _ => 0.0,
    };
    s.saturation_drive = (1.0 + amount * 6.0) as f32;
    s.saturation_output = (1.0 - amount * 0.15) as f32;
    s.saturation_mix = if amount > 0.0 { 1.0 } else { 0.0 };

    // Reverb.
    let amount = if track.reverb_on { finite_or(track.reverb_mix, 0.0).clamp(0.0, 1.0) } else { 0.0 };
    let reverb = reverb_preset(&track.reverb_type).map_or("none", |_| track.reverb_type.as_str());
    let (room, damp, width) = reverb_preset(reverb).unwrap_or((0.0, 0.5, 0.0));
    s.reverb_room = room;
    s.reverb_damping = damp;
    s.reverb_width = width;
    s.reverb_mix = if reverb == "none" { 0.0 } else { amount as f32 };

    // Delay: 'none' or no amount only silences the send.
    let amount = if track.delay_on { finite_or(track.delay_mix, 0.0).clamp(0.0, 1.0) } else { 0.0 };
    if track.delay_type == "none" || amount <= 0.0 {
        s.delay_mix = 0.0;
    } else {
        let mode = delay_mode(&track.delay_type).unwrap_or(1.0);
        let beats = if track.delay_beats.is_finite() { track.delay_beats } else { 1.0 };
        let seconds = 60.0 / bpm * beats;
        s.delay_time_l = (if mode >= 1.5 { seconds * 0.667 } else { seconds }) as f32;
        s.delay_time_r = seconds as f32;
        s.delay_mode = mode;
        s.delay_mix = amount as f32;
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
        cutoff: values(&track.filter_cutoff_lfo, &|v| (track.filter_cutoff + v).clamp(20.0, 20_000.0)),
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
        assert_eq!(
            (s.delay_time_l, s.delay_time_r, s.delay_mode, s.delay_mix),
            ((0.25_f64 * 0.667) as f32, 0.25, 2.0, 0.4)
        );
    }

    #[test]
    fn lfo_values_per_tick() {
        let lfo = ordrumbox_core::json::parse(r#"{"freq":1,"min":-0.5,"max":0.5,"type":"sine"}"#).unwrap();
        let track = Track { pan: 0.8, pan_lfo: lfo.as_object().cloned(), ..Track::default() };
        let l = strip_lfo(2, &track, false, 0, 128).unwrap();
        assert_eq!((l.track, l.pan.len(), l.volume.len()), (2, 128, 0));
        assert_eq!(l.pan[0], 0.3); // sine starts at its minimum
        assert_eq!(l.pan[64], 1.0); // 0.8 + 0.5, clamped
        assert!(strip_lfo(0, &Track::default(), false, 0, 128).is_none());
    }
}
