//! The track editor (`ui/track_editor.js` and its sections), the note editor
//! (`ui/note_editor.js`, `note_editor/sync_section.js`) and the kit editor's
//! controls (`ui/drumkit_manager/detail_section.js`), as [`panel`] items:
//! what each shows, in the JS order, with the JS ranges, defaults and
//! formats. What the widgets do is in the session.

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::js;
use ordrumbox_core::json::Value;
use ordrumbox_core::{Note, ScaleList, Track};
use ordrumbox_engine::strip_settings::delay_beats_used;
use ordrumbox_logic::generate;

use crate::canvas::text_width;
use crate::control::{Format, Spec};
use crate::kits::{DECAY_RANGE, Field, GAIN_RANGE, TRANSPOSE_RANGE};
use crate::panel::{ACCENT, DIM, Id, Item, Kind, LfoParam, Rect, label_width, text_color};

// ─── Tables (track_editor_constants.js, sync_section.js) ─────────────────────

/// Main tabs.
pub const TABS: [&str; 5] = ["FX", "Sound", "LFO", "Loop", "Gen"];
/// The index of the Gen tab in [`TABS`].
pub const GEN_TAB: u8 = 4;
pub const TAB_FX: u8 = 0;
pub const TAB_SOUND: u8 = 1;
pub const TAB_LFO: u8 = 2;
pub const TAB_LOOP: u8 = 3;

/// An effect: the setting its LED reads and writes, its tab label, its rows.
pub struct FxDef {
    pub key: &'static str,
    pub label: &'static str,
    pub controls: &'static [&'static str],
}

pub const FX_DEFS: [FxDef; 4] = [
    FxDef { key: "reverbMix", label: "Rev", controls: &["reverbMix", "reverbType"] },
    FxDef { key: "delayMix", label: "Dly", controls: &["delayMix", "delayBeats", "delayFeedback", "delayType"] },
    FxDef { key: "saturationMix", label: "Sat", controls: &["saturationMix", "saturationType"] },
    // The filter is on when its type is not the allpass bypass.
    FxDef { key: "filterType", label: "Flt", controls: &["filterType", "filterSlope", "filterCutoff", "filterQ"] },
];

pub const FILTER_TYPES: [&str; 4] = ["lowpass", "highpass", "bandpass", "notch"];
const FILTER_ICONS: [&str; 4] = ["LP", "HP", "BP", "NT"];
/// Cutoff a filter gets when switched on from its 20 Hz default, Hz (a
/// lowpass at 20 Hz is silence).
pub const FILTER_ON_CUTOFF: f64 = 1_000.0;

/// Delay time multipliers, in beats, and their labels (`fx_values.js`).
const DELAY_TIME_VALUES: [&str; 7] = ["0.0625", "0.125", "0.25", "0.5", "1", "2", "4"];
const DELAY_TIME_LABELS: [&str; 7] = ["1/16", "1/8", "1/4", "1/2", "1", "2", "4"];

/// Filter slopes, dB per octave, and their labels.
const FILTER_SLOPE_VALUES: [&str; 2] = ["12", "24"];
const FILTER_SLOPE_LABELS: [&str; 2] = ["12 dB", "24 dB"];

/// Selects of a track setting whose values are numbers (written as numbers).
pub const NUMERIC_SELECTS: [&str; 2] = ["delayBeats", "filterSlope"];

/// LFO waveforms (`WAVE_TYPES`).
pub const WAVE_TYPES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];

/// What a select of a track setting offers: values and labels.
pub fn track_select_options(key: &str) -> (Vec<String>, Vec<String>) {
    let list = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match key {
        "reverbType" => (list(&["none", "room", "hall", "plate", "spring", "gated"]), Vec::new()),
        "delayBeats" => (list(&DELAY_TIME_VALUES), list(&DELAY_TIME_LABELS)),
        "filterSlope" => (list(&FILTER_SLOPE_VALUES), list(&FILTER_SLOPE_LABELS)),
        "delayType" => (list(&["none", "slap", "tape", "pingpong"]), Vec::new()),
        "saturationType" => (list(&["soft", "hard", "tape"]), Vec::new()),
        "autoVariant" => {
            // Every melodic variant (a bass track plays the bass ones, a chord track the others).
            let mut names = vec![String::new()];
            for name in ["bass", "melody"].into_iter().flat_map(generate::variant_names) {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_owned());
                }
            }
            (names, Vec::new())
        }
        _ => (Vec::new(), Vec::new()),
    }
}

/// A target of the modulation tab: the setting, its short label, its name,
/// its LFO's key, its range and step.
pub struct LfoTarget {
    pub key: &'static str,
    pub label: &'static str,
    pub name: &'static str,
    pub lfo_key: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// The settings that take an LFO, in the JS order (track props, then the knob bar's).
pub const LFO_TARGETS: [LfoTarget; 5] = [
    LfoTarget {
        key: "filterCutoff",
        label: "Cutoff",
        name: "filter cutoff, Hz",
        lfo_key: "filterCutoffLfo",
        min: 20.0,
        max: 20_000.0,
        step: 1.0,
    },
    LfoTarget {
        key: "filterQ",
        label: "Q",
        name: "filter Q",
        lfo_key: "filterQLfo",
        min: 0.707,
        max: 18.707,
        step: 0.01,
    },
    LfoTarget { key: "volume", label: "Vol", name: "volume", lfo_key: "volumeLfo", min: 0.0, max: 1.0, step: 0.01 },
    LfoTarget { key: "pan", label: "Pan", name: "pan", lfo_key: "panLfo", min: -1.0, max: 1.0, step: 0.01 },
    LfoTarget {
        key: "transpose",
        label: "Transp",
        name: "transpose, semitones",
        lfo_key: "transposeLfo",
        min: -24.0,
        max: 24.0,
        step: 1.0,
    },
];

/// The LFO key of a setting, if it takes one.
pub fn lfo_key(key: &str) -> Option<&'static str> {
    LFO_TARGETS.iter().find(|t| t.key == key).map(|t| t.lfo_key)
}

/// A value setting of the track editor: key, label, range, step, format.
#[derive(Debug, Clone, Copy)]
pub struct Prop {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub format: Format,
}

const fn prop(key: &'static str, label: &'static str, min: f64, max: f64, step: f64, format: Format) -> Prop {
    Prop { key, label, min, max, step, format }
}

