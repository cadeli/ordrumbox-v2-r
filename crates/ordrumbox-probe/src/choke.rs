//! The choke group as heard: an open hi-hat cut by a closed one of its group,
//! rendered through the engine as the app exports a pattern, with and without
//! the group, and how much of the open hi-hat is left after each cut.

use ordrumbox_core::Library;
use ordrumbox_engine::offline::{self, RenderSetup, Stereo};

use crate::measure::amplitude_db;

/// The choke group both hi-hats are in, when they are in one.
pub const HAT_GROUP: i32 = 1;
/// Tempo of the pattern; each beat is an open hi-hat, then a closed one half a beat later.
const BPM: f64 = 120.0;
const BEAT_COUNT: usize = 4;
const STEPS_PER_BEAT: usize = 4;
/// Step of the beat the closed hi-hat plays on.
const CLOSED_STEP: usize = 2;
/// Loops of the pattern in the mix to listen to.
const LISTEN_LOOPS: u32 = 2;
/// Time after the closed hi-hat before what is left of the open one is
/// measured: twice the voice's 15 ms release.
const SETTLE_SECONDS: f64 = 0.03;

/// What a render with or without the choke group gives.
#[derive(Debug, Clone, PartialEq)]
pub struct ChokeRender {
    /// The mix, the pattern played [`LISTEN_LOOPS`] times.
    pub mix: Stereo,
    /// The open hi-hat's loudest frame before the closed one, dB.
    pub open_db: f64,
    /// The open hi-hat's loudest frame from [`SETTLE_SECONDS`] after the closed
    /// one to the next beat, dB relative to `open_db`; `None` when silent.
    pub left_db: Option<f64>,
}

/// One pattern: OHH on every beat, CHH half a beat later, both in `group`.
fn library(group: Option<i32>) -> Result<Library, String> {
    let choke = group.map_or(String::new(), |g| format!(r#","chokeGroup":{g}"#));
    let notes = |step: usize| {
        let notes: Vec<String> =
            (0..BEAT_COUNT).map(|beat| format!(r#"{{"beat":{beat},"beatStep":{step}}}"#)).collect();
        notes.join(",")
    };
    let track = |name: &str, step: usize| {
        format!(r#"{{"name":"{name}","stepsPerBeat":{STEPS_PER_BEAT},"notes":[{}]{choke}}}"#, notes(step))
    };
    let text = format!(
        r#"{{"bpm":{BPM},"patterns":[{{"id":"choke","name":"Choke","bpm":{BPM},"beatCount":{BEAT_COUNT},"tracks":[{},{}]}}]}}"#,
        track("OHH", 0),
        track("CHH", CLOSED_STEP)
    );
    let (library, _) = Library::parse(&text).map_err(|e| format!("the choke pattern: {e}"))?;
    Ok(library)
}

fn peak(signal: &[f32]) -> f32 {
    signal.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()))
}

/// The open hi-hat's level before each closed one, and what is left of it
/// after, over every beat of its stem.
fn open_levels(open: &[f32], sample_rate: u32) -> (f32, f32) {
    let rate = f64::from(sample_rate);
    let beat_frames = (rate * 60.0 / BPM) as usize;
    let closed_frames = beat_frames * CLOSED_STEP / STEPS_PER_BEAT;
    let settle_frames = (rate * SETTLE_SECONDS) as usize;
    let (mut before, mut after) = (0.0_f32, 0.0_f32);
    for beat in open.chunks(beat_frames) {
        let (head, tail) = beat.split_at(closed_frames.min(beat.len()));
        before = before.max(peak(head));
        after = after.max(peak(tail.get(settle_frames..).unwrap_or_default()));
    }
    (before, after)
}

/// The pattern rendered with `setup`, its hi-hats in `group`.
pub fn render_choke(setup: &RenderSetup, group: Option<i32>) -> Result<ChokeRender, String> {
    let library = library(group)?;
    let stems = offline::render_pattern_stems(&library, 0, setup, None).ok_or("the choke pattern renders nothing")?;
    let (open, _) = stems.first().ok_or("the choke pattern has no OHH stem")?;
    let (before, after) = open_levels(open, setup.sample_rate);
    if before == 0.0 {
        return Err(format!("kit {}: the open hi-hat is silent", setup.kit));
    }
    let mix =
        offline::render_pattern(&library, 0, LISTEN_LOOPS, setup, None).ok_or("the choke pattern renders nothing")?;
    let open_db = amplitude_db(f64::from(before));
    let left_db = (after > 0.0).then(|| amplitude_db(f64::from(after)) - open_db);
    Ok(ChokeRender { mix, open_db, left_db })
}

/// A level relative to the open hi-hat, for a report.
#[must_use]
pub fn left_to_text(left_db: Option<f64>) -> String {
    left_db.map_or_else(|| "silent".to_owned(), |db| format!("{db:.1} dB"))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;
    use std::sync::Arc;

    use ordrumbox_core::Scales;
    use ordrumbox_engine::kit::{BankSample, KitSample};
    use ordrumbox_engine::offline::EXPORT_RATE;
    use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets};

    use super::*;

    /// A kit whose open hi-hat holds its level for 0.4 s (under a beat, so
    /// that the hits never overlap) and whose closed one is a short click.
    fn setup() -> RenderSetup {
        let sample = |key: &str, buffer: Vec<f32>| BankSample {
            kit: "t".into(),
            sample: KitSample {
                key: Some(key.into()),
                display_name: key.to_lowercase(),
                path: format!("t/{key}.wav"),
                decay_ms: Some(100_000.0),
                ..Default::default()
            },
            buffer: SampleBuffer::mono(buffer, EXPORT_RATE).into(),
        };
        let rate = f64::from(EXPORT_RATE);
        let held: Vec<f32> =
            (0..EXPORT_RATE * 2 / 5).map(|i| (0.3 * (TAU * 1000.0 * f64::from(i) / rate).sin()) as f32).collect();
        RenderSetup {
            bank: Arc::new(SampleBank { samples: vec![sample("OHH", held), sample("CHH", vec![0.3; 200])] }),
            scales: Scales::default(),
            synths: SynthPresets::default(),
            kit: "t".into(),
            sample_rate: EXPORT_RATE,
            seed: 1,
            returns: Default::default(),
        }
    }

    #[test]
    fn the_closed_hi_hat_silences_the_open_one_of_its_group_only() {
        let choked = render_choke(&setup(), Some(HAT_GROUP)).unwrap();
        let free = render_choke(&setup(), None).unwrap();
        assert_eq!(choked.open_db, free.open_db);
        assert!(choked.left_db.is_none_or(|db| db < -60.0), "{:?}", choked.left_db);
        assert!(free.left_db.is_some_and(|db| db > -3.0), "{:?}", free.left_db);
        assert_eq!(choked.mix.0.len(), free.mix.0.len());
    }
}
