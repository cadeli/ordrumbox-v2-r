//! The metronome: a short click on every beat of a pass, the first of each
//! measure higher. It is built into the pass on the UI side, on a strip of its
//! own after the tracks' (a synth voice, so no sample is needed), and the audio
//! thread plays it like any note.

use ordrumbox_core::steps::TICK;
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_dsp::synth::Adsr;
use ordrumbox_dsp::{StripSettings, SynthPatch};

use crate::pass::{Event, Pass, Sound, SynthStart};

/// Beats in a measure, for the accent.
const BEATS_PER_MEASURE: u64 = 4;
/// Pitch of the click, Hz: on the first beat of a measure, and on the others.
const ACCENT_HZ: f64 = 1760.0;
const BEAT_HZ: f64 = 1175.0;
/// How long the click lasts, seconds.
const CLICK_SECONDS: f64 = 0.03;
/// The click's level, 0–1.
const CLICK_LEVEL: f64 = 0.6;

fn click_patch(hz: f64) -> SynthPatch {
    SynthPatch {
        osc_freq: [hz; 3],
        osc_gain: [1.0, 0.0, 0.0],
        envelope: Adsr { attack: 0.0005, decay: CLICK_SECONDS, sustain: 0.0, release: 0.01 },
        volume: CLICK_LEVEL,
        velocity: CLICK_LEVEL,
        ..SynthPatch::default()
    }
}

/// Add a click on every beat of `pass`. Nothing is added when every strip is
/// taken by a track.
pub fn add_clicks(pass: &mut Pass) {
    let strip = pass.strips.len();
    if strip >= MAX_TRACKS || pass.ticks == 0 {
        return;
    }
    pass.strips.push(StripSettings::default());
    let patch_base = pass.patches.len();
    pass.patches.extend([click_patch(ACCENT_HZ), click_patch(BEAT_HZ)]);
    let tick = u64::from(TICK);
    let beats = (u64::from(pass.ticks) / tick).max(1);
    let first_beat = pass.first_tick / tick;
    for beat in 0..beats {
        let accent = (first_beat + beat).is_multiple_of(BEATS_PER_MEASURE);
        let frame = u128::from(beat * tick) * u128::from(pass.frames) / u128::from(pass.ticks);
        pass.events.push(Event {
            frame: u64::try_from(frame).unwrap_or(u64::MAX),
            sound: Sound::Synth(SynthStart {
                track: strip,
                patch: patch_base + usize::from(!accent),
                length: CLICK_SECONDS,
            }),
            mono: false,
            velocity: CLICK_LEVEL as f32,
            note: None,
        });
    }
    // Stable: a note and a click on the same frame keep their order.
    pass.events.sort_by_key(|event| event.frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_falls_on_every_beat_and_the_first_of_a_measure_is_higher() {
        let mut pass = Pass { ticks: TICK * 8, frames: 8000, first_tick: u64::from(TICK) * 4, ..Pass::default() };
        add_clicks(&mut pass);
        assert_eq!(pass.strips.len(), 1);
        let frames: Vec<u64> = pass.events.iter().map(|e| e.frame).collect();
        assert_eq!(frames, [0, 1000, 2000, 3000, 4000, 5000, 6000, 7000]);
        let patches: Vec<usize> = pass
            .events
            .iter()
            .map(|e| match e.sound {
                Sound::Synth(s) => s.patch,
                Sound::Sample(_) => usize::MAX,
            })
            .collect();
        // The pass starts on beat 4: a measure starts there and 4 beats later.
        assert_eq!(patches, [0, 1, 1, 1, 0, 1, 1, 1]);
    }

    #[test]
    fn no_click_without_a_free_strip() {
        let mut pass =
            Pass { ticks: TICK, frames: 100, strips: vec![StripSettings::default(); MAX_TRACKS], ..Pass::default() };
        add_clicks(&mut pass);
        assert!(pass.events.is_empty());
    }
}
