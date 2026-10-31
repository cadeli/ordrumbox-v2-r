//! Measurements as JSON, as text, and the differences between two reports.

use ordrumbox_core::json::{Object, Value};

use crate::kit::KitSummary;
use crate::measure::{BandLevel, Measures, TimePoint};
use crate::mix::MixMeasures;

/// Points kept of the envelope in a report.
const ENVELOPE_REPORT_POINTS: usize = 200;
/// A change under this share of the value, or of 1, is not reported.
const COMPARE_TOLERANCE: f64 = 0.005;
/// Holds the curves, which a comparison leaves out.
const CURVES_KEY: &str = "curves";

/// A number to 3 decimals; null when it is not finite (the dB of silence).
fn number(value: f64) -> Value {
    if value.is_finite() { Value::Number((value * 1000.0).round() / 1000.0) } else { Value::Null }
}

fn optional(value: Option<f64>) -> Value {
    value.map_or(Value::Null, number)
}

fn curve(points: &[TimePoint]) -> Value {
    let row = |p: &TimePoint| Value::from(vec![number(p.ms), number(p.value)]);
    Value::from(points.iter().map(row).collect::<Vec<Value>>())
}

fn bands_to_json(bands: &[BandLevel]) -> Value {
    let mut o = Object::new();
    for band in bands {
        let mut level = Object::new();
        level.insert("share", number(band.share));
        level.insert("db", number(band.db));
        o.insert(band.name, Value::from(level));
    }
    Value::from(o)
}

fn bands_to_text(bands: &[BandLevel]) -> String {
    let band = |b: &BandLevel| format!("{} {:.0}% {:.0} dB", b.name, b.share * 100.0, b.db);
    bands.iter().map(band).collect::<Vec<String>>().join(", ")
}

/// One sound of a report.
pub fn sound_to_json(name: &str, m: &Measures, repeat_difference_db: Option<Option<f64>>) -> Value {
    let mut o = Object::new();
    o.insert("name", Value::from(name));
    o.insert("sampleRate", Value::from(m.sample_rate));
    o.insert("durationMs", number(m.duration_ms));
    o.insert("peak", number(m.peak));
    o.insert("rmsDb", number(m.rms_db));
    o.insert("dc", number(m.dc));
    o.insert("crestDb", number(m.crest_db));
    o.insert("clippedFrames", number(m.clipped_frames as f64));
    o.insert("balanceDb", number(m.balance_db));
    o.insert("attackMs", optional(m.attack_ms));
    o.insert("envelopePeakMs", optional(m.envelope_peak_ms));
    o.insert("decay20dbMs", optional(m.decay_20db_ms));
    o.insert("endMs", optional(m.end_ms));
    o.insert("centroidHz", optional(m.centroid_hz));
    o.insert("rolloffHz", optional(m.rolloff_hz));
    o.insert("bands", bands_to_json(&m.bands));
    o.insert("leadInMs", number(m.lead_in_ms));
    o.insert("endLevelDb", optional(m.end_level_db));
    o.insert("flatness", optional(m.flatness));
    o.insert("fundamentalHz", optional(m.fundamental_hz));
    o.insert("note", m.note.as_deref().map_or(Value::Null, Value::from));
    o.insert("pitchConfidence", number(m.pitch_confidence));
    o.insert("harmonicsDb", Value::from(m.harmonics_db.iter().map(|&db| number(db)).collect::<Vec<Value>>()));
    o.insert("subOctaveDb", optional(m.sub_octave_db));
    o.insert("pitchRangeCents", optional(m.pitch_range_cents));
    o.insert("brightnessRangeOctaves", optional(m.brightness_range_octaves));
    if let Some(difference) = repeat_difference_db {
        o.insert("repeatDifferenceDb", optional(difference));
    }
    let peak = |p: &crate::measure::SpectralPeak| Value::from(vec![number(p.hz), number(p.db)]);
    o.insert("peaks", Value::from(m.peaks.iter().map(peak).collect::<Vec<Value>>()));
    let step = (m.envelope_db.len() / ENVELOPE_REPORT_POINTS).max(1);
    let envelope: Vec<TimePoint> = m.envelope_db.iter().step_by(step).copied().collect();
    let mut curves = Object::new();
    curves.insert("envelopeDb", curve(&envelope));
    curves.insert("pitchHz", curve(&m.pitch_track));
    curves.insert("centroidHz", curve(&m.centroid_track));
    o.insert(CURVES_KEY, Value::from(curves));
    Value::from(o)
}

