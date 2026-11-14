//! The tables the editors read: tabs, effects, settings, ranges and labels.

use super::layout::note_default;
use crate::control::Format;
use crate::kits::{ATTACK_RANGE, DECAY_RANGE, Field, GAIN_RANGE, HOLD_RANGE, TRANSPOSE_RANGE};
use ordrumbox_core::Note;
use ordrumbox_core::note::{MAX_LENGTH_STEPS, MAX_SHIFT_STEPS};
use ordrumbox_core::track::{
    COMP_ATTACK_MS_RANGE, COMP_RATIO_RANGE, COMP_RELEASE_MS_RANGE, COMP_THRESHOLD_DB_RANGE, DUCK_RELEASE_MS_RANGE,
    MAX_STEPS_PER_BEAT,
};
use ordrumbox_logic::generate;

/// Main tabs.
pub const TABS: [&str; 5] = ["FX", "Sound", "LFO", "Loop", "Generate"];
/// The index of the Generate tab in [`TABS`].
pub const GEN_TAB: u8 = 4;
pub const TAB_FX: u8 = 0;
pub const TAB_SOUND: u8 = 1;
pub const TAB_LFO: u8 = 2;
pub const TAB_LOOP: u8 = 3;

/// An effect: the setting its on / off button reads and writes, its name,
/// its controls, and the value its button turns it on at.
pub struct FxDef {
    pub key: &'static str,
    pub label: &'static str,
    pub controls: &'static [&'static str],
    pub on_value: f64,
}

pub const FX_DEFS: [FxDef; 6] = [
    FxDef { key: "reverbMix", label: "Reverb", controls: &["reverbMix", "reverbType"], on_value: 0.5 },
    FxDef {
        key: "delayMix",
        label: "Delay",
        controls: &["delayMix", "delayBeats", "delayFeedback", "delayType"],
        on_value: 0.5,
    },
    FxDef {
        key: "saturationMix",
        label: "Saturation",
        controls: &["saturationMix", "saturationType", "bitcrush", "hardClip"],
        on_value: 0.5,
    },
    // On at full mix: a plain compressor; less mixes it in parallel.
    FxDef {
        key: "compMix",
        label: "Compressor",
        controls: &["compMix", "compThresholdDb", "compRatio", "compAttackMs", "compReleaseMs"],
        on_value: 1.0,
    },
    // Another track's sound ducks this one's volume and keys its compressor.
    FxDef {
        key: "duckAmount",
        label: "Sidechain",
        controls: &["duckAmount", "duckReleaseMs", "sidechainSource"],
        on_value: 0.5,
    },
    // The filter is on when its type is not the allpass bypass.
    FxDef {
        key: "filterType",
        label: "Filter",
        controls: &["filterType", "filterSlope", "filterCutoff", "filterQ"],
        on_value: 0.0,
    },
];

pub use ordrumbox_engine::synth_preset::FILTER_TYPES;
/// The filter types as their buttons are labelled (the font has no curves).
pub const FILTER_ICONS: [&str; 4] = ["LP", "HP", "BP", "NT"];
/// Cutoff a filter gets when switched on from its 20 Hz default, Hz (a
/// lowpass at 20 Hz is silence).
pub const FILTER_ON_CUTOFF: f64 = 1_000.0;

/// Delay time multipliers, in beats, and their labels (`fx_values.js`).
pub(super) const DELAY_TIME_VALUES: [&str; 7] = ["0.0625", "0.125", "0.25", "0.5", "1", "2", "4"];
pub(super) const DELAY_TIME_LABELS: [&str; 7] = ["1/16", "1/8", "1/4", "1/2", "1", "2", "4"];

/// Filter slopes, dB per octave, and their labels.
pub(super) const FILTER_SLOPE_VALUES: [&str; 2] = ["12", "24"];
pub(super) const FILTER_SLOPE_LABELS: [&str; 2] = ["12 dB", "24 dB"];

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

