//! A track of a pattern (`src/model/track_schema.js`): its parameters, its
//! notes, and the load/save rules of `fixer.js` and `exporter.js`.

use std::collections::HashMap;

use crate::drum_taxonomy::pan_from_track_name;
use crate::json::{Object, Value};
use crate::note::{Note, decode_notes, encode_notes};
use crate::record::{Issue, SetField, json_record, round2};

json_record! {
    /// One track. Table fields are saved only when they differ from their
    /// default (except `pan`, see `Track::to_json`); their order is the
    /// `TRACK_DEFAULTS` order.
    pub struct Track {
        /// Display name; also picks the default pan (`KICK`, `SNARE`…).
        name: String = String::new() => "name",
        /// Pick the sample from the track name.
        auto_assign: bool = true => "autoAssign" | "useAutoAssignSound",
        /// The sample this track plays (a key of the sound registry).
        sample_path: String = "NOT_DEFINED".to_owned() => "samplePath" | "sampleId",
        /// Beats in the track (independent of the pattern's).
        beat_count: i32 = 4 => "beatCount",
        /// Grid subdivision: steps per beat, 1–8.
        steps_per_beat: i32 = 4 => "stepsPerBeat",
        /// Loop point in steps; `None` loops over the track length.
        loop_length: Option<i32> = None => "loopLength" | "loopAtStep",
        swing_resolution: i32 = 1 => "swingResolution",
        swing_amount: f64 = 0.0 => "swingAmount",
        volume: f64 = 1.0 => "volume" | "velocity",
        /// LFO objects (`{freq, min, max, phase, type}`) are kept as written
        /// and read leniently when played (see [`crate::lfo`]).
        volume_lfo: Option<Object> = None => "volumeLfo" | "velocityLfo",
        /// Transposition in semitones.
        transpose: f64 = 0.0 => "transpose" | "pitch",
        transpose_lfo: Option<Object> = None => "transposeLfo" | "pitchLfo",
        pan: f64 = 0.0 => "pan",
        pan_lfo: Option<Object> = None => "panLfo",
        solo: bool = false => "solo",
        mute: bool = false => "mute",
        /// Auto mode: the generation engine drives the track.
        auto_mode: bool = false => "autoMode" | "auto",
        use_synth: bool = false => "useSynth" | "useSoftSynth",
        mono: bool = false => "mono",
        /// Variation amounts, 0–100.
        hit_variation: f64 = 0.0 => "hitVariation" | "variation",
        note_variation: f64 = 0.0 => "noteVariation" | "variation2",
        /// Base probability of every note, 0–1.
        probability: f64 = 1.0 => "probability",
        /// Per-note chances of the auto engine, 0–100.
        auto_pitch_chance: f64 = 50.0 => "autoPitchChance" | "probPitch" | "prob_pitch",
        auto_velocity_chance: f64 = 50.0 => "autoVelocityChance" | "probVelocity" | "prob_velocity",
        auto_silence_chance: f64 = 50.0 => "autoSilenceChance" | "probSilence" | "prob_silence",
        auto_fill_chance: f64 = 50.0 => "autoFillChance" | "probFill" | "prob_fill",
        auto_ghost_chance: f64 = 50.0 => "autoGhostChance" | "probGhost" | "prob_ghost",
        auto_retrigger_chance: f64 = 50.0 => "autoRetriggerChance" | "probRetrigger" | "prob_retrig",
        auto_euclidean_chance: f64 = 50.0 => "autoEuclideanChance" | "probEuclidean" | "prob_euclid",
        auto_note_probability_chance: f64 = 50.0 => "autoNoteProbabilityChance" | "probNote" | "prob_note",
        auto_note_probability_range: f64 = 100.0 => "autoNoteProbabilityRange" | "probNoteDepth" | "prob_note_depth",
        auto_arp_chance: f64 = 50.0 => "autoArpChance" | "probArp" | "prob_arp",
        /// Semitone span of pitch randomization, 1–24.
        pitch_range: f64 = 12.0 => "pitchRange" | "pitch_range",
        pitch_scale_lock: bool = false => "pitchScaleLock" | "pitch_scale_lock",
        auto_variant: String = String::new() => "autoVariant" | "auto_variant",
        /// Per-loop density offset, -1–1 (-1 = auto).
        auto_density: f64 = -1.0 => "autoDensity" | "auto_density",
        filter_type: String = "allpass".to_owned() => "filterType",
        filter_cutoff_lfo: Option<Object> = None => "filterCutoffLfo" | "filterFreqLfo",
        /// Hz.
        filter_cutoff: f64 = 20.0 => "filterCutoff" | "filterFreq",
        filter_q_lfo: Option<Object> = None => "filterQLfo",
        filter_q: f64 = 0.707 => "filterQ",
        reverb_type: String = "none".to_owned() => "reverbType",
        reverb_mix: f64 = 0.0 => "reverbMix" | "reverbAmount",
        delay_type: String = "tape".to_owned() => "delayType",
        delay_beats: f64 = 1.0 => "delayBeats" | "delayTime",
        delay_mix: f64 = 0.0 => "delayMix" | "delayDepth",
        saturation_type: String = "soft".to_owned() => "saturationType",
        saturation_mix: f64 = 0.0 => "saturationMix" | "saturationAmount",
        saturation_on: bool = true => "saturationOn" | "sat",
        reverb_on: bool = true => "reverbOn",
        delay_on: bool = true => "delayOn",
        /// Synth preset played when `use_synth` is on.
        synth_preset: Option<String> = None => "synthPreset" | "synthSoundKey",
        // Not in TRACK_DEFAULTS: written by the auto generator / scale picker,
        // saved only when set.
        /// Variation amount the auto generator picked (auto mode); `None` until it ran.
        auto_hit_variation: Option<f64> = None => "autoHitVariation" | "auto_variation",
        /// Per-note variation amount the auto generator picked (auto mode).
        auto_note_variation: Option<f64> = None => "autoNoteVariation" | "auto_variation2",
        /// Scale (a key of scales.json) the pitch variation snaps to when `pitch_scale_lock` is on.
        scale_name: Option<String> = None => "scaleName",
    }
    other_fields {
        notes: Vec<Note> = Vec::new(),
        /// Keys this model does not know, kept and written back as they were.
        extra: Object = Object::new(),
    }
}