/// The effect knobs.
pub const FX_KNOBS: [Prop; 6] = [
    prop("reverbMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("delayMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("delayFeedback", "Fdbk", 0.0, 0.95, 0.01, Format::Plain),
    prop("saturationMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("filterCutoff", "Freq", 20.0, 20_000.0, 1.0, Format::Freq),
    prop("filterQ", "Q", 0.707, 18.707, 0.01, Format::Fixed2),
];

/// Labels of the effect rows that are not knobs.
pub fn fx_label(key: &str) -> &'static str {
    match key {
        "delayBeats" => "Beats",
        "filterSlope" => "Slope",
        _ => "Type",
    }
}

/// The knob bar (volume, pan, pitch, decay) and its units.
pub const KNOB_BAR: [(Prop, &str); 4] = [
    (prop("volume", "Vol", 0.0, 1.0, 0.01, Format::Percent), "%"),
    (prop("pan", "Pan", -1.0, 1.0, 0.01, Format::Fmt), ""),
    (prop("transpose", "Transp", -24.0, 24.0, 1.0, Format::SignedPlain), "st"),
    (prop("decay", "Decay", DECAY_RANGE.0, DECAY_RANGE.1, 10.0, Format::Millis), ""),
];

/// Decay a double-click restores (the knob bar's default).
pub const DEFAULT_DECAY: f64 = 500.0;

/// Generation tab, shown while auto is off.
pub const GENERATION_MANUAL: [Prop; 3] = [
    prop("hitVariation", "Var Hits", 0.0, 100.0, 1.0, Format::Plain),
    prop("noteVariation", "Var Notes", 0.0, 100.0, 1.0, Format::Plain),
    prop("probability", "Prob", 0.0, 1.0, 0.01, Format::Plain),
];

/// A row of the Groove / Engine sub-tabs.
#[derive(Debug, Clone, Copy)]
pub enum GenerationRow {
    Slider(Prop),
    Toggle(&'static str, &'static str),
    Select(&'static str, &'static str),
}

pub const GENERATION_GROOVE: [GenerationRow; 8] = [
    GenerationRow::Slider(prop("autoHitVariation", "Var Hits", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoPitchChance", "Pitch", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoVelocityChance", "Velocity", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoSilenceChance", "Silence", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoFillChance", "Fill", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoGhostChance", "Ghost", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("pitchRange", "Pitch Rng", 1.0, 24.0, 1.0, Format::Plain)),
    GenerationRow::Toggle("pitchScaleLock", "Scale Lock"),
];

pub const GENERATION_ENGINE: [GenerationRow; 8] = [
    GenerationRow::Slider(prop("autoNoteVariation", "Var Notes", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoRetriggerChance", "Retrig", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoEuclideanChance", "Euclid", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoNoteProbabilityChance", "Note Prob", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoNoteProbabilityRange", "Depth", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Slider(prop("autoArpChance", "Arp", 0.0, 100.0, 1.0, Format::Plain)),
    GenerationRow::Select("autoVariant", "Variant"),
    GenerationRow::Slider(prop("autoDensity", "Density", -1.0, 1.0, 0.01, Format::Density)),
];

/// The Compose sub-tab: how a drum track plays its style's part.
pub const GENERATION_COMPOSE: [Prop; 7] = [
    prop("composeHitsPerMeasure", "Hits/Meas", 0.0, 32.0, 1.0, Format::Plain),
    prop("composeComplexity", "Complexity", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeSyncopation", "Syncopation", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeCoupling", "Coupling", -100.0, 100.0, 1.0, Format::Plain),
    prop("composeVariation", "Variation", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeStyleWeight", "Style", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeSeed", "Seed", 0.0, 9999.0, 1.0, Format::Seed),
];

/// What a double-click on a compose slider sets: the setting given back to
/// the style (`None`). Below every range.
pub const STYLE_DEFAULT: f64 = -1.0e9;

/// The sub-tab index of Compose.
pub const COMPOSE_TAB: u8 = 2;

/// The loop tab.
pub const LOOP_PROPS: [Prop; 3] = [
    prop("stepsPerBeat", "Steps/Beat", 1.0, 8.0, 1.0, Format::Plain),
    prop("loopLength", "Loop Len", 1.0, 128.0, 1.0, Format::Plain),
    prop("swingAmount", "Swing", 0.0, 1.0, 0.01, Format::Plain),
];

/// Note editor tabs.
pub const NOTE_TABS: [&str; 4] = ["Trig", "Retr", "Eucl", "Arp"];

/// The note editor's knobs and their units.
pub const NOTE_KNOBS: [(Prop, &str); 3] = [
    (prop("velocity", "Vel", 0.0, 1.0, 0.01, Format::Percent), "%"),
    (prop("pitch", "Pitch", -24.0, 24.0, 1.0, Format::SignedPlain), "st"),
    (prop("pan", "Pan", -1.0, 1.0, 0.01, Format::Fmt), ""),
];

/// A row of a note editor tab.
#[derive(Debug, Clone, Copy)]
pub enum NoteRow {
    Slider(Prop),
    Select(&'static str, &'static str),
}

const fn note_slider(key: &'static str, label: &'static str, min: f64, max: f64, step: f64) -> NoteRow {
    NoteRow::Slider(prop(key, label, min, max, step, Format::Fmt))
}

/// The rows of each note tab. As in the JS, the Euclidean tab's "Prob"
/// (the same setting as the Retrig tab's) never shows: both sliders share
/// one key, and the second one takes the first one's place.
pub const NOTE_GROUPS: [&[NoteRow]; 4] = [
    &[
        note_slider("cycle", "Cycle", 1.0, 16.0, 1.0),
        note_slider("cycleOffset", "Offset", 0.0, 15.0, 1.0),
        note_slider("probability", "Prob", 0.0, 1.0, 0.01),
    ],
    &[
        note_slider("retriggerCount", "Retrig", 1.0, 16.0, 1.0),
        note_slider("retriggerRate", "Rate", 1.0, 16.0, 1.0),
        note_slider("retriggerProbability", "Prob", 0.0, 1.0, 0.01),
    ],
    &[
        note_slider("euclideanPulses", "Pulses", 0.0, 16.0, 1.0),
        note_slider("euclideanRotation", "Rot", 0.0, 15.0, 1.0),
    ],
    &[
        NoteRow::Select("arpScale", "Scale"),
        NoteRow::Select("arpDirection", "Dir"),
        note_slider("arpRange", "Range", 0.0, 12.0, 1.0),
    ],
];

/// The range of a note setting in the note editor. Not the pitch: the knob
/// spans two octaves, the piano roll more.
pub fn note_range(key: &str) -> Option<(f64, f64)> {
    let knobs = NOTE_KNOBS.iter().map(|(prop, _)| prop);
    let sliders = NOTE_GROUPS.iter().flat_map(|rows| rows.iter()).filter_map(|row| match row {
        NoteRow::Slider(prop) => Some(prop),
        NoteRow::Select(..) => None,
    });
    let prop = knobs.chain(sliders).find(|prop| prop.key == key && key != "pitch")?;
    Some((prop.min, prop.max))
}

pub const ARP_DIRECTIONS: [&str; 4] = ["up", "down", "updown", "random"];

/// Kit editor knobs: field, label, range, step, format, unit, default.
pub const KIT_KNOBS: [(Field, Prop, &str, f64); 3] = [
    (Field::Gain, prop("gain", "Gain", GAIN_RANGE.0, GAIN_RANGE.1, 0.1, Format::Fixed1), "dB", 0.0),
    (
        Field::Transpose,
        prop("transpose", "Transp", TRANSPOSE_RANGE.0, TRANSPOSE_RANGE.1, 0.1, Format::SignedFixed1),
        "st",
        0.0,
    ),
    (Field::Decay, prop("decay", "Decay", DECAY_RANGE.0, DECAY_RANGE.1, 10.0, Format::Millis), "", DEFAULT_DECAY),
];

// ─── State ───────────────────────────────────────────────────────────────────

/// What the editors remember between frames: the tabs, the selected
/// setting, the filter type an off LED restores.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorState {
    pub tab: u8,
    pub fx_tab: u8,
    pub generation_tab: u8,
    /// Index in [`LFO_TARGETS`].
    pub lfo_target: u8,
    pub note_tab: u8,
    /// The Gen tab shows its Advanced settings.
    pub generation_advanced: bool,
    /// The row last clicked.
    pub selected_prop: Option<&'static str>,
    /// The filter type the fx LED turns back on.
    pub prev_filter_type: Option<String>,
}

// ─── Values of a track ───────────────────────────────────────────────────────

pub fn number(track: &Track, key: &str) -> f64 {
    track.field(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

fn flag(track: &Track, key: &str) -> bool {
    track.field(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// `String(track[key] ?? '')`, as a select compares it.
fn text_of(track: &Track, key: &str) -> String {
    match track.field(key) {
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => ordrumbox_core::js::number_to_string(n),
        _ => String::new(),
    }
}

fn has_lfo(track: &Track, key: &str) -> bool {
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

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// `pitchToNoteName`: the note at `pitch` semitones from C4 (plus the track's pitch).
fn pitch_to_note_name(pitch: f64, track_pitch: f64) -> String {
    let midi = 60.0 + track_pitch + pitch;
    let index = (midi % 12.0 + 12.0) % 12.0;
    // A fractional pitch has no name: the JS shows `undefined`.
    let name = if index.fract() == 0.0 { NOTE_NAMES[index as usize] } else { "undefined" };
    format!("{name}{}", js::number_to_string((midi / 12.0).floor() - 1.0))
}

/// `formatNoteTooltip`: a note's pitch and MIDI number, then what differs from the defaults.
pub fn note_tooltip(note: &Note, track_pitch: f64) -> String {
    let fmt = crate::control::fmt;
    let midi = 60.0 + track_pitch + note.pitch;
    let mut parts =
        vec![format!("{}  MIDI {}", pitch_to_note_name(note.pitch, track_pitch), js::number_to_string(midi))];
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
        parts.push(format!("arp:[{}]", js::to_string(&Value::Array(a.clone()))));
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
    if note.pan != 0.0 {
        parts.push(format!("pan:{}", fmt(note.pan)));
    }
    parts.join("  ")
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

// ─── Views ───────────────────────────────────────────────────────────────────

/// What the sample bar shows.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBar {
    pub info: String,
    pub envelope: Vec<f64>,
    pub marker: Option<f64>,
}

/// A select's options and the one shown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Choice {
    pub values: Vec<String>,
    pub labels: Vec<String>,
    pub selected: usize,
}

impl Choice {
    fn kind(&self) -> Kind {
        let labels = if self.labels.is_empty() { self.values.clone() } else { self.labels.clone() };
        Kind::Select { values: self.values.clone(), labels, selected: self.selected }
    }
}

/// What the track editor shows of a track, beyond its settings.
#[derive(Debug, Clone)]
pub struct TrackView<'a> {
    pub track: &'a Track,
    /// `Track: NAME - kit/sample`.
    pub title: String,
    pub sample: Option<SampleBar>,
    /// The sample's decay (`sound.decay ?? 0`).
    pub decay: f64,
    pub instruments: Choice,
    pub samples: Choice,
    pub synths: Choice,
    /// LFO values while playing, by setting.
    pub lfo: Vec<(&'static str, f64)>,
    /// Tempo the track plays at (the delay's beats may not fit at it).
    pub bpm: f64,
    /// A drum track: how it plays its pattern's style.
    pub compose: Option<ComposeView>,
}

/// What the Compose sub-tab shows of a drum track.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposeView {
    /// The pattern's style.
    pub style: String,
    /// The part the track plays in it, its settings with and without the
    /// pattern's macros, the hits a measure can hold.
    pub shown: ordrumbox_logic::compose::TrackCompose,
}

/// What the note editor shows.
#[derive(Debug, Clone)]
pub struct NoteView {
    /// `NAME [beat b step s]`, `— no note` when the step has none.
    pub title: String,
    /// The note (a default one when the track has none).
    pub note: Option<Note>,
    pub scales: Choice,
    pub arp_direction: Choice,
    pub arp_range: usize,
}

/// What the kit editor's controls show.
#[derive(Debug, Clone)]
pub struct KitControls {
    pub kits: Choice,
    pub instruments: Choice,
    pub gain: f64,
    pub transpose: f64,
    pub decay: f64,
}

// ─── Layout ──────────────────────────────────────────────────────────────────

fn row_h(scale: i32) -> i32 {
    16 * scale
}

/// Places rows top to bottom in a column.
struct Rows {
    x: i32,
    w: i32,
    y: i32,
    scale: i32,
    items: Vec<Item>,
}

impl Rows {
    fn row(&mut self) -> Rect {
        let r = Rect::new(self.x, self.y, self.w, row_h(self.scale));
        self.y += row_h(self.scale) + 2 * self.scale;
        r
    }

    fn label(&mut self, r: Rect, text: &str, key: Option<&'static str>, selected: bool) {
        let s = self.scale;
        if selected {
            self.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
        }
        let rect = Rect::new(r.x, r.y + (r.h - 8 * s) / 2, label_width(s) - 4 * s, 8 * s);
        self.items.push(Item::new(None, rect, Kind::Text { text: text.to_owned(), color: DIM }));
        if let Some(key) = key {
            self.items.push(Item::new(Some(Id::Row(key)), Rect::new(r.x, r.y, label_width(s), r.h), Kind::Highlight));
            // The row id goes under the label only: drop its highlight look.
            if let Some(last) = self.items.last_mut() {
                last.kind = Kind::Text { text: String::new(), color: DIM };
            }
        }
    }

    /// The control part of a row (after its label).
    fn field(&self, r: Rect, w: i32) -> Rect {
        let s = self.scale;
        let x = r.x + label_width(s);
        Rect::new(x, r.y + 2 * s, w.min(r.x + r.w - x), r.h - 4 * s)
    }

    fn tabs(&mut self, ids: &[(Id, Option<Id>, &str, bool, bool)]) {
        // (tab id, led id, label, active, led on)
        let s = self.scale;
        let r = Rect::new(self.x, self.y, self.w, 14 * s);
        let n = ids.len().max(1) as i32;
        for (i, (tab, led, label, active, on)) in ids.iter().enumerate() {
            let x = r.x + i as i32 * r.w / n;
            let w = (i as i32 + 1) * r.w / n - i as i32 * r.w / n;
            let led_w = if led.is_some() { 10 * s } else { 0 };
            if let Some(led) = led {
                self.items.push(Item::new(Some(*led), Rect::new(x, r.y, led_w, r.h), Kind::Led { on: *on }));
            }
            let kind = Kind::Tab { label: (*label).to_owned(), active: *active };
            self.items.push(Item::new(Some(*tab), Rect::new(x + led_w, r.y, w - led_w - s, r.h), kind));
        }
        self.y += 18 * s;
    }

    fn slider(&mut self, id: Id, spec: Spec, value: f64, label: &str, text: String, lfo: bool) {
        let r = self.row();
        let kind = Kind::Value { spec, value, label: Some(label.to_owned()), text, lfo };
        self.items.push(Item::new(Some(id), r, kind));
    }

    fn knob_bar(&mut self, knobs: Vec<(Id, Spec, f64, &str, String, bool)>, slots: i32) {
        let s = self.scale;
        let h = 48 * s;
        for (i, (id, spec, value, label, text, lfo)) in knobs.into_iter().enumerate() {
            let i = i as i32;
            let x = self.x + i * self.w / slots;
            let w = (i + 1) * self.w / slots - i * self.w / slots;
            let kind = Kind::Value { spec, value, label: Some(label.to_owned()), text, lfo };
            self.items.push(Item::new(Some(id), Rect::new(x, self.y, w, h), kind));
        }
        self.y += h + 4 * s;
    }

    fn text(&mut self, text: &str, color: u32) {
        let s = self.scale;
        let fitted: String = text.chars().take(usize::try_from(self.w / (8 * s)).unwrap_or(0)).collect();
        self.items.push(Item::text(self.x, self.y, fitted, color, s));
        self.y += 12 * s;
    }
}

fn value_spec(p: &Prop, default: f64, knob: bool) -> Spec {
    if knob { Spec::knob(p.min, p.max, p.step, default) } else { Spec::slider(p.min, p.max, p.step, default) }
}

/// Default of a setting (`TRACK_DEFAULTS[key] ?? min`).
fn default_of(key: &str, min: f64) -> f64 {
    Track::default().field(key).and_then(|v| v.as_f64()).unwrap_or(min)
}

/// The track editor in `area`; the y under it.
pub fn track_editor(view: &TrackView, state: &EditorState, area: Rect, scale: i32) -> (Vec<Item>, i32) {
    let s = scale;
    let track = view.track;
    let mut rows = Rows { x: area.x, w: area.w, y: area.y, scale: s, items: Vec::new() };
    rows.text(&view.title, ACCENT);

    // Sample bar: analysis, load button, envelope with the decay end.
    if let Some(bar) = &view.sample {
        let load_w = 40 * s;
        let info: String = bar.info.chars().take(usize::try_from((area.w - load_w) / (8 * s)).unwrap_or(0)).collect();
        rows.items.push(Item::text(area.x, rows.y + 2 * s, info, text_color(), s));
        let button = Kind::Button { label: "Load".into(), on: false };
        rows.items.push(Item::new(
            Some(Id::LoadSample),
            Rect::new(area.x + area.w - load_w, rows.y, load_w, 12 * s),
            button,
        ));
        rows.y += 15 * s;
        let kind = Kind::Envelope { points: bar.envelope.clone(), marker: bar.marker };
        rows.items.push(Item::new(None, Rect::new(area.x, rows.y, area.w, 32 * s), kind));
        rows.y += 36 * s;
    }

    // Knob bar: while playing, a setting with an LFO shows the LFO's value.
    let lfo_now = |key: &str| view.lfo.iter().find(|(k, _)| *k == key).map(|&(_, v)| v);
    let knobs = KNOB_BAR
        .iter()
        .map(|(p, unit)| {
            if p.key == "decay" {
                let spec = value_spec(p, DEFAULT_DECAY, true).log();
                (Id::Decay, spec, view.decay, p.label, p.format.value(view.decay, unit), false)
            } else {
                let value = lfo_now(p.key).unwrap_or_else(|| number(track, p.key));
                let spec = value_spec(p, default_of(p.key, p.min), true);
                (Id::KnobBar(p.key), spec, value, p.label, p.format.value(value, unit), has_lfo(track, p.key))
            }
        })
        .collect();
    rows.knob_bar(knobs, 4);

    let tabs: Vec<_> =
        TABS.iter().enumerate().map(|(i, l)| (Id::Tab(i as u8), None, *l, state.tab == i as u8, false)).collect();
    rows.tabs(&tabs);

    match state.tab {
        TAB_FX => fx_tab(&mut rows, view, state),
        TAB_SOUND => sound_tab(&mut rows, view),
        TAB_LFO => lfo_tab(&mut rows, view, state),
        TAB_LOOP => loop_tab(&mut rows, track),
        _ => generation_tab(&mut rows, track, view.compose.as_ref(), state),
    }
    let bottom = rows.y;
    (rows.items, bottom)
}

fn fx_tab(rows: &mut Rows, view: &TrackView, state: &EditorState) {
    let s = rows.scale;
    let track = view.track;
    let tabs: Vec<_> = FX_DEFS
        .iter()
        .enumerate()
        .map(|(i, fx)| {
            (Id::FxTab(i as u8), Some(Id::FxLed(i as u8)), fx.label, state.fx_tab == i as u8, fx_on(track, fx))
        })
        .collect();
    rows.tabs(&tabs);
    let fx = &FX_DEFS[usize::from(state.fx_tab.min(3))];
    for &key in fx.controls {
        // A slap is one echo: no feedback to set.
        if key == "delayFeedback" && track.delay_type == "slap" {
            continue;
        }
        let r = rows.row();
        if key == "filterType" {
            rows.label(r, "Type", Some(key), state.selected_prop == Some(key));
            for (i, (icon, value)) in FILTER_ICONS.iter().zip(FILTER_TYPES).enumerate() {
                let rect = Rect::new(r.x + label_width(s) + i as i32 * 30 * s, r.y + 2 * s, 26 * s, r.h - 4 * s);
                let kind = Kind::Button { label: (*icon).to_owned(), on: track.filter_type == value };
                rows.items.push(Item::new(Some(Id::FilterIcon(i as u8)), rect, kind));
            }
        } else if let Some(p) = FX_KNOBS.iter().find(|p| p.key == key) {
            rows.label(r, p.label, Some(key), state.selected_prop == Some(key));
            let value = view.lfo.iter().find(|(k, _)| *k == key).map_or_else(|| number(track, key), |&(_, v)| v);
            let mut spec = value_spec(p, default_of(key, p.min), true);
            if p.format == Format::Freq {
                spec = spec.log_full_travel();
            }
            let kind =
                Kind::Value { spec, value, label: None, text: p.format.value(value, ""), lfo: has_lfo(track, key) };
            rows.items.push(Item::new(Some(Id::Fx(key)), rows.field(r, 120 * s), kind));
        } else {
            rows.label(r, fx_label(key), Some(key), state.selected_prop == Some(key));
            let (values, mut labels) = track_select_options(key);
            let selected = index_of(&values, &text_of(track, key));
            if key == "delayBeats" {
                // Too long a delay plays at fewer beats: shown on the choice.
                let used = delay_beats_used(track.delay_beats, view.bpm);
                if used != track.delay_beats
                    && let Some(label) = labels.get_mut(selected)
                {
                    *label = format!("{label} ({} used)", js::number_to_string(used));
                }
            }
            let choice = Choice { values, labels, selected };
            rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * s), choice.kind()));
        }
    }
}

fn sound_tab(rows: &mut Rows, view: &TrackView) {
    let s = rows.scale;
    let track = view.track;
    let r = rows.row();
    rows.label(r, "Mono", None, false);
    rows.items.push(Item::new(Some(Id::Toggle("mono")), rows.field(r, 36 * s), Kind::Toggle { on: track.mono }));
    let r = rows.row();
    rows.items.push(Item::new(
        Some(Id::AutoAssign),
        Rect::new(r.x, r.y, 12 * s, r.h),
        Kind::Led { on: track.auto_assign },
    ));
    rows.items.push(Item::text(r.x + 14 * s, r.y + (r.h - 8 * s) / 2, "auto", DIM, s));
    for (id, label, choice) in [
        (Id::Instrument, "Instr", &view.instruments),
        (Id::Sample, "Sample", &view.samples),
        (Id::Synth, "Synth", &view.synths),
    ] {
        let r = rows.row();
        rows.label(r, label, None, false);
        rows.items.push(Item::new(Some(id), rows.field(r, r.w), choice.kind()));
    }
}

fn lfo_tab(rows: &mut Rows, view: &TrackView, state: &EditorState) {
    let s = rows.scale;
    let track = view.track;
    let target = &LFO_TARGETS[usize::from(state.lfo_target.min(4))];
    let lfo = track.field(target.lfo_key).and_then(|v| v.as_object().cloned());
    let get = |key: &str, fallback: f64| {
        lfo.as_ref().map_or(fallback, |o| o.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN))
    };
    let kind_now = lfo
        .as_ref()
        .map_or_else(|| "sine".to_owned(), |o| o.get("type").and_then(Value::as_str).unwrap_or("sine").to_owned());

    // Target: its on/off LED, then the list (a target with an LFO on says so).
    let r = rows.row();
    rows.label(r, "Target", None, false);
    let field = rows.field(r, r.w);
    let led_w = 10 * s;
    let on = |t: &LfoTarget| track.field(t.lfo_key).is_some_and(|v| !v.is_null());
    let led = Id::LfoLed(state.lfo_target.min(4));
    rows.items.push(Item::new(Some(led), Rect::new(field.x, field.y, led_w, field.h), Kind::Led { on: on(target) }));
    let values = LFO_TARGETS.iter().map(|t| t.key.to_owned()).collect();
    let labels =
        LFO_TARGETS.iter().map(|t| if on(t) { format!("{} (on)", t.name) } else { t.name.to_owned() }).collect();
    let choice = Choice { values, labels, selected: usize::from(state.lfo_target.min(4)) };
    let select = Rect::new(field.x + led_w + 2 * s, field.y, (200 * s).min(field.w - led_w - 2 * s), field.h);
    rows.items.push(Item::new(Some(Id::LfoTarget), select, choice.kind()));

    let r = rows.row();
    rows.label(r, "Type", None, false);
    let values: Vec<String> = WAVE_TYPES.iter().map(|w| (*w).to_owned()).collect();
    let selected = index_of(&values, &kind_now);
    rows.items.push(Item::new(
        Some(Id::LfoType),
        rows.field(r, 120 * s),
        Choice { values, labels: Vec::new(), selected }.kind(),
    ));

    let freq = get("freq", 1.0);
    rows.slider(
        Id::Lfo(LfoParam::Freq),
        Spec::slider(0.1, 2.0, 0.1, 1.0),
        freq,
        "Freq",
        crate::control::fmt(freq),
        false,
    );

    // Range: two sliders side by side, one value.
    let (min, max) = (get("min", target.min), get("max", target.max));
    let r = rows.row();
    let mut spec = Spec::slider(target.min, target.max, target.step, target.min);
    if target.key == "filterCutoff" {
        spec = spec.log();
    }
    let field = rows.field(r, r.w);
    let text = format!("{}..{}", crate::control::fmt(min), crate::control::fmt(max));
    let text_w = text_width(&text, s) + 4 * s;
    let half = ((field.w - text_w) / 2).max(16 * s);
    let rows_label = |rows: &mut Rows| rows.label(r, "Range", None, false);
    rows_label(rows);
    let value_kind = |value: f64| Kind::Value { spec, value, label: None, text: String::new(), lfo: false };
    rows.items.push(Item::new(
        Some(Id::Lfo(LfoParam::Min)),
        Rect::new(field.x - 4 * s, r.y, half, r.h),
        value_kind(min),
    ));
    rows.items.push(Item::new(
        Some(Id::Lfo(LfoParam::Max)),
        Rect::new(field.x - 4 * s + half, r.y, half, r.h),
        value_kind(max),
    ));
    rows.items.push(Item::text(field.x - 4 * s + 2 * half + 2 * s, r.y + (r.h - 8 * s) / 2, text, text_color(), s));

    let phase = get("phase", 0.0);
    rows.slider(
        Id::Lfo(LfoParam::Phase),
        Spec::slider(0.0, 1.0, 0.01, 0.0),
        phase,
        "Phase",
        crate::control::fmt(phase),
        false,
    );
}

fn loop_tab(rows: &mut Rows, track: &Track) {
    let spb = track.steps_per_beat;
    let max_steps = f64::from(track.beat_count * spb);
    for p in &LOOP_PROPS {
        let default = default_of(p.key, p.min);
        let (value, spec, format) = match p.key {
            "stepsPerBeat" => (f64::from(spb), Spec::slider(p.min, p.max, p.step, default), p.format),
            // The loop length's slider ends at the track's length.
            "loopLength" => (
                loop_length(track).min(max_steps),
                Spec::slider(p.min, max_steps, p.step, p.min),
                Format::LoopLength(spb),
            ),
            _ => (track.swing_amount, Spec::slider(p.min, p.max, p.step, default), p.format),
        };
        rows.slider(Id::Loop(p.key), spec, value, p.label, format.value(value, ""), false);
    }
}

fn generation_tab(rows: &mut Rows, track: &Track, compose: Option<&ComposeView>, state: &EditorState) {
    let s = rows.scale;
    // The simple view: auto, lock, a new line, three settings.
    let toggle = |rows: &mut Rows, key: &'static str, label: &str, on: bool| {
        let r = rows.row();
        rows.label(r, label, Some(key), state.selected_prop == Some(key));
        rows.items.push(Item::new(Some(Id::Toggle(key)), rows.field(r, 36 * s), Kind::Toggle { on }));
    };
    toggle(rows, "autoMode", "Auto", track.auto_mode);
    toggle(rows, "composeLock", "Lock", track.compose_lock);
    let r = rows.row();
    rows.label(r, "New line", None, false);
    let button = Kind::Button { label: "\u{21bb}".into(), on: false };
    rows.items.push(Item::new(Some(Id::Regenerate), rows.field(r, 36 * s), button));
    let melodic = instrument_family(&track.name).is_melodic();
    match compose {
        Some(view) => {
            rows.text(&format!("{} / {}  (* = style)", view.style, view.shown.part), DIM);
            for p in GENERATION_COMPOSE.iter().filter(|p| SIMPLE_COMPOSE.contains(&p.key)) {
                compose_slider(rows, track, view, p, state);
            }
        }
        None if melodic => {
            variant_select(rows, track, state);
            generation_slider(rows, track, &GENERATION_DENSITY, state);
        }
        None => rows.text("no part for this track in the pattern's style", DIM),
    }
    // Everything else, folded.
    let r = rows.row();
    let label = if state.generation_advanced { "Advanced \u{25be}" } else { "Advanced \u{25b8}" };
    let button = Kind::Button { label: label.into(), on: state.generation_advanced };
    rows.items.push(Item::new(Some(Id::GenerationAdvanced), Rect::new(r.x, r.y + 2 * s, 120 * s, r.h - 4 * s), button));
    if !state.generation_advanced {
        return;
    }
    let compose_tab = compose.is_some() && state.generation_tab == COMPOSE_TAB;
    let mut tabs = if track.auto_mode {
        vec![
            (Id::GenerationTab(0), None, "Groove", !compose_tab && state.generation_tab == 0, false),
            (Id::GenerationTab(1), None, "Engine", !compose_tab && state.generation_tab == 1, false),
        ]
    } else {
        vec![(Id::GenerationTab(0), None, "Manual", !compose_tab, false)]
    };
    if compose.is_some() {
        tabs.push((Id::GenerationTab(COMPOSE_TAB), None, "Compose", compose_tab, false));
    }
    rows.tabs(&tabs);
    if let (true, Some(view)) = (compose_tab, compose) {
        for p in &GENERATION_COMPOSE {
            compose_slider(rows, track, view, p, state);
        }
        return;
    }
    if !track.auto_mode {
        for p in &GENERATION_MANUAL {
            generation_slider(rows, track, p, state);
        }
        return;
    }
    for row in if state.generation_tab == 0 { &GENERATION_GROOVE } else { &GENERATION_ENGINE } {
        match row {
            // A drum line comes from its style (Compose); a melodic one has them above.
            GenerationRow::Select("autoVariant", _) | GenerationRow::Slider(Prop { key: "autoDensity", .. }) => {}
            GenerationRow::Slider(p) => generation_slider(rows, track, p, state),
            GenerationRow::Toggle(key, label) => toggle(rows, key, label, flag(track, key)),
            GenerationRow::Select(key, label) => {
                let r = rows.row();
                rows.label(r, label, Some(key), state.selected_prop == Some(key));
                let values = track_select_options(key).0;
                let selected = index_of(&values, &text_of(track, key));
                let choice = Choice { values, labels: Vec::new(), selected };
                rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * s), choice.kind()));
            }
        }
    }
}

