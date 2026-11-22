//! The strips of a song pass: each instrument's settings and LFOs, silent
//! where its pattern's mute and solo, or the song's automation lanes, say so.
//! A lane that switches inside the pass gates the strip's volume tick by tick
//! (a volume LFO), so a mute cuts the sound at its step, tails included.

use ordrumbox_core::automation::is_silent;
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Pattern, Song, Track};
use ordrumbox_dsp::StripSettings;

use crate::strip_settings::{StripLfo, strip_lfo, strip_settings};

/// The ticks a pass spans, and its tempo.
#[derive(Debug, Clone, Copy)]
pub(super) struct Span {
    pub(super) first_tick: u64,
    pub(super) ticks: u32,
    pub(super) bpm: f64,
}

/// The settings and the LFOs of `strips` (a pattern and a track each) over
/// `span` of `song`.
pub(super) fn settings_and_lfos(
    song: &Song,
    patterns: &[Pattern],
    strips: &[(usize, usize)],
    span: Span,
) -> (Vec<StripSettings>, Vec<StripLfo>) {
    let mut settings = Vec::with_capacity(strips.len());
    let mut lfos = Vec::new();
    for (i, &(p, t)) in strips.iter().enumerate() {
        let tracks = &patterns[p].tracks;
        let silent_at = |tick: u32| {
            let step = song.automation_step_at(span.first_tick + u64::from(tick), TICK);
            is_silent(tracks, t, &song.automations, step)
        };
        let starts_silent = silent_at(0);
        let mut strip = strip_settings(&tracks[t], span.bpm);
        let volume = strip.volume;
        if starts_silent {
            strip.volume = 0.0;
        }
        // Without lanes nothing switches inside a pass.
        let silent: Vec<bool> =
            if song.automations.is_empty() { Vec::new() } else { (0..span.ticks).map(silent_at).collect() };
        let lfo = if silent.iter().any(|&s| s != starts_silent) {
            Some(gated_lfo(i, &tracks[t], volume, &silent, span))
        } else {
            strip_lfo(i, &tracks[t], starts_silent, span.first_tick, span.ticks)
        };
        settings.push(strip);
        lfos.extend(lfo);
    }
    (settings, lfos)
}

/// Strip `index`'s LFOs with its volume 0 on the ticks `silent` marks, and
/// elsewhere `volume` (or its volume LFO's value).
fn gated_lfo(index: usize, track: &Track, volume: f32, silent: &[bool], span: Span) -> StripLfo {
    let mut lfo = strip_lfo(index, track, false, span.first_tick, span.ticks)
        .unwrap_or(StripLfo { track: index, ..StripLfo::default() });
    let lfo_volume = std::mem::take(&mut lfo.volume);
    lfo.volume = silent
        .iter()
        .enumerate()
        .map(|(tick, &is_silent)| if is_silent { 0.0 } else { lfo_volume.get(tick).copied().unwrap_or(volume) })
        .collect();
    lfo
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;
    use ordrumbox_core::automation::{AutomationLane, AutomationTarget};

    #[test]
    fn a_mute_lane_cuts_its_strip_from_its_step() {
        let (mut library, _) = Library::parse(
            r#"{"patterns":[{"id":"a","name":"a","tracks":[{"name":"KICK"},{"name":"HAT"}]}],
                "songs":[{"name":"s","clips":[{"pattern":"a","startMeasure":0,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let mut lane = AutomationLane::new("KICK", AutomationTarget::Mute);
        lane.toggle_at(8); // the middle of the first measure
        library.songs[0].automations.push(lane);
        let (song, patterns) = (&library.songs[0], &library.patterns);
        let strips = [(0, 0), (0, 1)];
        let ticks = TICK * 4;
        let span = |measure: u64| Span { first_tick: measure * u64::from(ticks), ticks, bpm: 120.0 };

        let (settings, lfos) = settings_and_lfos(song, patterns, &strips, span(0));
        assert_eq!((settings[0].volume, settings[1].volume), (1.0, 1.0));
        assert_eq!(lfos.len(), 1, "only the strip that switches");
        let volume = &lfos[0].volume;
        assert_eq!((lfos[0].track, volume.len()), (0, ticks as usize));
        let half = (ticks / 2) as usize;
        assert!(volume[..half].iter().all(|&v| v == 1.0) && volume[half..].iter().all(|&v| v == 0.0));

        // Muted for the whole second measure: silent, as a muted track is.
        let (settings, lfos) = settings_and_lfos(song, patterns, &strips, span(1));
        assert_eq!((settings[0].volume, settings[1].volume), (0.0, 1.0));
        assert!(lfos.is_empty());
    }
}