/// `TRACK_VALUE_RANGES`: bounds of the numeric settings an edit clamps to.
const VALUE_RANGES: [(&str, f64, f64); 28] = [
    ("volume", 0.0, 1.0),
    ("pan", -1.0, 1.0),
    ("transpose", -24.0, 24.0),
    ("beatCount", 1.0, 16.0),
    ("stepsPerBeat", 1.0, 8.0),
    ("loopLength", 0.0, 1024.0),
    ("swingResolution", 1.0, 8.0),
    ("swingAmount", 0.0, 1.0),
    ("filterCutoff", 20.0, 20_000.0),
    ("filterQ", 0.707, 18.707),
    ("reverbMix", 0.0, 1.0),
    ("delayBeats", 0.0, 4.0),
    ("delayMix", 0.0, 1.0),
    ("saturationMix", 0.0, 1.0),
    ("hitVariation", 0.0, 100.0),
    ("noteVariation", 0.0, 100.0),
    ("autoPitchChance", 0.0, 100.0),
    ("autoVelocityChance", 0.0, 100.0),
    ("autoSilenceChance", 0.0, 100.0),
    ("autoFillChance", 0.0, 100.0),
    ("autoGhostChance", 0.0, 100.0),
    ("autoRetriggerChance", 0.0, 100.0),
    ("autoEuclideanChance", 0.0, 100.0),
    ("autoNoteProbabilityChance", 0.0, 100.0),
    ("autoNoteProbabilityRange", 0.0, 100.0),
    ("autoArpChance", 0.0, 100.0),
    ("pitchRange", 1.0, 24.0),
    ("autoDensity", -1.0, 1.0),
];

/// Table keys that are not track settings (`TRACK_DEFAULTS` lacks them):
/// an edit never sets them.
const NOT_SETTINGS: [&str; 3] = ["autoHitVariation", "autoNoteVariation", "scaleName"];

/// Keys written rounded to 2 decimals (`ROUND_2D` in exporter.js).
const ROUND_2D: [&str; 9] =
    ["volume", "pan", "reverbMix", "delayMix", "delayBeats", "saturationMix", "swingAmount", "filterCutoff", "filterQ"];

impl Track {
    /// A new track: `beat_count` beats at `steps_per_beat`
    /// (clamped to 1–8), looping over its whole length, panned from its name.
    pub fn new(name: &str, beat_count: i32, steps_per_beat: i32) -> Self {
        let steps_per_beat = steps_per_beat.clamp(1, 8);
        Self {
            name: name.to_owned(),
            beat_count,
            steps_per_beat,
            loop_length: Some(beat_count * steps_per_beat),
            pan: pan_from_track_name(name),
            ..Self::default()
        }
    }

    /// Whether `key` is a track setting an edit may change (a key of `TRACK_DEFAULTS`).
    /// A legacy key (`prob_pitch`, `velocity`…) counts as its current one.
    pub fn is_setting(key: &str) -> bool {
        Self::canonical_key(key).is_some_and(|key| !NOT_SETTINGS.contains(&key))
    }

