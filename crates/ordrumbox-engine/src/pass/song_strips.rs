//! The strips of a song pass: each instrument's settings and LFOs, its
//! volume, pan and filter set by the song's automation lanes (silent where
//! its pattern's mute and solo, or the lanes, say so; at a volume lane's
//! level, in decibels). A value that changes inside the pass is set tick by
//! tick (as its LFO), so a mute cuts the sound at its step, tails included,
//! and a drawn level glides.

use ordrumbox_core::automation::{AutomationTarget, automated_at, is_silent, volume_gain};
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Pattern, Song};
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
        let track = &tracks[t];
        // The volume the song sets at `tick`: 0 where silent, a lane's level,
        // `None` where it leaves the track's own.
        let volume_at = |tick: u32| {
            let position = song.automation_position_at(span.first_tick + u64::from(tick), TICK);
            if is_silent(tracks, t, &song.automations, position.floor() as u32) {
                Some(0.0)
            } else {
                let level = automated_at(&song.automations, &track.name, AutomationTarget::Volume, position);
                level.map(|level| volume_gain(level) as f32)
            }
        };
        // Without lanes nothing changes inside a pass: the pattern's mute and solo hold.
        let volumes: Vec<Option<f32>> =
            if song.automations.is_empty() { vec![volume_at(0)] } else { (0..span.ticks).map(volume_at).collect() };
        let mut strip = strip_settings(track, span.bpm);
        // The track's mute is `volume_at`'s: a lane can switch it off.
        if track.mute && track.volume_lfo.is_none() {
            strip.volume = track.volume as f32;
        }
        let mut lfo = strip_lfo(i, track, false, span.first_tick, span.ticks)
            .unwrap_or(StripLfo { track: i, ..StripLfo::default() });
        let cutoffs = lane_values(song, &track.name, AutomationTarget::Cutoff, span);
        // A cutoff lane filters a track whose filter is off, as a note's lock does.
        if cutoffs.iter().any(Option::is_some) {
            strip.filter_on = 1.0;
        }
        set_per_tick(&mut strip.volume, &mut lfo.volume, &volumes);
        let pans = lane_values(song, &track.name, AutomationTarget::Pan, span);
        set_per_tick(&mut strip.pan, &mut lfo.pan, &pans);
        set_per_tick(&mut strip.cutoff, &mut lfo.cutoff, &cutoffs);
        let qs = lane_values(song, &track.name, AutomationTarget::Q, span);
        set_per_tick(&mut strip.q, &mut lfo.q, &qs);
        settings.push(strip);
        lfos.extend((!lfo.is_empty()).then_some(lfo));
    }
    (settings, lfos)
}

/// What the lanes of `song` set `target` of `instrument` to at each tick of
/// `span`, in the control's unit; empty when no lane is for it.
fn lane_values(song: &Song, instrument: &str, target: AutomationTarget, span: Span) -> Vec<Option<f32>> {
    if !song.automations.iter().any(|lane| lane.is_for(instrument, target)) {
        return Vec::new();
    }
    let value_at = |tick: u32| {
        let position = song.automation_position_at(span.first_tick + u64::from(tick), TICK);
        automated_at(&song.automations, instrument, target, position).map(|value| target.to_control(value) as f32)
    };
    (0..span.ticks).map(value_at).collect()
}

/// A strip's value and its LFO's ticks with `levels` (one per tick, or one
/// for the whole pass) set over them: one value for the whole pass sets the
/// strip and drops its LFO; values that change are set tick by tick, the
/// LFO's own (or the strip's) where they set nothing.
fn set_per_tick(value: &mut f32, ticks: &mut Vec<f32>, levels: &[Option<f32>]) {
    let Some(&first) = levels.first() else { return };
    if levels.iter().all(|&level| level == first) {
        if let Some(level) = first {
            *value = level;
            ticks.clear();
        }
        return;
    }
    let own = std::mem::take(ticks);
    *ticks = levels
        .iter()
        .enumerate()
        .map(|(tick, level)| level.unwrap_or_else(|| own.get(tick).copied().unwrap_or(*value)))
        .collect();
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

    #[test]
    fn pan_cutoff_and_q_lanes_set_their_strip() {
        let (mut library, _) = Library::parse(
            r#"{"patterns":[{"id":"a","name":"a","tracks":[{"name":"KICK","pan":0.2},{"name":"HAT","filterType":"highpass","filterQ":2}]}],
                "songs":[{"name":"s","clips":[{"pattern":"a","startMeasure":0,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let lanes = [
            (AutomationTarget::Pan, "KICK", 8, AutomationStroke::Fill(Some(0.0))),
            (AutomationTarget::Cutoff, "KICK", 0, AutomationStroke::Fill(Some(0.5))),
            (AutomationTarget::Q, "HAT", 0, AutomationStroke::Line((0, 0.0), (16, 1.0))),
        ];
        for (target, instrument, from, stroke) in lanes {
            let mut lane = AutomationLane::new(instrument, target);
            lane.paint(from, u32::MAX, stroke);
            library.songs[0].automations.push(lane);
        }
        let (song, patterns) = (&library.songs[0], &library.patterns);
        let ticks = TICK * 4;
        let span = |measure: u64| Span { first_tick: measure * u64::from(ticks), ticks, bpm: 120.0 };

        let (settings, lfos) = settings_and_lfos(song, patterns, &[(0, 0), (0, 1)], span(0));
        let cutoff = AutomationTarget::Cutoff.to_control(0.5) as f32;
        assert_eq!((settings[0].filter_on, settings[0].cutoff), (1.0, cutoff), "its filter switched on by its lane");
        assert!(lfos[0].cutoff.is_empty() && lfos[0].volume.is_empty() && lfos[0].q.is_empty());
        let half = (ticks / 2) as usize;
        let pan = &lfos[0].pan;
        assert!(pan[..half].iter().all(|&v| v == 0.2) && pan[half..].iter().all(|&v| v == -1.0), "its own, then left");
        let q = &lfos[1].q;
        assert_eq!((q.len(), q[0]), (ticks as usize, 0.707));
        assert!(q.windows(2).all(|pair| pair[0] <= pair[1]) && q[ticks as usize - 1] > 17.0, "a line, rising");

        let (settings, lfos) = settings_and_lfos(song, patterns, &[(0, 0), (0, 1)], span(1));
        assert_eq!((settings[0].pan, settings[1].q), (-1.0, 18.707), "held for the whole pass");
        assert!(lfos.is_empty());
    }
}
