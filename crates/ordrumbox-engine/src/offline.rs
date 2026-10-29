//! Offline renders (`audio/export/wav_exporter.js`): a pattern or a song
//! through the same engine as playback, as fast as it computes, at 44.1 kHz.
//! The length is the JS one: the pattern's loops or the song's measures,
//! without a tail.

use std::borrow::Cow;
use std::sync::Arc;

use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Library, Rng, Scales, song::BEATS_PER_MEASURE};

use crate::controller::{Controller, Mode};
use crate::kit::SampleBank;
use crate::synth_preset::SynthPresets;

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
}

/// Frames of `ticks` ticks at `bpm`: `floor(rate × duration)` for a pattern,
/// `ceil` for a song, as the JS sizes its `OfflineAudioContext`.
fn frames(ticks: f64, bpm: f64, rate: u32, round_up: bool) -> usize {
    let seconds = ticks * 60.0 / (bpm * f64::from(TICK));
    let frames = f64::from(rate) * seconds;
    (if round_up { frames.ceil() } else { frames.floor() }) as usize
}

/// Called before each pass of a pattern render is built, with its number:
/// where the tracks in auto mode get their new lines (`#handleLoopStart`). It
/// changes the render's own copy of the library, never the caller's.
pub type BeforePass<'a> = &'a mut dyn FnMut(&mut Library, usize, u64);

fn render(
    library: &Library,
    mode: Mode,
    length: usize,
    setup: &RenderSetup,
    mut before_pass: Option<BeforePass<'_>>,
) -> (Vec<f32>, Vec<f32>) {
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
    loop_start(&mut library, 0);
    controller.play(&library, mode);
    let (mut left, mut right) = (vec![0.0; length], vec![0.0; length]);
    for from in (0..length).step_by(CHUNK) {
        let to = (from + CHUNK).min(length);
        if let Some(number) = controller.pending_pass() {
            loop_start(&mut library, number);
        }
        controller.update(&library);
        audio.render(&mut left[from..to], &mut right[from..to]);
    }
    (left, right)
}

/// Pattern `index` played `loops` times (`exportPatternToWav`); `before_pass`
/// regenerates its auto-mode tracks loop after loop.
pub fn render_pattern(
    library: &Library,
    index: usize,
    loops: u32,
    setup: &RenderSetup,
    before_pass: Option<BeforePass<'_>>,
) -> Option<(Vec<f32>, Vec<f32>)> {
    let pattern = library.patterns.get(index)?;
    let ticks = f64::from(pattern.beat_count.max(1)) * f64::from(TICK) * f64::from(loops.max(1));
    let length = frames(ticks, pattern.bpm.clamp(20.0, 300.0), setup.sample_rate, false);
    Some(render(library, Mode::Pattern(index), length, setup, before_pass))
}

/// Song `index`, its whole arrangement once (`exportSongToWav`).
pub fn render_song(library: &Library, index: usize, setup: &RenderSetup) -> Option<(Vec<f32>, Vec<f32>)> {
    let song = library.songs.get(index)?;
    if !song.clips.iter().any(|c| library.patterns.iter().any(|p| p.id == c.pattern)) {
        return None; // no playable clip
    }
    let measures = song.length_measures().max(1.0);
    let ticks = (measures * BEATS_PER_MEASURE * f64::from(TICK)).ceil();
    let length = frames(ticks, song.bpm.clamp(20.0, 300.0), setup.sample_rate, true);
    Some(render(library, Mode::Song(index), length, setup, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::{BankSample, KitSample};
    use ordrumbox_dsp::SampleBuffer;

    fn setup() -> RenderSetup {
        let instrument = KitSample {
            key: "KICK".into(),
            display_name: "k".into(),
            path: "k".into(),
            decay: Some(200.0),
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
}