    /// Bounds an edit clamps the numeric setting `key` to (`TRACK_VALUE_RANGES`).
    pub fn value_range(key: &str) -> Option<(f64, f64)> {
        let key = Self::canonical_key(key)?;
        VALUE_RANGES.iter().find(|(k, _, _)| *k == key).map(|&(_, min, max)| (min, max))
    }

    /// Load a track as `fixTrackDefaults()` does: decode compact notes, take
    /// the pan from the name when the file has none, fill defaults, switch
    /// auto-assign off for synth tracks, fold note steps past their beat.
    /// Values of the wrong type are reported and replaced by their default.
    pub(crate) fn from_json(value: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        let Some(object) = value.as_object() else {
            issues.push(Issue::new(at, "track is not an object, skipped"));
            return None;
        };
        let mut track = Self::default();
        for (key, value) in object.iter() {
            match key {
                // `fxSelected`: an editor tab the JS app saved with the track.
                "notes" | "noteKeys" | "fxSelected" => {}
                _ => match track.set_field(key, value) {
                    SetField::Set => {}
                    SetField::WrongType if key == "pan" => {
                        let message = format!("not a number ({value}), pan taken from the track name");
                        issues.push(Issue::new(format!("{at}.pan"), message));
                    }
                    SetField::WrongType => {
                        issues.push(Issue::new(format!("{at}.{key}"), format!("wrong type ({value}), default kept")));
                    }
                    SetField::Unknown => {
                        track.extra.insert(key, value.clone());
                    }
                },
            }
        }
        if !object.get("pan").is_some_and(|pan| pan.as_f64().is_some()) {
            track.pan = pan_from_track_name(&track.name);
        }
        if track.use_synth {
            track.auto_assign = false;
        }
        match object.get("notes") {
            Some(Value::Array(notes)) => track.notes = decode_notes(notes, object.get("noteKeys"), at, issues),
            Some(Value::Null) | None => {}
            Some(_) => issues.push(Issue::new(format!("{at}.notes"), "notes is not an array, skipped")),
        }
        for note in &mut track.notes {
            note.wrap_grid_position(track.steps_per_beat);
        }
        Some(track)
    }

    /// Save a track as `Exporter.cleanTrack()` does: non-default fields only
    /// (some rounded to 2 decimals), unknown keys as they came, then the notes
    /// in compact form. `pan` is also written when it differs from the pan the
    /// name implies, even at 0: a renamed track must not be re-panned on load.
    pub(crate) fn to_json(&self) -> Value {
        let name_pan = pan_from_track_name(&self.name);
        let mut object = Object::new();
        for (key, value, is_default) in self.table_fields() {
            let keep = !is_default || (key == "pan" && self.pan != name_pan);
            if keep {
                let value = match value {
                    Value::Number(n) if ROUND_2D.contains(&key) => Value::Number(round2(n)),
                    other => other,
                };
                object.insert(key, value);
            }
        }
        for (key, value) in self.extra.iter() {
            object.insert(key, value.clone());
        }
        if !self.notes.is_empty() {
            let (header, notes) = encode_notes(&self.notes);
            object.insert("noteKeys", Value::Array(header));
            object.insert("notes", Value::Array(notes));
        }
        Value::Object(object)
    }
}

/// What a note sounds like, wherever it is:
/// every field but its beat and step — its `stepPercent` included, as in
/// the app every note has one, so only notes on the same step
/// of their beat compare equal.
fn audible_signature(note: &Note, steps_per_beat: i32) -> String {
    let mut signature: Vec<String> = note
        .table_fields()
        .into_iter()
        .filter(|(key, ..)| !matches!(*key, "beat" | "step"))
        .map(|(key, value, _)| format!("{key}={}", value.to_json()))
        .collect();
    signature.push(format!("stepPercent={}", Value::from(note.step_percent(steps_per_beat)).to_json()));
    signature.join(",")
}

impl Track {
    /// Steps the track spans: its beats, widened to its last note.
    fn step_length(&self) -> i64 {
        let spb = i64::from(self.steps_per_beat);
        let declared = (i64::from(self.beat_count) * spb).max(0);
        let last = self.notes.iter().map(|n| i64::from(n.beat) * spb + i64::from(n.step) + 1).max().unwrap_or(0);
        declared.max(last)
    }

