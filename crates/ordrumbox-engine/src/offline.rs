//! Offline renders: a pattern or a song
//! through the same engine as playback, as fast as it computes, at 44.1 kHz.
//! The length is the pattern's loops or the song's measures, without a tail.

use std::borrow::Cow;
use std::sync::Arc;

use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Library, Rng, Scales, song::BEATS_PER_MEASURE, song::clamp_bpm};

use crate::controller::{Controller, Mode};
use crate::engine::HostMix;
use crate::kit::SampleBank;
use crate::synth_preset::SynthPresets;
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_dsp::ReturnSettings;

/// Sample rate of the exports.
pub const EXPORT_RATE: u32 = 44_100;
/// Frames rendered per engine call (the callback size of playback).
const CHUNK: usize = 512;

/// What a render needs besides the library.
#[derive(Debug, Clone)]
pub struct RenderSetup {
    pub bank: Arc<SampleBank>,
    pub scales: Scales,
    pub synths: SynthPresets,
    /// Kit the tracks get their samples from.
    pub kit: String,
    pub sample_rate: u32,
    /// Seed of the random choices (sound assignment, probabilities, variations).
    pub seed: u64,
    /// The shared echo and reverb the tracks send to.
    pub returns: ReturnSettings,
}

/// Frames of `ticks` ticks at `bpm`: `floor(rate × duration)` for a pattern,
/// `ceil` for a song.
fn frames(ticks: f64, bpm: f64, rate: u32, round_up: bool) -> usize {
    let seconds = ticks * 60.0 / (bpm * f64::from(TICK));
    let frames = f64::from(rate) * seconds;
    (if round_up { frames.ceil() } else { frames.floor() }) as usize
}

/// Called before each pass of a pattern render is built, with its number:
/// where the tracks in auto mode get their new lines. It
/// changes the render's own copy of the library, never the caller's.
pub type BeforePass<'a> = &'a mut dyn FnMut(&mut Library, usize, u64);

/// A stereo signal, left then right.
pub type Stereo = (Vec<f32>, Vec<f32>);

/// What a render gives back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Output {
    /// The mix, through the master bus.
    Mix,
    /// Each track apart, as its strip leaves it: before the master bus.
    Stems { track_count: usize },
}

fn render(
    library: &Library,
    mode: Mode,
    length: usize,
    output: Output,
    setup: &RenderSetup,
    mut before_pass: Option<BeforePass<'_>>,
) -> (Stereo, Vec<Stereo>) {
    let mut library = Cow::Borrowed(library);
    let mut loop_start = |library: &mut Cow<'_, Library>, number: u64| {
        if let (Some(hook), Mode::Pattern(p)) = (before_pass.as_mut(), mode) {
            hook(library.to_mut(), p, number);
        }
    };
    let (mut controller, mut audio) = Controller::with_rng(
        setup.sample_rate as f32,
        setup.bank.clone(),
        setup.scales.clone(),
        setup.synths.clone(),
        Rng::new(setup.seed),
    );
    controller.set_kit(&setup.kit);
    controller.set_returns(setup.returns);
    let track_count = match output {
        Output::Mix => 0,
        Output::Stems { track_count } => track_count.min(MAX_TRACKS),
    };
    // Every track split from the mix: each is given to `aux`.
    audio.set_host_mix(HostMix { split: if track_count == 0 { 0 } else { u64::MAX }, ..HostMix::default() });
    loop_start(&mut library, 0);
    controller.play(&library, mode);
    let (mut left, mut right) = (vec![0.0; length], vec![0.0; length]);
    let mut stems: Vec<Stereo> = (0..track_count).map(|_| (vec![0.0; length], vec![0.0; length])).collect();
    for from in (0..length).step_by(CHUNK) {
        let to = (from + CHUNK).min(length);
        if let Some(number) = controller.pending_pass() {
            loop_start(&mut library, number);
        }
        controller.update(&library);
        audio.render_split(&mut left[from..to], &mut right[from..to], |track, at, l, r| {
            if let Some((stem_left, stem_right)) = stems.get_mut(track) {
                let start = from + at;
                stem_left[start..start + l.len()].copy_from_slice(l);
                stem_right[start..start + r.len()].copy_from_slice(r);
            }
        });
    }
    ((left, right), stems)
}

/// Frames a pattern played `loops` times lasts.
fn pattern_length(library: &Library, index: usize, loops: u32, rate: u32) -> Option<usize> {
    let pattern = library.patterns.get(index)?;
    let ticks = f64::from(pattern.beat_count.max(1)) * f64::from(TICK) * f64::from(loops.max(1));
    Some(frames(ticks, clamp_bpm(library.bpm), rate, false))
}

/// Frames song `index` lasts, `None` without a playable clip.
fn song_length(library: &Library, index: usize, rate: u32) -> Option<usize> {
    let song = library.songs.get(index)?;
    if !song.clips.iter().any(|c| library.patterns.iter().any(|p| p.id == c.pattern_id)) {
        return None; // no playable clip
    }
    let measures = song.length_measures().max(1.0);
    let ticks = (measures * f64::from(BEATS_PER_MEASURE) * f64::from(TICK)).ceil();
    Some(frames(ticks, clamp_bpm(song.bpm), rate, true))
}

/// Pattern `index` played `loops` times; `before_pass`
/// regenerates its auto-mode tracks loop after loop.
pub fn render_pattern(
    library: &Library,
    index: usize,
    loops: u32,
    setup: &RenderSetup,
    before_pass: Option<BeforePass<'_>>,
) -> Option<Stereo> {
    let length = pattern_length(library, index, loops, setup.sample_rate)?;
    Some(render(library, Mode::Pattern(index), length, Output::Mix, setup, before_pass).0)
}

