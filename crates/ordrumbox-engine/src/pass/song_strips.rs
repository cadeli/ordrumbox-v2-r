//! The strips of a song pass: each instrument's settings and LFOs, its
//! volume set by the song's automation lanes (silent where its pattern's
//! mute and solo, or the lanes, say so; at a volume lane's level, in
//! decibels). A level that changes inside the pass is set tick by tick (a
//! volume LFO), so a mute cuts the sound at its step, tails included, and a
//! drawn volume glides.

use ordrumbox_core::automation::{AutomationTarget, automated_at, is_silent, volume_gain};
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
        // The volume the song sets at `tick`: 0 where silent, a lane's level,
        // `None` where it leaves the track's own.
        let level_at = |tick: u32| {
            let position = song.automation_position_at(span.first_tick + u64::from(tick), TICK);
            if is_silent(tracks, t, &song.automations, position.floor() as u32) {
                Some(0.0)
            } else {
                let level = automated_at(&song.automations, &tracks[t].name, AutomationTarget::Volume, position);
                level.map(|level| volume_gain(level) as f32)
            }
        };
        let first = level_at(0);
        let mut strip = strip_settings(&tracks[t], span.bpm);
        // The track's mute is `level_at`'s: a lane can switch it off.
        if tracks[t].mute && tracks[t].volume_lfo.is_none() {
            strip.volume = tracks[t].volume as f32;
        }
        let volume = strip.volume;
        if let Some(level) = first {
            strip.volume = level;
        }
        // Without lanes nothing changes inside a pass.
        let levels: Vec<Option<f32>> =
            if song.automations.is_empty() { Vec::new() } else { (0..span.ticks).map(level_at).collect() };
        let lfo = if levels.iter().any(|&level| level != first) {
            Some(leveled_lfo(i, &tracks[t], volume, &levels, span))
        } else {
            strip_lfo(i, &tracks[t], first.is_some(), span.first_tick, span.ticks)
        };
        settings.push(strip);
        lfos.extend(lfo);
    }
    (settings, lfos)
}

/// Strip `index`'s LFOs with its volume at `levels` on the ticks they set,
/// and elsewhere `volume` (or its volume LFO's value).
fn leveled_lfo(index: usize, track: &Track, volume: f32, levels: &[Option<f32>], span: Span) -> StripLfo {
    let mut lfo = strip_lfo(index, track, false, span.first_tick, span.ticks)
        .unwrap_or(StripLfo { track: index, ..StripLfo::default() });
    let lfo_volume = std::mem::take(&mut lfo.volume);
    lfo.volume = levels
        .iter()
        .enumerate()
        .map(|(tick, level)| level.unwrap_or_else(|| lfo_volume.get(tick).copied().unwrap_or(volume)))
        .collect();
    lfo
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;
    use ordrumbox_core::automation::{AutomationLane, AutomationStroke, AutomationTarget};

    #[test]
    fn a_mute_lane_cuts_its_strip_from_its_step() {
        let (mut library, _) = Library::parse(
            r#"{"patterns":[{"id":"a","name":"a","tracks":[{"name":"KICK"},{"name":"HAT"}]}],
                "songs":[{"name":"s","clips":[{"pattern":"a","startMeasure":0,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let mut lane = AutomationLane::new("KICK", AutomationTarget::Mute);
        lane.paint(8, u32::MAX, AutomationStroke::Fill(Some(1.0))); // from the middle of the first measure on
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

    #[test]
    fn a_volume_lane_sets_its_strip_s_level() {
        let (mut library, _) = Library::parse(
            r#"{"patterns":[{"id":"a","name":"a","tracks":[{"name":"KICK","volume":0.8},{"name":"SNARE","volume":0.6,"mute":true}]}],
                "songs":[{"name":"s","clips":[{"pattern":"a","startMeasure":0,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let mut lane = AutomationLane::new("KICK", AutomationTarget::Volume);
        lane.paint(4, 7, AutomationStroke::Fill(Some(0.75))); // the second beat
        lane.paint(16, u32::MAX, AutomationStroke::Fill(Some(1.0)));
        lane.paint(10, 12, AutomationStroke::Line((10, 0.5), (12, 1.0))); // a line on the third beat
        library.songs[0].automations.push(lane);
        let mut unmute = AutomationLane::new("SNARE", AutomationTarget::Mute);
        unmute.paint(0, u32::MAX, AutomationStroke::Fill(Some(0.0)));
        library.songs[0].automations.push(unmute);
        let (song, patterns) = (&library.songs[0], &library.patterns);
        let ticks = TICK * 4;
        let span = |measure: u64| Span { first_tick: measure * u64::from(ticks), ticks, bpm: 120.0 };

        let (settings, lfos) = settings_and_lfos(song, patterns, &[(0, 0), (0, 1)], span(0));
        assert_eq!(settings[0].volume, 0.8, "the track's own at first");
        assert_eq!(settings[1].volume, 0.6, "muted in its pattern, switched back on by its lane");
        let volume = &lfos[0].volume;
        let (beat, step) = ((ticks / 4) as usize, (ticks / 16) as usize);
        let at = |level: f64| volume_gain(level) as f32;
        assert_eq!([volume[0], volume[beat], volume[2 * beat]], [0.8, at(0.75), 0.8], "the lane's, in dB, on its beat");
        let line = &volume[10 * step..12 * step];
        assert_eq!((line[0], line[step]), (at(0.5), at(0.75)), "each step of the line");
        assert!(line.windows(2).all(|pair| pair[0] < pair[1]), "gliding from tick to tick");

        let (settings, lfos) = settings_and_lfos(song, patterns, &[(0, 0)], span(1));
        assert_eq!(settings[0].volume, 1.0, "the whole pass at the lane's level");
        assert!(lfos.is_empty());
    }
}