    /// The signatures of the notes on each step, `map` folding the steps
    ///.
    fn step_signatures<'a>(
        notes: impl Iterator<Item = &'a Note>,
        spb: i32,
        map: impl Fn(i64) -> i64,
    ) -> HashMap<i64, String> {
        let mut steps: HashMap<i64, Vec<String>> = HashMap::new();
        for note in notes {
            let step = map(i64::from(note.beat) * i64::from(spb) + i64::from(note.step));
            if step >= 0 {
                steps.entry(step).or_default().push(audible_signature(note, spb));
            }
        }
        steps
            .into_iter()
            .map(|(step, mut signatures)| {
                signatures.sort();
                (step, signatures.join("|"))
            })
            .collect()
    }

    /// Shorten the track to its shortest loop, when its notes repeat one
    /// (`addLoopToTrackIfPossible`, the tools panel's Compact Tracks): the
    /// loop point set, the notes past it removed, the rest sorted by step.
    /// Whether it changed.
    pub fn compact_loop(&mut self) -> bool {
        let spb = self.steps_per_beat;
        let total = self.step_length();
        if spb <= 0 || total <= 1 || self.notes.is_empty() {
            return false;
        }
        let current = self.loop_length.map(i64::from).filter(|&l| l > 0).unwrap_or(total);
        let step_of = |n: &Note| i64::from(n.beat) * i64::from(spb) + i64::from(n.step);
        let original = Self::step_signatures(self.notes.iter(), spb, |s| s);
        let Some(loop_at) = (1..current).find(|&loop_at| {
            let looped =
                Self::step_signatures(self.notes.iter().filter(|n| step_of(n) < loop_at), spb, |s| s % loop_at);
            (0..total).all(|step| original.get(&step) == looped.get(&(step % loop_at)))
        }) else {
            return false;
        };
        self.notes.retain(|n| step_of(n) < loop_at);
        self.notes.sort_by_key(step_of);
        self.loop_length = Some(loop_at as i32);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn load(text: &str) -> (Track, Vec<Issue>) {
        let mut issues = Vec::new();
        let track = Track::from_json(&json::parse(text).unwrap(), "t", &mut issues).unwrap();
        (track, issues)
    }

    #[test]
    fn pan_comes_from_the_name_only_when_missing() {
        assert_eq!(load(r#"{"name":"SNARE"}"#).0.pan, 0.3);
        assert_eq!(load(r#"{"name":"SNARE","pan":0}"#).0.pan, 0.0);
        assert_eq!(load(r#"{"name":"SNARE","pan":"left"}"#).0.pan, 0.3);
    }

    #[test]
    fn pan_is_saved_when_it_differs_from_the_name() {
        let saved = |text: &str| load(text).0.to_json().get("pan").cloned();
        assert_eq!(saved(r#"{"name":"KICK","pan":0}"#), None);
        assert_eq!(saved(r#"{"name":"SNARE"}"#), Some(Value::Number(0.3)));
        assert_eq!(saved(r#"{"name":"SNARE","pan":0}"#), Some(Value::Number(0.0)));
    }

    #[test]
    fn synth_tracks_do_not_auto_assign() {
        let (track, _) = load(r#"{"useSoftSynth":true,"useAutoAssignSound":true}"#);
        assert!(!track.auto_assign);
    }

    #[test]
    fn keeps_unknown_keys_and_reports_wrong_types() {
        let (track, issues) = load(r#"{"name":"X","custom":[1],"mono":"yes","loopAtStep":null}"#);
        assert_eq!(track.extra.get("custom"), Some(&json::parse("[1]").unwrap()));
        assert!(!track.mono);
        assert_eq!(track.loop_length, None);
        assert_eq!(issues, [Issue::new("t.mono", "wrong type (\"yes\"), default kept")]);
        assert_eq!(track.to_json().to_json(), r#"{"name":"X","custom":[1]}"#);
    }

    #[test]
    fn folds_steps_with_the_track_grid() {
        let (track, _) = load(r#"{"stepsPerBeat":3,"notes":[{"beatStep":7}]}"#);
        assert_eq!((track.notes[0].beat, track.notes[0].step), (2, 1));
    }

    #[test]
    fn reads_the_legacy_key_names() {
        let (track, issues) =
            load(r#"{"name":"KICK","velocity":0.5,"delayDepth":0.3,"prob_pitch":10,"sat":false,"synthSoundKey":"B1"}"#);
        assert!(issues.is_empty());
        assert_eq!((track.volume, track.delay_mix, track.auto_pitch_chance), (0.5, 0.3, 10.0));
        assert!(!track.saturation_on);
        assert_eq!(
            track.to_json().to_json(),
            r#"{"name":"KICK","volume":0.5,"autoPitchChance":10,"delayMix":0.3,"saturationOn":false,"synthPreset":"B1"}"#
        );
        assert!(Track::is_setting("prob_pitch") && Track::is_setting("autoPitchChance"));
        assert!(!Track::is_setting("auto_variation"));
        assert_eq!(Track::value_range("delayDepth"), Some((0.0, 1.0)));
    }

    #[test]
    fn rounds_mix_parameters() {
        let (track, _) = load(r#"{"name":"KICK","velocity":0.876,"filterFreq":1234.567,"filterQ":0.707}"#);
        assert_eq!(track.to_json().to_json(), r#"{"name":"KICK","volume":0.88,"filterCutoff":1234.57}"#);
    }
}
