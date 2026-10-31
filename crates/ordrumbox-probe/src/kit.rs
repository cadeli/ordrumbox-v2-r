//! What is worth a look in a kit: samples that clip, carry DC, are still loud at their end,
//! start late, stray from the kit's level or from the note in their name.

use crate::measure::{Measures, amplitude_db, median};

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const DC_LIMIT: f64 = 0.02;
/// Last 5 ms over the peak: louder than this, the sample is cut or fades very
/// fast, which can click.
const END_LEVEL_LIMIT_DB: f64 = -40.0;
const LEAD_IN_LIMIT_MS: f64 = 5.0;
const LEVEL_OUTLIER_DB: f64 = 6.0;
const DETUNE_LIMIT_CENTS: f64 = 25.0;

/// MIDI number of a note name such as `C3` or `F#2`.
pub fn parse_note(text: &str) -> Result<i32, String> {
    let split = text.find(|c: char| c.is_ascii_digit() || c == '-').ok_or_else(|| format!("{text} has no octave"))?;
    let (name, octave) = text.split_at(split);
    let semitone = NOTE_NAMES
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("{name} is not a note name"))?;
    let octave: i32 = octave.parse().map_err(|_| format!("{octave} is not an octave"))?;
    Ok((octave + 1) * 12 + semitone as i32)
}

/// The note a sample's name declares: `piano-c4` is C4.
pub fn declared_note(name: &str) -> Option<i32> {
    let (_, last) = name.rsplit_once('-')?;
    parse_note(last).ok()
}

/// Cents from the pitch class of `midi` to `hz`, -600 to 600: the detector can
/// land on another octave of the same note, which is no detuning.
fn cents_from(midi: i32, hz: f64) -> f64 {
    let semitones = 69.0 + 12.0 * (hz / 440.0).log2() - f64::from(midi);
    (semitones - (semitones / 12.0).round() * 12.0) * 100.0
}

/// What is worth a look in one sample, `kit_peak_db` the median peak of its kit.
pub fn sample_flags(name: &str, m: &Measures, kit_peak_db: f64) -> Vec<String> {
    let mut flags = Vec::new();
    if m.peak == 0.0 {
        flags.push("silent".to_owned());
        return flags;
    }
    if m.clipped_frames > 0 {
        flags.push(format!("{} frames at full scale", m.clipped_frames));
    }
    if m.dc.abs() > DC_LIMIT {
        flags.push(format!("DC {:.3}", m.dc));
    }
    if let Some(level) = m.end_level_db.filter(|level| *level > END_LEVEL_LIMIT_DB) {
        flags.push(format!("still loud at its end ({level:.0} dB)"));
    }
    if m.lead_in_ms > LEAD_IN_LIMIT_MS {
        flags.push(format!("starts {:.0} ms late", m.lead_in_ms));
    }
    let from_kit = amplitude_db(m.peak) - kit_peak_db;
    if from_kit.abs() > LEVEL_OUTLIER_DB {
        flags.push(format!("peak {from_kit:+.1} dB from the kit"));
    }
    if let Some(midi) = declared_note(name) {
        match m.fundamental_hz {
            None => flags.push("named after a note, no pitch found".to_owned()),
            Some(hz) => {
                let cents = cents_from(midi, hz);
                if cents.abs() > DETUNE_LIMIT_CENTS {
                    flags.push(format!("{cents:+.0} ct from its name"));
                }
            }
        }
    }
    flags
}

/// One kit's summary.
#[derive(Debug, Clone, PartialEq)]
pub struct KitSummary {
    pub name: String,
    pub sample_count: usize,
    pub peak_min_db: f64,
    pub peak_max_db: f64,
    pub rms_min_db: f64,
    pub rms_max_db: f64,
    pub sample_rates: Vec<u32>,
    /// Sample name and what is worth a look in it.
    pub flagged: Vec<(String, Vec<String>)>,
}