/// The compose settings of the simple view.
const SIMPLE_COMPOSE: [&str; 3] = ["composeHitsPerMeasure", "composeComplexity", "composeVariation"];

/// A melodic track's density (the simple view).
const GENERATION_DENSITY: Prop = prop("autoDensity", "Density", -1.0, 1.0, 0.01, Format::Density);

fn generation_slider(rows: &mut Rows, track: &Track, p: &Prop, state: &EditorState) {
    let s = rows.scale;
    let value = match p.key {
        // Unset until the generator picks them: the amounts used meanwhile.
        "autoHitVariation" => track.auto_hit_amount(),
        "autoNoteVariation" => track.auto_note_amount(),
        key => number(track, key),
    };
    let spec = value_spec(p, default_of(p.key, p.min), false);
    let r = rows.row();
    if state.selected_prop == Some(p.key) {
        rows.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
    }
    let kind =
        Kind::Value { spec, value, label: Some(p.label.to_owned()), text: p.format.value(value, ""), lfo: false };
    rows.items.push(Item::new(Some(Id::Generation(p.key)), r, kind));
}

/// The variant a melodic track plays (`""`: its style's).
fn variant_select(rows: &mut Rows, track: &Track, state: &EditorState) {
    let key = "autoVariant";
    let r = rows.row();
    rows.label(r, "Variant", Some(key), state.selected_prop == Some(key));
    let values = variant_options(track);
    let selected = index_of(&values, &text_of(track, key));
    let choice = Choice { values, labels: Vec::new(), selected };
    rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * rows.scale), choice.kind()));
}