/// A target of the modulation tab: the setting, its label, its LFO's key,
/// its range and step.
pub struct LfoTarget {
    pub key: &'static str,
    pub label: &'static str,
    pub lfo_key: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// The settings that take an LFO, in the JS order (track props, then the knob bar's).
pub const LFO_TARGETS: [LfoTarget; 5] = [
    LfoTarget { key: "filterCutoff", label: "Cutoff", lfo_key: "filterCutoffLfo", min: 20.0, max: 20_000.0, step: 1.0 },
    LfoTarget { key: "filterQ", label: "Q", lfo_key: "filterQLfo", min: 0.707, max: 18.707, step: 0.01 },
    LfoTarget { key: "volume", label: "Volume", lfo_key: "volumeLfo", min: 0.0, max: 1.0, step: 0.01 },
    LfoTarget { key: "pan", label: "Pan", lfo_key: "panLfo", min: -1.0, max: 1.0, step: 0.01 },
    LfoTarget { key: "transpose", label: "Transpose", lfo_key: "transposeLfo", min: -24.0, max: 24.0, step: 1.0 },
];

/// The LFO key of a setting, if it takes one.
pub fn lfo_key(key: &str) -> Option<&'static str> {
    LFO_TARGETS.iter().find(|t| t.key == key).map(|t| t.lfo_key)
}

/// A value setting of the track editor: key, label, range, step, format,
/// and whether it is a count (a whole number of things: a stepper in a row).
#[derive(Debug, Clone, Copy)]
pub struct Prop {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub format: Format,
    pub count: bool,
}

pub(super) const fn prop(
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    step: f64,
    format: Format,
) -> Prop {
    Prop { key, label, min, max, step, format, count: false }
}

/// A count, one by one: shown as a stepper in a row.
pub(super) const fn count(key: &'static str, label: &'static str, min: f64, max: f64, format: Format) -> Prop {
    Prop { key, label, min, max, step: 1.0, format, count: true }
}