/// Summarize a kit from its samples' names and measures.
pub fn summarize(name: &str, samples: &[(String, Measures)]) -> KitSummary {
    let peaks: Vec<f64> = samples.iter().filter(|(_, m)| m.peak > 0.0).map(|(_, m)| amplitude_db(m.peak)).collect();
    let rms: Vec<f64> = samples.iter().map(|(_, m)| m.rms_db).filter(|db| db.is_finite()).collect();
    let kit_peak_db = median(&peaks).unwrap_or(0.0);
    let low = |values: &[f64]| values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = |values: &[f64]| values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut sample_rates: Vec<u32> = samples.iter().map(|(_, m)| m.sample_rate).collect();
    sample_rates.sort_unstable();
    sample_rates.dedup();
    let flagged = samples
        .iter()
        .map(|(sample, m)| (sample.clone(), sample_flags(sample, m, kit_peak_db)))
        .filter(|(_, flags)| !flags.is_empty())
        .collect();
    KitSummary {
        name: name.to_owned(),
        sample_count: samples.len(),
        peak_min_db: low(&peaks),
        peak_max_db: high(&peaks),
        rms_min_db: low(&rms),
        rms_max_db: high(&rms),
        sample_rates,
        flagged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> Measures {
        Measures { peak: 0.5, rms_db: -20.0, sample_rate: 44_100, end_level_db: Some(-70.0), ..Measures::default() }
    }

    #[test]
    fn notes_are_read_from_names() {
        assert_eq!(parse_note("C3"), Ok(48));
        assert_eq!(parse_note("f#2"), Ok(42));
        assert!(parse_note("H3").is_err() && parse_note("C").is_err());
        assert_eq!(declared_note("acid-synth-c4"), Some(60));
        assert_eq!(declared_note("kick"), None);
        assert_eq!(declared_note("kick-deep"), None);
    }

    #[test]
    fn a_clean_sample_has_no_flag() {
        assert!(sample_flags("kick", &clean(), amplitude_db(0.5)).is_empty());
    }

    #[test]
    fn each_problem_is_flagged() {
        let kit = amplitude_db(0.5);
        let flags = |m: Measures| sample_flags("kick", &m, kit);
        assert_eq!(flags(Measures { dc: -0.1, ..clean() }), ["DC -0.100"]);
        assert_eq!(flags(Measures { clipped_frames: 3, ..clean() }), ["3 frames at full scale"]);
        assert_eq!(flags(Measures { end_level_db: Some(-12.0), ..clean() }), ["still loud at its end (-12 dB)"]);
        assert_eq!(flags(Measures { lead_in_ms: 20.0, ..clean() }), ["starts 20 ms late"]);
        assert_eq!(flags(Measures { peak: 0.1, ..clean() }), ["peak -14.0 dB from the kit"]);
        assert_eq!(flags(Measures { peak: 0.0, ..clean() }), ["silent"]);
    }

    #[test]
    fn only_a_named_note_is_checked_and_octaves_do_not_count() {
        let kit = amplitude_db(0.5);
        let at = |hz: f64| Measures { fundamental_hz: Some(hz), ..clean() };
        assert!(sample_flags("piano-c4", &at(261.6), kit).is_empty());
        assert!(sample_flags("piano-c4", &at(130.8), kit).is_empty());
        assert_eq!(sample_flags("piano-c4", &at(277.2), kit), ["+100 ct from its name"]);
        assert_eq!(sample_flags("piano-c4", &clean(), kit), ["named after a note, no pitch found"]);
        assert!(sample_flags("kick", &at(277.2), kit).is_empty());
    }

    #[test]
    fn the_summary_spans_the_kit() {
        let samples = vec![
            ("a".to_owned(), clean()),
            ("b".to_owned(), clean()),
            ("c".to_owned(), Measures { peak: 0.1, rms_db: -40.0, sample_rate: 48_000, ..clean() }),
        ];
        let summary = summarize("kit", &samples);
        assert_eq!(summary.sample_count, 3);
        assert!((summary.peak_max_db - summary.peak_min_db - 13.98).abs() < 0.01);
        assert_eq!((summary.rms_min_db, summary.rms_max_db), (-40.0, -20.0));
        assert_eq!(summary.sample_rates, [44_100, 48_000]);
        assert_eq!(summary.flagged.len(), 1);
        assert_eq!(summary.flagged[0].0, "c");
    }
}