/// The variants a melodic track can play (`""`: its style's).
pub fn variant_options(track: &Track) -> Vec<String> {
    let kind = if instrument_family(&track.name) == InstrumentFamily::Bass { "bass" } else { "melody" };
    std::iter::once(String::new()).chain(generate::variant_names(kind).into_iter().map(str::to_owned)).collect()
}

/// A setting of `settings` by its key.
fn compose_value(key: &str, settings: &ordrumbox_logic::compose::Settings) -> f64 {
    match key {
        "composeHitsPerMeasure" => settings.hits_per_measure,
        "composeComplexity" => settings.complexity,
        "composeSyncopation" => settings.syncopation,
        "composeCoupling" => settings.coupling,
        "composeVariation" => settings.variation,
        "composeStyleWeight" => settings.style_weight,
        _ => 0.0,
    }
}

/// One compose slider: the track's value, else the style's (its text ends
/// in `*`; a double-click gives a value back to the style); `a→b` when the
/// pattern's macros make it play `b`; the hits show the most a measure holds
/// when asked for more.
fn compose_slider(rows: &mut Rows, track: &Track, view: &ComposeView, p: &Prop, state: &EditorState) {
    let s = rows.scale;
    let own_key = matches!(p.key, "composeStyleWeight" | "composeSeed");
    let own = track.field(p.key).and_then(|v| v.as_f64());
    let value =
        if own_key { number(track, p.key) } else { own.unwrap_or_else(|| compose_value(p.key, &view.shown.base)) };
    let mut text = p.format.value(value, "");
    if !own_key {
        let used = compose_value(p.key, &view.shown.used);
        let used = if p.key == "composeHitsPerMeasure" { used.min(view.shown.open_hits as f64) } else { used };
        if (used - value).abs() >= 0.5 {
            text = format!("{text}\u{2192}{}", js::number_to_string(used.round()));
        }
        if own.is_none() {
            text.push('*');
        }
    }
    let default = if own_key { default_of(p.key, p.min) } else { STYLE_DEFAULT };
    let r = rows.row();
    if state.selected_prop == Some(p.key) {
        rows.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
    }
    let spec = Spec::slider(p.min, p.max, p.step, default);
    let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text, lfo: false };
    rows.items.push(Item::new(Some(Id::Compose(p.key)), r, kind));
}