/// A report: what was rendered and its sounds.
pub fn report_to_json(settings: Object, sounds: Vec<Value>) -> Value {
    let mut o = Object::new();
    o.insert("settings", Value::from(settings));
    o.insert("sounds", Value::from(sounds));
    Value::from(o)
}

fn text_or_none(value: Option<f64>, unit: &str, decimals: usize) -> String {
    value.map_or_else(|| "none".to_owned(), |v| format!("{v:.decimals$}{unit}"))
}

/// A few lines about one sound, for the terminal.
pub fn sound_to_text(name: &str, m: &Measures, repeat_difference_db: Option<Option<f64>>) -> String {
    let mut text = format!("{name}\n");
    text += &format!(
        "  level     peak {:.3}  rms {:.1} dB  crest {:.1} dB  dc {:.3}  clipped {}  balance {:.1} dB\n",
        m.peak, m.rms_db, m.crest_db, m.dc, m.clipped_frames, m.balance_db
    );
    text += &format!(
        "  envelope  length {:.0} ms  attack {}  peak at {}  decay -20 dB {}  end -60 dB {}\n",
        m.duration_ms,
        text_or_none(m.attack_ms, " ms", 1),
        text_or_none(m.envelope_peak_ms, " ms", 0),
        text_or_none(m.decay_20db_ms, " ms", 0),
        text_or_none(m.end_ms, " ms", 0),
    );
    text += &format!(
        "  spectrum  centroid {}  rolloff {}  flatness {}  brightness moves {}\n",
        text_or_none(m.centroid_hz, " Hz", 0),
        text_or_none(m.rolloff_hz, " Hz", 0),
        text_or_none(m.flatness, "", 3),
        text_or_none(m.brightness_range_octaves, " oct", 2),
    );
    text += &format!("  bands     {}\n", bands_to_text(&m.bands));
    let peaks: Vec<String> = m.peaks.iter().take(5).map(|p| format!("{:.0} Hz {:.0} dB", p.hz, p.db)).collect();
    text += &format!("  peaks     {}\n", peaks.join(", "));
    let harmonics: Vec<String> = m.harmonics_db.iter().map(|db| format!("{db:.0}")).collect();
    text += &format!(
        "  pitch     {} ({}, confidence {:.2})  moves {}  sub-octave {}  harmonics dB [{}]\n",
        text_or_none(m.fundamental_hz, " Hz", 1),
        m.note.as_deref().unwrap_or("no note"),
        m.pitch_confidence,
        text_or_none(m.pitch_range_cents, " ct", 0),
        text_or_none(m.sub_octave_db, " dB", 0),
        harmonics.join(" "),
    );
    if let Some(difference) = repeat_difference_db {
        let text_of = difference.map_or_else(|| "identical".to_owned(), |db| format!("{db:.1} dB of difference"));
        text += &format!("  repeat    a later note: {text_of}\n");
    }
    text
}

/// One mix of a report: what holds over a whole mix, no attack or pitch.
pub fn mix_to_json(name: &str, m: &Measures, mix: &MixMeasures) -> Value {
    let mut o = Object::new();
    o.insert("name", Value::from(name));
    o.insert("sampleRate", Value::from(m.sample_rate));
    o.insert("durationMs", number(m.duration_ms));
    o.insert("integratedLufs", optional(mix.integrated_lufs));
    o.insert("maxMomentaryLufs", optional(mix.max_momentary_lufs));
    o.insert("maxShortTermLufs", optional(mix.max_short_term_lufs));
    o.insert("loudnessRangeLu", optional(mix.loudness_range_lu));
    o.insert("truePeak", number(mix.true_peak));
    o.insert("peakToLoudnessDb", optional(mix.peak_to_loudness_db));
    o.insert("peak", number(m.peak));
    o.insert("rmsDb", number(m.rms_db));
    o.insert("dc", number(m.dc));
    o.insert("clippedFrames", number(m.clipped_frames as f64));
    o.insert("balanceDb", number(m.balance_db));
    o.insert("centroidHz", optional(m.centroid_hz));
    o.insert("rolloffHz", optional(m.rolloff_hz));
    o.insert("bands", bands_to_json(&m.bands));
    o.insert("hitCount", number(mix.hits.len() as f64));
    let mut by_band = Object::new();
    for band in &mix.band_hits {
        let mut hits = Object::new();
        hits.insert("count", number(band.count as f64));
        hits.insert("levelSpreadDb", number(band.level_spread_db));
        by_band.insert(band.band, Value::from(hits));
    }
    o.insert("hitsByBand", Value::from(by_band));
    if let Some(grid) = mix.grid {
        let mut fit = Object::new();
        fit.insert("offsetMs", number(grid.offset_ms));
        fit.insert("maxDeviationMs", number(grid.max_deviation_ms));
        fit.insert("offGridCount", number(grid.off_grid_count as f64));
        o.insert("grid", Value::from(fit));
    }
    let hit = |h: &crate::mix::Hit| Value::from(vec![number(h.ms), number(h.peak), Value::from(h.band)]);
    let mut curves = Object::new();
    curves.insert("hits", Value::from(mix.hits.iter().map(hit).collect::<Vec<Value>>()));
    o.insert(CURVES_KEY, Value::from(curves));
    Value::from(o)
}

