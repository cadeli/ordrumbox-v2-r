//! The synth editor's controls set from a list: which they are, the values
//! each takes and their labels.

use ordrumbox_core::json::Value;

use super::{FILTER_TYPES, MATRIX_SIZE, MATRIX_SOURCES, NOISE_MODES, OSC2_SYNCS, WAVES, lfo_targets};

/// The LFO syncs (value, label).
/// The labels of what oscillator 2 syncs to, in the order of [`OSC2_SYNCS`].
pub const OSC2_SYNC_LABELS: [&str; 2] = ["Off", "VCO1"];
/// The labels of the noise's modes, in the order of [`NOISE_MODES`].
pub const NOISE_MODE_LABELS: [&str; 3] = ["White", "Long", "Short"];
/// Filter slopes, dB per octave, and their labels.
pub const FILTER_SLOPES: [(&str, &str); 2] = [("12", "12 dB"), ("24", "24 dB")];
pub const LFO_SYNCS: [(&str, &str); 8] = [
    ("off", "free"),
    ("1/1", "1/1"),
    ("1/2", "1/2"),
    ("1/4", "1/4"),
    ("1/8", "1/8"),
    ("1/16", "1/16"),
    ("1/8T", "1/8T"),
    ("1/16T", "1/16T"),
];

/// The controls set from a list (`filter.slope` holds numbers).
pub const CHOICE_PATHS: [&str; 12 + 2 * MATRIX_SIZE] = [
    "vco1.wave",
    "vco2.wave",
    "vco3.wave",
    "vco2.sync",
    "lfo.wave",
    "lfo2.wave",
    "filter.type",
    "noise.filterType",
    "filter.slope",
    "noise.mode",
    "lfo.sync",
    "lfo2.sync",
    "matrix.source1",
    "matrix.target1",
    "matrix.source2",
    "matrix.target2",
    "matrix.source3",
    "matrix.target3",
    "matrix.source4",
    "matrix.target4",
    "matrix.source5",
    "matrix.target5",
    "matrix.source6",
    "matrix.target6",
    "matrix.source7",
    "matrix.target7",
    "matrix.source8",
    "matrix.target8",
    "matrix.source9",
    "matrix.target9",
    "matrix.source10",
    "matrix.target10",
    "matrix.source11",
    "matrix.target11",
];

/// Whether `path` is `matrix.<name><n>` for a line `n` of the matrix.
fn is_matrix_line(path: &str, name: &str) -> bool {
    path.strip_prefix("matrix.")
        .and_then(|rest| rest.strip_prefix(name))
        .is_some_and(|n| (1..=MATRIX_SIZE).any(|line| line.to_string() == n))
}

/// The values control `path` takes from a list, if it is one of [`CHOICE_PATHS`].
pub fn choices(path: &str) -> Option<Vec<&'static str>> {
    let values = |list: &[(&'static str, &'static str)]| list.iter().map(|(value, _)| *value).collect();
    Some(match path {
        "vco1.wave" | "vco2.wave" | "vco3.wave" | "lfo.wave" | "lfo2.wave" => WAVES.to_vec(),
        "filter.type" | "noise.filterType" => FILTER_TYPES.to_vec(),
        "filter.slope" => values(&FILTER_SLOPES),
        "noise.mode" => NOISE_MODES.to_vec(),
        "vco2.sync" => OSC2_SYNCS.to_vec(),
        "lfo.sync" | "lfo2.sync" => values(&LFO_SYNCS),
        _ if is_matrix_line(path, "source") => MATRIX_SOURCES.to_vec(),
        _ if is_matrix_line(path, "target") => lfo_targets(),
        _ => return None,
    })
}

/// Value `value` of list control `path` as a preset holds it: the slope
/// list holds numbers.
pub fn choice_value(path: &str, value: &str) -> Value {
    if path == "filter.slope" {
        Value::from(value).to_number().map_or_else(|| Value::from(value), Value::from)
    } else {
        Value::from(value)
    }
}