/// `NOTE_DEFAULTS[key]`, what a double-click restores.
fn note_default(key: &str, min: f64) -> f64 {
    Note::default().field(key).and_then(|v| v.as_f64()).unwrap_or(min)
}

/// The note editor in `area`.
pub fn note_editor(view: &NoteView, state: &EditorState, area: Rect, scale: i32) -> Vec<Item> {
    let s = scale;
    let mut rows = Rows { x: area.x, w: area.w, y: area.y, scale: s, items: Vec::new() };
    rows.items.push(Item::new(None, Rect::new(area.x, area.y, area.w, s), Kind::Line));
    rows.y += 6 * s;
    rows.text(&view.title, ACCENT);
    let Some(note) = &view.note else { return rows.items };
    let knobs = NOTE_KNOBS
        .iter()
        .map(|(p, unit)| {
            let value = note.field(p.key).and_then(|v| v.as_f64()).unwrap_or(p.min);
            let spec = value_spec(p, note_default(p.key, p.min), true);
            (Id::NoteKnob(p.key), spec, value, p.label, p.format.value(value, unit), false)
        })
        .collect();
    rows.knob_bar(knobs, 4);
    let tabs: Vec<_> = NOTE_TABS
        .iter()
        .enumerate()
        .map(|(i, l)| (Id::NoteTab(i as u8), None, *l, state.note_tab == i as u8, false))
        .collect();
    rows.tabs(&tabs);
    for row in NOTE_GROUPS[usize::from(state.note_tab.min(3))] {
        match row {
            NoteRow::Slider(p) => {
                let value = if p.key == "arpRange" {
                    view.arp_range as f64
                } else {
                    note.field(p.key).and_then(|v| v.as_f64()).unwrap_or(p.min)
                };
                let spec = value_spec(p, note_default(p.key, p.min), false);
                rows.slider(Id::NoteSlider(p.key), spec, value, p.label, p.format.value(value, ""), false);
            }
            NoteRow::Select(key, label) => {
                let r = rows.row();
                rows.label(r, label, None, false);
                let choice = if *key == "arpScale" { &view.scales } else { &view.arp_direction };
                rows.items.push(Item::new(Some(Id::NoteSelect(key)), rows.field(r, 140 * s), choice.kind()));
            }
        }
    }
    rows.items
}