/// A few lines about one mix, for the terminal.
pub fn mix_to_text(name: &str, m: &Measures, mix: &MixMeasures) -> String {
    let mut text = format!("{name}\n");
    text += &format!(
        "  loudness  {} integrated  {} momentary max  {} short-term max  range {}\n",
        text_or_none(mix.integrated_lufs, " LUFS", 1),
        text_or_none(mix.max_momentary_lufs, " LUFS", 1),
        text_or_none(mix.max_short_term_lufs, " LUFS", 1),
        text_or_none(mix.loudness_range_lu, " LU", 1),
    );
    text += &format!(
        "  peaks     true peak {:.3} ({:.1} dBTP)  sample peak {:.3}  clipped {}  peak to loudness {}\n",
        mix.true_peak,
        crate::measure::amplitude_db(mix.true_peak),
        m.peak,
        m.clipped_frames,
        text_or_none(mix.peak_to_loudness_db, " dB", 1),
    );
    text += &format!("  stereo    balance {:.1} dB  dc {:.3}  length {:.0} ms\n", m.balance_db, m.dc, m.duration_ms);
    text += &format!(
        "  spectrum  centroid {}  rolloff {}\n",
        text_or_none(m.centroid_hz, " Hz", 0),
        text_or_none(m.rolloff_hz, " Hz", 0)
    );
    text += &format!("  bands     {}\n", bands_to_text(&m.bands));
    let by_band = |b: &crate::mix::BandHits| format!("{} {} (spread {:.1} dB)", b.band, b.count, b.level_spread_db);
    let list: Vec<String> = mix.band_hits.iter().map(by_band).collect();
    text += &format!("  hits      {}: {}\n", mix.hits.len(), list.join(", "));
    if let Some(grid) = mix.grid {
        text += &format!(
            "  grid      offset {:.1} ms  largest deviation {:.1} ms  {} hits over 10 ms off\n",
            grid.offset_ms, grid.max_deviation_ms, grid.off_grid_count
        );
    }
    text
}

/// A kit's summary, for a report.
pub fn kit_to_json(summary: &KitSummary) -> Value {
    let mut o = Object::new();
    o.insert("name", Value::from(summary.name.as_str()));
    o.insert("sampleCount", number(summary.sample_count as f64));
    o.insert("peakMinDb", number(summary.peak_min_db));
    o.insert("peakMaxDb", number(summary.peak_max_db));
    o.insert("rmsMinDb", number(summary.rms_min_db));
    o.insert("rmsMaxDb", number(summary.rms_max_db));
    let rates: Vec<Value> = summary.sample_rates.iter().map(|&rate| Value::from(rate)).collect();
    o.insert("sampleRates", Value::from(rates));
    let mut flagged = Object::new();
    for (sample, flags) in &summary.flagged {
        let list: Vec<Value> = flags.iter().map(|flag| Value::from(flag.as_str())).collect();
        flagged.insert(sample.as_str(), Value::from(list));
    }
    o.insert("flagged", Value::from(flagged));
    Value::from(o)
}

/// A kit's summary and what is worth a look in it, for the terminal.
pub fn kit_to_text(summary: &KitSummary) -> String {
    let rates: Vec<String> = summary.sample_rates.iter().map(u32::to_string).collect();
    let mut text = format!(
        "{}: {} samples  peak {:.1} to {:.1} dB  rms {:.1} to {:.1} dB  {} Hz\n",
        summary.name,
        summary.sample_count,
        summary.peak_min_db,
        summary.peak_max_db,
        summary.rms_min_db,
        summary.rms_max_db,
        rates.join(", ")
    );
    for (sample, flags) in &summary.flagged {
        text += &format!("  {sample}: {}\n", flags.join(", "));
    }
    text
}