/// The effect knobs.
pub const FX_KNOBS: [Prop; 15] = [
    prop("reverbMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("delayMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("delayFeedback", "Feedback", 0.0, 0.95, 0.01, Format::Plain),
    prop("saturationMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("bitcrush", "Crush", 0.0, 1.0, 0.01, Format::Plain),
    prop("hardClip", "Clip", 0.0, 1.0, 0.01, Format::Plain),
    prop("compMix", "Mix", 0.0, 1.0, 0.01, Format::Plain),
    prop("compThresholdDb", "Thresh", COMP_THRESHOLD_DB_RANGE.0, COMP_THRESHOLD_DB_RANGE.1, 0.5, Format::Decibels),
    prop("compRatio", "Ratio", COMP_RATIO_RANGE.0, COMP_RATIO_RANGE.1, 0.1, Format::Ratio),
    prop("compAttackMs", "Attack", COMP_ATTACK_MS_RANGE.0, COMP_ATTACK_MS_RANGE.1, 0.1, Format::FineMillis),
    prop("compReleaseMs", "Release", COMP_RELEASE_MS_RANGE.0, COMP_RELEASE_MS_RANGE.1, 1.0, Format::FineMillis),
    prop("duckAmount", "Duck", 0.0, 1.0, 0.01, Format::Plain),
    prop("duckReleaseMs", "Release", DUCK_RELEASE_MS_RANGE.0, DUCK_RELEASE_MS_RANGE.1, 1.0, Format::FineMillis),
    prop("filterCutoff", "Cutoff", 20.0, 20_000.0, 1.0, Format::Freq),
    prop("filterQ", "Q", 0.707, 18.707, 0.01, Format::Fixed2),
];

/// Labels of the effect rows that are not knobs.
pub fn fx_label(key: &str) -> &'static str {
    match key {
        "delayBeats" => "Beats",
        "filterSlope" => "Slope",
        "sidechainSource" => "Key",
        _ => "Type",
    }
}

/// The knob bar (volume, pan, pitch, decay) and its units.
pub const KNOB_BAR: [(Prop, &str); 4] = [
    (prop("volume", "Volume", 0.0, 1.0, 0.01, Format::Percent), "%"),
    (prop("pan", "Pan", -1.0, 1.0, 0.01, Format::Fmt), ""),
    (prop("transpose", "Transpose", -24.0, 24.0, 1.0, Format::SignedPlain), "st"),
    (prop("decay", "Decay", DECAY_RANGE.0, DECAY_RANGE.1, 10.0, Format::Millis), ""),
];

/// Decay a double-click restores (the knob bar's default).
pub const DEFAULT_DECAY: f64 = 500.0;

/// Generation tab, shown while auto is off.
pub const GENERATION_MANUAL: [Prop; 3] = [
    prop("hitVariation", "Hit Variation", 0.0, 100.0, 1.0, Format::Chance),
    prop("noteVariation", "Note Variation", 0.0, 100.0, 1.0, Format::Chance),
    prop("probability", "Probability", 0.0, 1.0, 0.01, Format::Percent),
];

/// A row of the Groove / Engine sub-tabs.
#[derive(Debug, Clone, Copy)]
pub enum GenerationRow {
    Slider(Prop),
    Toggle(&'static str, &'static str),
    Select(&'static str, &'static str),
}

pub const GENERATION_GROOVE: [GenerationRow; 8] = [
    GenerationRow::Slider(prop("autoHitVariation", "Hit Variation", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoPitchChance", "Pitch", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoVelocityChance", "Velocity", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoSilenceChance", "Silence", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoFillChance", "Fill", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoGhostChance", "Ghost", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(count("pitchRange", "Pitch Range", 1.0, 24.0, Format::Plain)),
    GenerationRow::Toggle("pitchScaleLock", "Scale Lock"),
];

pub const GENERATION_ENGINE: [GenerationRow; 8] = [
    GenerationRow::Slider(prop("autoNoteVariation", "Note Variation", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoRetriggerChance", "Retrigger", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoEuclideanChance", "Euclidean", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoNoteProbabilityChance", "Probability", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoNoteProbabilityRange", "Depth", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Slider(prop("autoArpChance", "Arpeggio", 0.0, 100.0, 1.0, Format::Chance)),
    GenerationRow::Select("autoVariant", "Variant"),
    GenerationRow::Slider(prop("autoDensity", "Density", -1.0, 1.0, 0.01, Format::Density)),
];

/// The Compose sub-tab: how a drum track plays its style's part.
pub const GENERATION_COMPOSE: [Prop; 7] = [
    count("composeHitsPerMeasure", "Measure Hits", 0.0, 32.0, Format::Plain),
    prop("composeComplexity", "Complexity", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeSyncopation", "Syncopation", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeCoupling", "Coupling", -100.0, 100.0, 1.0, Format::Plain),
    prop("composeVariation", "Variation", 0.0, 100.0, 1.0, Format::Plain),
    prop("composeStyleWeight", "Style Weight", 0.0, 100.0, 1.0, Format::Plain),
    count("composeSeed", "Seed", 0.0, 9999.0, Format::Seed),
];

/// What a double-click on a compose slider sets: the setting given back to
/// the style (`None`). Below every range.
pub const STYLE_DEFAULT: f64 = -1.0e9;

/// The sub-tab index of Compose.
pub const COMPOSE_TAB: u8 = 2;

/// The loop tab.
pub const LOOP_PROPS: [Prop; 3] = [
    count("stepsPerBeat", "Steps per Beat", 1.0, MAX_STEPS_PER_BEAT as f64, Format::Plain),
    count("loopLength", "Loop Length", 1.0, 128.0, Format::Plain),
    prop("swingAmount", "Swing", 0.0, 1.0, 0.01, Format::Plain),
];

/// Note editor tabs.
pub const NOTE_TABS: [&str; 5] = ["Trigger", "Retrigger", "Euclidean", "Arpeggio", "Timing"];

/// The note editor's knobs and their units.
pub const NOTE_KNOBS: [(Prop, &str); 4] = [
    (prop("velocity", "Velocity", 0.0, 1.0, 0.01, Format::Percent), "%"),
    (prop("pitch", "Pitch", -24.0, 24.0, 1.0, Format::SignedPlain), "st"),
    (prop("pan", "Pan", -1.0, 1.0, 0.01, Format::Fmt), ""),
    // In steps of the track: longer notes are drawn out in the piano roll.
    (prop("lengthSteps", "Length", MIN_LENGTH_SLIDER, MAX_LENGTH_SLIDER, 0.25, Format::Fmt), ""),
];

/// A row of a note editor tab.
#[derive(Debug, Clone, Copy)]
pub enum NoteRow {
    Slider(Prop),
    Select(&'static str, &'static str),
}

/// A note's chance, 0–1, shown as a percentage.
pub(super) const fn note_chance(key: &'static str, label: &'static str) -> NoteRow {
    NoteRow::Slider(prop(key, label, 0.0, 1.0, 0.01, Format::Percent))
}

pub(super) const fn note_count(key: &'static str, label: &'static str, min: f64, max: f64) -> NoteRow {
    NoteRow::Slider(count(key, label, min, max, Format::Fmt))
}

/// The rows of each note tab. As in the JS, the Euclidean tab's "Prob"
/// (the same setting as the Retrig tab's) never shows: both sliders share
/// one key, and the second one takes the first one's place.
pub const NOTE_GROUPS: [&[NoteRow]; NOTE_TABS.len()] = [
    &[
        note_count("cycle", "Cycle", 1.0, 16.0),
        note_count("cycleOffset", "Offset", 0.0, 15.0),
        note_chance("probability", "Probability"),
    ],
    &[
        note_count("retriggerCount", "Retrigger", 1.0, 16.0),
        note_count("retriggerRate", "Rate", 1.0, 16.0),
        note_chance("retriggerProbability", "Probability"),
        NoteRow::Slider(prop("retriggerFade", "Fade", 0.0, 1.0, 0.01, Format::Percent)),
    ],
    &[note_count("euclideanPulses", "Pulses", 0.0, 16.0), note_count("euclideanRotation", "Rotation", 0.0, 15.0)],
    &[
        NoteRow::Select("arpScale", "Scale"),
        NoteRow::Select("arpDirection", "Direction"),
        note_count("arpRange", "Range", 0.0, 12.0),
    ],
    &[NoteRow::Slider(prop("shiftSteps", "Shift", -MAX_SHIFT_STEPS, MAX_SHIFT_STEPS, 0.01, Format::Percent))],
];

/// The shortest and the longest note the note editor's slider and the lane
/// set, in steps; the piano roll draws a note out further.
pub const MIN_LENGTH_SLIDER: f64 = 0.25;
pub const MAX_LENGTH_SLIDER: f64 = 16.0;

/// The range of a note setting. Not the pitch: the knob spans two octaves,
/// the piano roll more. The length goes past its slider, as far as the piano
/// roll draws a note out.
pub fn note_range(key: &str) -> Option<(f64, f64)> {
    let prop = note_prop(key).filter(|_| key != "pitch")?;
    Some((prop.min, if key == "lengthSteps" { MAX_LENGTH_STEPS } else { prop.max }))
}

/// The note editor's setting of key `key`: a knob's or a slider's.
#[must_use]
pub fn note_prop(key: &str) -> Option<&'static Prop> {
    let knobs = NOTE_KNOBS.iter().map(|(prop, _)| prop);
    let sliders = NOTE_GROUPS.iter().flat_map(|rows| rows.iter()).filter_map(|row| match row {
        NoteRow::Slider(prop) => Some(prop),
        NoteRow::Select(..) => None,
    });
    knobs.chain(sliders).find(|prop| prop.key == key)
}

/// The note settings the grid's lane shows, one bar per step: their keys
/// (settings of the note editor) and the labels of their tabs.
pub const LANES: [(&str, &str); 6] = [
    ("velocity", "VELOCITY"),
    ("probability", "CHANCE"),
    ("retriggerCount", "RETRIG"),
    ("pan", "PAN"),
    ("lengthSteps", "LENGTH"),
    ("shiftSteps", "SHIFT"),
];

/// The value of note setting `prop` of `note`, else its default.
#[must_use]
pub fn note_value(note: &Note, prop: &Prop) -> f64 {
    note.field(prop.key).and_then(|v| v.as_f64()).unwrap_or_else(|| note_default(prop.key, prop.min))
}

pub const ARP_DIRECTIONS: [&str; 4] = ["up", "down", "updown", "random"];

/// Kit editor knobs: field, label, range, step, format, unit, default.
pub const KIT_KNOBS: [(Field, Prop, &str, f64); 3] = [
    (Field::Gain, prop("gain", "Gain", GAIN_RANGE.0, GAIN_RANGE.1, 0.1, Format::Fixed1), "dB", 0.0),
    (
        Field::Transpose,
        prop("transpose", "Transpose", TRANSPOSE_RANGE.0, TRANSPOSE_RANGE.1, 0.1, Format::SignedFixed1),
        "st",
        0.0,
    ),
    (Field::Decay, prop("decay", "Decay", DECAY_RANGE.0, DECAY_RANGE.1, 10.0, Format::Millis), "", DEFAULT_DECAY),
];

/// The playback knobs of a sample, in a row of four: field, label, step, default (the
/// end's is the sample's length, the range of the start and the end too).
pub const SHAPE_KNOBS: [(Field, &str, f64); 4] = [
    (Field::Start, "Start", 1.0),
    (Field::End, "End", 1.0),
    (Field::Attack, "Attack", 1.0),
    (Field::Hold, "Hold", 5.0),
];

/// The longest a knob sets the attack and the hold, ms.
pub const SHAPE_RANGES: (f64, f64) = (ATTACK_RANGE.1, HOLD_RANGE.1);

/// Attack a double-click restores, ms (the voice's own).
pub const DEFAULT_ATTACK: f64 = ordrumbox_engine::kit::DEFAULT_ATTACK_MS;

/// The playback switches of a sample: field, label.
pub const SHAPE_SWITCHES: [(Field, &str); 3] =
    [(Field::Reverse, "Reverse"), (Field::Loop, "Loop"), (Field::Gate, "Gate")];