/// The kit editor's selects and knobs at `area`; the y under them.
pub fn kit_controls(view: &KitControls, area: Rect, scale: i32) -> (Vec<Item>, i32) {
    let s = scale;
    let mut rows = Rows { x: area.x, w: area.w, y: area.y, scale: s, items: Vec::new() };
    for (id, label, choice) in [(Id::KitKit, "Kit:", &view.kits), (Id::KitInstrument, "Instrument:", &view.instruments)]
    {
        let r = rows.row();
        rows.label(r, label, None, false);
        rows.items.push(Item::new(Some(id), rows.field(r, 160 * s), choice.kind()));
    }
    rows.y += 4 * s;
    let knobs = KIT_KNOBS
        .iter()
        .map(|(field, p, unit, default)| {
            let value = match field {
                Field::Gain => view.gain,
                Field::Transpose => view.transpose,
                _ => view.decay,
            };
            let mut spec = value_spec(p, *default, true);
            if *field == Field::Decay {
                spec = spec.log();
            }
            (Id::KitKnob(*field), spec, value, p.label, p.format.value(value, unit), false)
        })
        .collect();
    rows.knob_bar(knobs, 4);
    let bottom = rows.y;
    (rows.items, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    fn scales() -> ScaleList {
        ScaleList::from_json(
            &json::parse(
                r#"{"major":{"scaleSteps":[0,2,4,5,7,9,11]},"minor":{"scaleSteps":[0,2,3,5,7,8,10]},"odd":{}}"#,
            )
            .unwrap(),
        )
    }

    #[test]
    fn arpeggios_round_trip() {
        let scales = scales();
        assert_eq!(scales.names, ["major", "minor", "odd"]);
        assert_eq!(scales.intervals("minor", 9), [0, 2, 3, 5, 7, 8, 10, 12, 14]);
        assert_eq!(scales.intervals("odd", 3), [0]);
        let mut note = Note::default();
        assert_eq!(arp_state(&note, &scales), ArpState { scale: "major".into(), direction: "up".into(), range: 0 });
        note.arp_direction = Some("down".into());
        note.arp = arp_value(&note, &scales, Some("minor"), None, Some(4));
        assert_eq!(note.arp.to_json(), r#"{"intervals":[0,2,3,5],"direction":"down"}"#);
        assert_eq!(arp_state(&note, &scales), ArpState { scale: "minor".into(), direction: "down".into(), range: 4 });
        assert_eq!(arp_value(&note, &scales, None, None, Some(0)), Value::Null);
    }

    /// `formatNoteTooltip` (tools/export_control_fixtures.mjs).
    #[test]
    fn golden_tooltips() {
        let fixture = json::parse(include_str!("../tests/fixtures/controls.json")).unwrap();
        let rows = fixture.get("tooltips").and_then(Value::as_array).unwrap();
        assert!(rows.len() > 20);
        for row in rows {
            let r = row.as_array().unwrap();
            let mut note = Note::default();
            for (key, value) in r[0].as_object().unwrap().iter() {
                let _ = note.set_field(key, value);
            }
            assert_eq!(note_tooltip(&note, r[1].as_f64().unwrap()), r[2].as_str().unwrap(), "{r:?}");
        }
    }

    #[test]
    fn locale_order() {
        let mut names = vec!["b", "B", "a10", "a2", "_x", "A"];
        names.sort_by(|a, b| locale_cmp(a, b));
        assert_eq!(names, ["_x", "A", "a10", "a2", "b", "B"]);
    }

    #[test]
    fn loop_point_falls_back_to_the_length() {
        let mut track = Track { beat_count: 4, steps_per_beat: 4, ..Track::default() };
        assert_eq!(loop_length(&track), 16.0);
        track.notes.push(Note { beat: 7, step: 1, ..Note::default() });
        assert_eq!(loop_length(&track), 30.0);
        track.loop_length = Some(5);
        assert_eq!(loop_length(&track), 5.0);
    }

    fn settings(hits: f64) -> ordrumbox_logic::compose::Settings {
        ordrumbox_logic::compose::Settings {
            hits_per_measure: hits,
            complexity: 5.0,
            // As the test track sets it.
            syncopation: 70.0,
            coupling: 0.0,
            variation: 5.0,
            style_weight: 100.0,
        }
    }

    fn view(track: &Track) -> TrackView<'_> {
        TrackView {
            track,
            title: "Track: KICK - real/kick".into(),
            sample: Some(SampleBar { info: "C4 - 210 ms - -0.3 dB".into(), envelope: vec![0.5; 8], marker: Some(0.4) }),
            decay: 0.0,
            instruments: Choice { values: vec!["KICK".into()], labels: Vec::new(), selected: 0 },
            samples: Choice::default(),
            synths: Choice { values: vec!["none".into()], labels: Vec::new(), selected: 0 },
            lfo: Vec::new(),
            bpm: 120.0,
            compose: Some(ComposeView {
                style: "house".into(),
                shown: ordrumbox_logic::compose::TrackCompose {
                    part: "KICK".into(),
                    used: settings(6.0),
                    base: settings(4.0),
                    open_hits: 16,
                },
            }),
        }
    }

    #[test]
    fn every_tab_builds() {
        let mut track = Track { name: "KICK".into(), ..Track::default() };
        for auto in [false, true] {
            track.auto_mode = auto;
            for tab in 0..5 {
                for sub in 0..6 {
                    let state = EditorState {
                        tab,
                        fx_tab: sub % 4,
                        generation_tab: sub % 3,
                        generation_advanced: sub >= 3,
                        lfo_target: sub,
                        ..EditorState::default()
                    };
                    let (items, bottom) = track_editor(&view(&track), &state, Rect::new(700, 30, 360, 600), 1);
                    assert!(bottom > 30 && items.iter().any(|i| i.id == Some(Id::Decay)));
                }
            }
        }
        // Folded: the simple view only.
        let state = EditorState { tab: 4, ..EditorState::default() };
        let (items, _) = track_editor(&view(&track), &state, Rect::new(0, 0, 360, 600), 1);
        assert!(items.iter().any(|i| i.id == Some(Id::Toggle("composeLock"))));
        assert!(items.iter().any(|i| i.id == Some(Id::GenerationAdvanced)));
        assert!(!items.iter().any(|i| i.id == Some(Id::Generation("autoPitchChance"))));
        let state = EditorState { generation_advanced: true, ..state };
        let (items, _) = track_editor(&view(&track), &state, Rect::new(0, 0, 360, 600), 1);
        assert!(items.iter().any(|i| i.id == Some(Id::Generation("autoPitchChance"))));
        assert!(!items.iter().any(|i| i.id == Some(Id::Generation("hitVariation"))));
    }

    #[test]
    fn the_engine_tab_shows_the_amounts_and_the_variants_a_track_can_use() {
        let ids = |track: &Track, tab: u8| {
            let state =
                EditorState { tab: 4, generation_tab: tab, generation_advanced: true, ..EditorState::default() };
            let mut view = view(track);
            if instrument_family(&track.name).is_melodic() {
                view.compose = None;
            }
            track_editor(&view, &state, Rect::new(0, 0, 360, 600), 1).0
        };
        let kick = Track { name: "KICK".into(), auto_mode: true, auto_hit_variation: Some(20.0), ..Track::default() };
        let groove = ids(&kick, 0);
        let hits = groove.iter().find(|i| i.id == Some(Id::Generation("autoHitVariation"))).unwrap();
        assert!(matches!(&hits.kind, Kind::Value { value, .. } if *value == 20.0));
        // A drum line: no variant, no density; the note amount unset shows the one used.
        let engine = ids(&kick, 1);
        assert!(!engine.iter().any(|i| i.id == Some(Id::TrackSelect("autoVariant"))));
        assert!(!engine.iter().any(|i| i.id == Some(Id::Generation("autoDensity"))));
        let notes = engine.iter().find(|i| i.id == Some(Id::Generation("autoNoteVariation"))).unwrap();
        assert!(matches!(&notes.kind, Kind::Value { value, .. } if *value == kick.auto_note_amount()));
        // A bass line: its own variants, in the simple view.
        let bass = Track { name: "BASS".into(), auto_mode: true, ..Track::default() };
        assert!(ids(&bass, 1).iter().any(|i| i.id == Some(Id::TrackSelect("autoVariant"))));
        assert!(ids(&bass, 1).iter().any(|i| i.id == Some(Id::Generation("autoDensity"))));
        let variants = variant_options(&bass);
        assert!(variants.contains(&"groove".to_owned()) && !variants.contains(&"chordStab".to_owned()));
        assert!(variant_options(&Track { name: "PIANO".into(), ..Track::default() }).contains(&"chordStab".to_owned()));
    }

    #[test]
    fn the_compose_tab_shows_the_style_values_until_set() {
        let mut track = Track { name: "KICK".into(), ..Track::default() };
        track.compose_syncopation = Some(70.0);
        let state =
            EditorState { tab: 4, generation_tab: COMPOSE_TAB, generation_advanced: true, ..EditorState::default() };
        let (items, _) = track_editor(&view(&track), &state, Rect::new(0, 0, 360, 600), 1);
        let shown = |key: &'static str| {
            items.iter().find(|i| i.id == Some(Id::Compose(key))).and_then(|i| match &i.kind {
                Kind::Value { value, text, spec, .. } => Some((*value, text.clone(), spec.default)),
                _ => None,
            })
        };
        // The pattern's energy makes the 4 hits of the style 6.
        assert_eq!(shown("composeHitsPerMeasure"), Some((4.0, "4\u{2192}6*".to_owned(), STYLE_DEFAULT)));
        assert_eq!(shown("composeSyncopation"), Some((70.0, "70".to_owned(), STYLE_DEFAULT)));
        assert_eq!(shown("composeSeed"), Some((0.0, "new".to_owned(), 0.0)));
        assert!(items.iter().any(|i| i.id == Some(Id::Regenerate)));
        // A melodic track has no Compose tab: the manual sliders show.
        let mut bass = view(&track);
        bass.compose = None;
        let (items, _) = track_editor(&bass, &state, Rect::new(0, 0, 360, 600), 1);
        assert!(items.iter().any(|i| i.id == Some(Id::Generation("hitVariation"))));
    }

    #[test]
    fn note_editor_hides_the_euclidean_prob() {
        let view = NoteView {
            title: "KICK [beat 1 step 1]".into(),
            note: Some(Note::default()),
            scales: Choice::default(),
            arp_direction: Choice::default(),
            arp_range: 0,
        };
        let state = EditorState { note_tab: 2, ..EditorState::default() };
        let items = note_editor(&view, &state, Rect::new(0, 0, 360, 300), 1);
        let sliders: Vec<_> = items
            .iter()
            .filter_map(|i| match i.id {
                Some(Id::NoteSlider(k)) => Some(k),
                _ => None,
            })
            .collect();
        assert_eq!(sliders, ["euclideanPulses", "euclideanRotation"]);
    }
}