/// Song `index`, its whole song once.
pub fn render_song(library: &Library, index: usize, setup: &RenderSetup) -> Option<Stereo> {
    let length = song_length(library, index, setup.sample_rate)?;
    Some(render(library, Mode::Song(index), length, Output::Mix, setup, None).0)
}

/// Pattern `index` played once, each of its tracks apart (see [`Output::Stems`]),
/// in the order of the tracks, all as long as the pattern; `before_pass` as in
/// [`render_pattern`].
pub fn render_pattern_stems(
    library: &Library,
    index: usize,
    setup: &RenderSetup,
    before_pass: Option<BeforePass<'_>>,
) -> Option<Vec<Stereo>> {
    let length = pattern_length(library, index, 1, setup.sample_rate)?;
    let track_count = library.patterns.get(index)?.tracks.len();
    Some(render(library, Mode::Pattern(index), length, Output::Stems { track_count }, setup, before_pass).1)
}

/// Song `index`, its whole song once, each track apart: as many as the
/// most tracks of a pattern it plays, a track being the one of that position
/// in each pattern.
pub fn render_song_stems(library: &Library, index: usize, setup: &RenderSetup) -> Option<Vec<Stereo>> {
    let length = song_length(library, index, setup.sample_rate)?;
    let song = library.songs.get(index)?;
    let track_count = song
        .clips
        .iter()
        .filter_map(|c| library.patterns.iter().find(|p| p.id == c.pattern_id))
        .map(|p| p.tracks.len())
        .max()?;
    Some(render(library, Mode::Song(index), length, Output::Stems { track_count }, setup, None).1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::{BankSample, KitSample};
    use ordrumbox_dsp::SampleBuffer;

    fn setup() -> RenderSetup {
        let instrument = KitSample {
            key: Some("KICK".into()),
            display_name: "k".into(),
            path: "k".into(),
            decay_ms: Some(200.0),
            ..Default::default()
        };
        let buffer = SampleBuffer::mono(vec![0.5; 2000], 44_100);
        RenderSetup {
            bank: Arc::new(SampleBank {
                samples: vec![BankSample { kit: "x".into(), sample: instrument, buffer: buffer.into() }],
            }),
            scales: Scales::default(),
            synths: SynthPresets::default(),
            kit: "x".into(),
            sample_rate: EXPORT_RATE,
            seed: 7,
            returns: Default::default(),
        }
    }

    #[test]
    fn lengths_and_repeatability() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"id":"p","name":"p","bpm":100,"beatCount":3,"tracks":[{"name":"KICK","notes":[{},{"beat":1}]}]}],
                "songs":[{"id":"s","name":"s","bpm":90,"clips":[{"pattern":"p","startMeasure":1,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let setup = setup();
        let (left, _) = render_pattern(&library, 0, 2, &setup, None).unwrap();
        // 3 beats × 2 loops at 100 bpm = 3.6 s.
        assert_eq!(left.len(), 158_760);
        assert!(left.iter().any(|s| s.abs() > 0.01));
        assert_eq!(render_pattern(&library, 0, 2, &setup, None).unwrap().0, left, "same seed, same render");
        // The hook runs before each pass, on a copy.
        let mut passes = Vec::new();
        let mut hook = |library: &mut Library, p: usize, number: u64| {
            library.patterns[p].tracks[0].notes.clear();
            passes.push(number);
        };
        let (silent, _) = render_pattern(&library, 0, 2, &setup, Some(&mut hook)).unwrap();
        assert!(silent.iter().all(|s| s.abs() < 1e-6));
        // (the engine asks for the third pass ahead, before the render ends)
        assert!(passes.starts_with(&[0, 1, 2]), "{passes:?}");
        assert_eq!(library.patterns[0].tracks[0].notes.len(), 2);
        // 3 measures at 90 bpm = 8 s; the first measure is silent.
        let (song, _) = render_song(&library, 0, &setup).unwrap();
        assert_eq!(song.len(), 352_800);
        assert!(song[..117_000].iter().all(|s| s.abs() < 1e-6));
        assert!(song[117_600..].iter().any(|s| s.abs() > 0.01));
        assert!(render_song(&library, 3, &setup).is_none());
    }

    #[test]
    fn stems_give_each_track_apart_as_long_as_the_render() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"id":"p","name":"p","bpm":100,"beatCount":3,"tracks":[
                {"name":"KICK","notes":[{}]},{"name":"SNARE","notes":[]}]}],
                "songs":[{"id":"s","name":"s","bpm":90,"clips":[{"pattern":"p","startMeasure":1,"measureCount":1}]}]}"#,
        )
        .unwrap();
        let setup = setup();
        let stems = render_pattern_stems(&library, 0, &setup, None).unwrap();
        let (mix, _) = render_pattern(&library, 0, 1, &setup, None).unwrap();
        assert_eq!(stems.len(), 2);
        assert!(stems.iter().all(|(l, r)| l.len() == mix.len() && r.len() == mix.len()));
        assert!(stems[0].0.iter().any(|s| s.abs() > 0.01), "the kick is heard");
        assert!(stems[1].0.iter().all(|s| s.abs() < 1e-6), "the snare has no note");
        let song = render_song_stems(&library, 0, &setup).unwrap();
        assert_eq!(song.len(), 2);
        assert!(song[0].0[..44_000].iter().all(|s| s.abs() < 1e-6), "the first measure is silent");
        assert!(song[0].0.iter().any(|s| s.abs() > 0.01));
        assert!(render_pattern_stems(&library, 5, &setup, None).is_none());
    }
}
