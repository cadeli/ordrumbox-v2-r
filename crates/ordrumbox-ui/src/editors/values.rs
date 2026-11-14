//! What a track or a note holds, read for the editors: values, arpeggio, tooltips, ordering.

use super::tables::{ARP_DIRECTIONS, FxDef, lfo_key};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Note, ScaleList, Track, num};

// ─── State ───────────────────────────────────────────────────────────────────

/// What the editors remember between frames: the tabs, the selected
/// setting, the filter type an effect button turns back on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorState {
    pub tab: u8,
    pub generation_tab: u8,
    pub note_tab: u8,
    /// The lane under the grid: an index in [`LANES`].
    pub lane: u8,
    /// The Generate tab shows its Advanced settings.
    pub generation_advanced: bool,
    /// The row last clicked.
    pub selected_prop: Option<&'static str>,
    /// The filter type the Filter button turns back on.
    pub prev_filter_type: Option<String>,
}

// ─── Values of a track ───────────────────────────────────────────────────────

pub fn number(track: &Track, key: &str) -> f64 {
    track.field(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

pub(super) fn flag(track: &Track, key: &str) -> bool {
    track.field(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// The text of setting `key`, as a select compares it (empty without one).
pub(super) fn text_of(track: &Track, key: &str) -> String {
    match track.field(key) {
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => ordrumbox_core::num::text(n),
        _ => String::new(),
    }
}

pub(super) fn has_lfo(track: &Track, key: &str) -> bool {
    lfo_key(key).is_some_and(|k| track.field(k).is_some_and(|v| !v.is_null()))
}

/// `isFxOn`.
pub fn fx_on(track: &Track, fx: &FxDef) -> bool {
    if fx.key == "filterType" {
        return track.filter_type != "allpass";
    }
    number(track, fx.key) > 0.0
}

/// `getTrackLoopAtStep`: the loop length, else the track's length in steps.
pub fn loop_length(track: &Track) -> f64 {
    if let Some(l) = track.loop_length.filter(|&l| l > 0) {
        return f64::from(l);
    }
    let spb = track.steps_per_beat;
    let declared = if track.beat_count > 0 && spb > 0 { track.beat_count * spb } else { 0 };
    let last = track
        .notes
        .iter()
        .map(|n| ordrumbox_core::steps::absolute_step(n.beat, n.step, spb) + 1)
        .max()
        .unwrap_or(0)
        .max(0);
    f64::from(declared).max(last as f64)
}

/// The index of `current` in `values`, else 0 (a select with no match shows its first option).
pub fn index_of(values: &[String], current: &str) -> usize {
    values.iter().position(|v| v == current).unwrap_or(0)
}

// ─── Arpeggio (sync_section.js) ──────────────────────────────────────────────

/// What a note's arpeggio is: scale, direction, notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArpState {
    pub scale: String,
    pub direction: String,
    pub range: usize,
}

pub fn arp_state(note: &Note, scales: &ScaleList) -> ArpState {
    let Some(arp) = note.arp.as_object() else {
        return ArpState { scale: "major".into(), direction: "up".into(), range: 0 };
    };
    let mode = arp.get("direction").and_then(Value::as_str).map(str::to_lowercase).unwrap_or_else(|| "up".into());
    let direction = if ARP_DIRECTIONS.contains(&mode.as_str()) { mode } else { "up".into() };
    let intervals: Vec<f64> = arp
        .get("intervals")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default();
    let mut scale = scales.names.first().cloned().unwrap_or_else(|| "major".into());
    for (name, steps) in scales.names.iter().zip(&scales.steps) {
        let Some(steps) = steps else { continue };
        if intervals.is_empty() {
            continue;
        }
        let n = steps.len();
        if intervals.iter().enumerate().all(|(i, &iv)| f64::from(steps[i % n] + (i / n) as i32 * 12) == iv) {
            scale = name.clone();
            break;
        }
    }
    ArpState { scale, direction, range: intervals.len() }
}

/// `#arpValue`: the note's `arp` with these overrides.
pub fn arp_value(
    note: &Note,
    scales: &ScaleList,
    scale: Option<&str>,
    direction: Option<&str>,
    range: Option<usize>,
) -> Value {
    let scale = scale.or(note.arp_scale.as_deref()).unwrap_or("major");
    let direction = direction.or(note.arp_direction.as_deref()).unwrap_or("up");
    let range = range.unwrap_or_else(|| arp_state(note, scales).range);
    if range == 0 {
        return Value::Null;
    }
    let mut object = ordrumbox_core::json::Object::new();
    let intervals = scales.intervals(scale, range).into_iter().map(|i| Value::Number(f64::from(i))).collect();
    object.insert("intervals", Value::Array(intervals));
    object.insert("direction", Value::String(direction.to_owned()));
    Value::Object(object)
}

// ─── Note tooltip (ui_utils.js) ──────────────────────────────────────────────

pub(super) const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// `pitchToNoteName`: the note at `pitch` semitones from C4 (plus the track's pitch).
pub(super) fn pitch_to_note_name(pitch: f64, track_pitch: f64) -> String {
    let midi = 60.0 + track_pitch + pitch;
    let index = num::wrap(midi, 12.0);
    // A fractional pitch has no name.
    let name = if index.fract() == 0.0 { NOTE_NAMES[index as usize] } else { "?" };
    format!("{name}{}", num::text((midi / 12.0).floor() - 1.0))
}

/// `formatNoteTooltip`: a note's pitch and MIDI number, then what differs from the defaults.
pub fn note_tooltip(note: &Note, track_pitch: f64) -> String {
    let fmt = crate::control::fmt;
    let midi = 60.0 + track_pitch + note.pitch;
    let mut parts = vec![format!("{}  MIDI {}", pitch_to_note_name(note.pitch, track_pitch), num::text(midi))];
    if note.velocity != 0.8 {
        parts.push(format!("vel:{}", fmt(note.velocity)));
    }
    if note.probability != 1.0 {
        parts.push(format!("prob:{}", fmt(note.probability)));
    }
    if note.cycle != 1 {
        parts.push(format!("every:{}", note.cycle));
    }
    if note.retrigger_count != 1 {
        parts.push(format!("retrig:{}", note.retrigger_count));
    }
    let intervals = match &note.arp {
        Value::Array(a) => Some(a),
        Value::Object(o) => o.get("intervals").and_then(|v| match v {
            Value::Array(a) => Some(a),
            _ => None,
        }),
        _ => None,
    };
    if let Some(a) = intervals.filter(|a| a.len() >= 2) {
        parts.push(format!(
            "arp:[{}]",
            a.iter().map(|v| v.to_text().unwrap_or_else(|| v.to_json())).collect::<Vec<_>>().join(",")
        ));
    }
    if note.retrigger_probability != 1.0 {
        parts.push(format!("arpProb:{}", fmt(note.retrigger_probability)));
    }
    if note.euclidean_pulses > 0 {
        parts.push(if note.euclidean_rotation != 0 {
            format!("eucl:{}@{}", note.euclidean_pulses, note.euclidean_rotation)
        } else {
            format!("eucl:{}", note.euclidean_pulses)
        });
    }
    if note.retrigger_rate != 1.0 {
        parts.push(format!("rate:{}", fmt(note.retrigger_rate)));
    }
    if note.retrigger_fade != Note::default().retrigger_fade {
        parts.push(format!("fade:{}", fmt(note.retrigger_fade)));
    }
    if note.pan != 0.0 {
        parts.push(format!("pan:{}", fmt(note.pan)));
    }
    if note.length_steps != Note::default().length_steps {
        parts.push(format!("length:{}", fmt(note.length_steps)));
    }
    if note.shift_steps != 0.0 {
        parts.push(format!("shift:{}", fmt(note.shift_steps)));
    }
    parts.join("  ")
}

// ─── Track tooltip ───────────────────────────────────────────────────────────

/// The lines of the bubble over a track's name: the name and its states, the
/// sound it plays (`sound`, whole), then its mix and length, and the generator
/// settings that differ from the defaults.
pub fn track_tooltip(track: &Track, sound: &str) -> Vec<String> {
    let fmt = crate::control::fmt;
    let states = [
        (track.mute, "mute"),
        (track.solo, "solo"),
        (track.auto_mode, "auto"),
        (track.mono, "mono"),
        (track.legato, "legato"),
        (track.auto_assign && !track.use_synth, "auto-assigned"),
    ];
    let mut title = track.name.clone();
    for (_, state) in states.iter().filter(|(on, _)| *on) {
        title.push_str("  ");
        title.push_str(state);
    }
    let mut mix = vec![format!("vol:{}", fmt(track.volume)), format!("pan:{}", fmt(track.pan))];
    if track.transpose != 0.0 {
        mix.push(format!("transpose:{}", fmt(track.transpose)));
    }
    if track.probability != 1.0 {
        mix.push(format!("prob:{}", fmt(track.probability)));
    }
    let mut length = vec![
        format!("notes:{}", track.notes.len()),
        format!("beats:{}", track.beat_count),
        format!("steps/beat:{}", track.steps_per_beat),
    ];
    if let Some(steps) = track.loop_length {
        length.push(format!("loop:{steps}"));
    }
    if track.swing_amount != 0.0 {
        length.push(format!("swing:{}", fmt(track.swing_amount)));
    }
    let mut lines = vec![title];
    if !sound.is_empty() {
        lines.push(sound.to_owned());
    }
    lines.push(mix.join("  "));
    lines.push(length.join("  "));
    lines
}

// ─── localeCompare ───────────────────────────────────────────────────────────

/// `a.localeCompare(b)`, approximated for names: punctuation, then digits,
/// then letters (case-insensitive), lower case first on a tie.
pub fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| -> Vec<(u8, char)> {
        s.chars()
            .map(|c| {
                let class = if c.is_alphabetic() {
                    2
                } else if c.is_numeric() {
                    1
                } else {
                    0
                };
                (class, c.to_lowercase().next().unwrap_or(c))
            })
            .collect()
    };
    key(a).cmp(&key(b)).then_with(|| {
        let tertiary = |s: &str| s.chars().map(|c| c.is_uppercase()).collect::<Vec<_>>();
        tertiary(a).cmp(&tertiary(b))
    })
}