/// The scalars of a sound under their keys, arrays by index, curves left out.
fn flatten(prefix: &str, value: &Value, out: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(object) => {
            for (key, item) in object.iter().filter(|(key, _)| *key != CURVES_KEY) {
                let path = if prefix.is_empty() { key.to_owned() } else { format!("{prefix}.{key}") };
                flatten(&path, item, out);
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                flatten(&format!("{prefix}[{i}]"), item, out);
            }
        }
        scalar => out.push((prefix.to_owned(), scalar.clone())),
    }
}

fn differs(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() > COMPARE_TOLERANCE * a.abs().max(1.0),
        _ => a != b,
    }
}

fn scalar_text(value: Option<&Value>) -> String {
    value.map_or_else(|| "absent".to_owned(), Value::to_json)
}

fn sounds(report: &Value) -> Result<&[Value], String> {
    report.get("sounds").and_then(Value::as_array).ok_or_else(|| "a report holds a \"sounds\" array".to_owned())
}

/// What changed from report `before` to report `after`, sound by sound.
pub fn compare(before: &Value, after: &Value) -> Result<String, String> {
    let (old_sounds, new_sounds) = (sounds(before)?, sounds(after)?);
    let name_of = |sound: &Value| sound.get("name").and_then(Value::as_str).map(str::to_owned);
    let mut text = String::new();
    let mut changed_count = 0;
    for old in old_sounds {
        let Some(name) = name_of(old) else { return Err("a sound of a report has no name".to_owned()) };
        let Some(new) = new_sounds.iter().find(|s| name_of(s).as_deref() == Some(&name)) else {
            text += &format!("{name}: only in the first report\n");
            continue;
        };
        let (mut old_flat, mut new_flat) = (Vec::new(), Vec::new());
        flatten("", old, &mut old_flat);
        flatten("", new, &mut new_flat);
        let find = |flat: &'_ [(String, Value)], key: &str| flat.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let mut keys: Vec<&str> = old_flat.iter().map(|(k, _)| k.as_str()).collect();
        keys.extend(new_flat.iter().map(|(k, _)| k.as_str()).filter(|k| !old_flat.iter().any(|(o, _)| o == k)));
        let mut lines = String::new();
        for key in keys {
            let (a, b) = (find(&old_flat, key), find(&new_flat, key));
            let changed = match (&a, &b) {
                (Some(a), Some(b)) => differs(a, b),
                _ => true,
            };
            if changed {
                lines += &format!("  {key}: {} -> {}\n", scalar_text(a.as_ref()), scalar_text(b.as_ref()));
            }
        }
        if !lines.is_empty() {
            changed_count += 1;
            text += &format!("{name}\n{lines}");
        }
    }
    for new in new_sounds {
        if let Some(name) = name_of(new)
            && !old_sounds.iter().any(|s| name_of(s).as_deref() == Some(&name))
        {
            text += &format!("{name}: only in the second report\n");
        }
    }
    text += &format!("{changed_count} of {} sounds changed\n", old_sounds.len());
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn compare_reports_real_changes_only() {
        let before = json::parse(
            r#"{"sounds":[{"name":"A","peak":0.5,"dc":0.0,"harmonicsDb":[0,-6],"curves":{"x":[1]}},{"name":"B","peak":1},{"name":"C","peak":1}]}"#,
        )
        .unwrap();
        let after = json::parse(
            r#"{"sounds":[{"name":"A","peak":0.502,"dc":0.6,"harmonicsDb":[0,-12],"curves":{"x":[2]}},{"name":"B","peak":1},{"name":"D","peak":1}]}"#,
        )
        .unwrap();
        let text = compare(&before, &after).unwrap();
        assert!(text.contains("dc: 0 -> 0.6"), "{text}");
        assert!(text.contains("harmonicsDb[1]: -6 -> -12"), "{text}");
        assert!(!text.contains("peak") && !text.contains("curves"), "{text}");
        assert!(
            text.contains("C: only in the first report") && text.contains("D: only in the second report"),
            "{text}"
        );
        assert!(text.contains("1 of 3 sounds changed"), "{text}");
        assert!(compare(&Value::Null, &after).is_err());
    }
}
