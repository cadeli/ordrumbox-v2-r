//! A track of a pattern (`src/model/track_schema.js`): its parameters, its
//! notes, and the load/save rules of `fixer.js` and `exporter.js`.

use crate::drum_taxonomy::pan_from_track_name;
use crate::json::{Object, Value};
use crate::note::{Note, decode_notes, encode_notes};
use crate::record::{Issue, SetField, json_record, round2};

json_record! {
    /// One track. Table fields are saved only when they differ from their
    /// default (except `pan`, see [`Track::to_json`]); their order is the
    /// `TRACK_DEFAULTS` order.
    pub struct Track {
        /// Display name; also picks the default pan (`KICK`, `SNARE`…).
        name: String = String::new() => "name",
        /// Pick the sample from the track name.
        use_auto_assign_sound: bool = true => "useAutoAssignSound",
        /// The sample this track plays (a key of the sound registry).
        sample_id: String = "NOT_DEFINED".to_owned() => "sampleId",
        /// Beats in the track (independent of the pattern's).
        beat_count: i32 = 4 => "beatCount",
        /// Grid subdivision: steps per beat, 1–8.
        steps_per_beat: i32 = 4 => "stepsPerBeat",
        /// Loop point in steps; `None` loops over the track length.
        loop_at_step: Option<i32> = None => "loopAtStep",
        swing_resolution: i32 = 1 => "swingResolution",
        swing_amount: f64 = 0.0 => "swingAmount",
        velocity: f64 = 1.0 => "velocity",
        /// LFO objects (`{freq, min, max, phase, type}`) are kept as written
        /// until the LFO engine is ported (phase 3).
        velocity_lfo: Option<Object> = None => "velocityLfo",
        /// Transposition in semitones.
        pitch: f64 = 0.0 => "pitch",
        pitch_lfo: Option<Object> = None => "pitchLfo",
        pan: f64 = 0.0 => "pan",
        pan_lfo: Option<Object> = None => "panLfo",
        solo: bool = false => "solo",
        mute: bool = false => "mute",
        /// Auto mode: the generation engine drives the track.
        auto: bool = false => "auto",
        use_soft_synth: bool = false => "useSoftSynth",
        mono: bool = false => "mono",
        /// Variation amounts, 0–100.
        variation: f64 = 0.0 => "variation",
        variation2: f64 = 0.0 => "variation2",
        /// Base probability of every note, 0–1.
        probability: f64 = 1.0 => "probability",
        /// Per-note chances of the auto engine, 0–100.
        prob_pitch: f64 = 50.0 => "prob_pitch",
        prob_velocity: f64 = 50.0 => "prob_velocity",
        prob_silence: f64 = 50.0 => "prob_silence",
        prob_fill: f64 = 50.0 => "prob_fill",
        prob_ghost: f64 = 50.0 => "prob_ghost",
        prob_retrig: f64 = 50.0 => "prob_retrig",
        prob_euclid: f64 = 50.0 => "prob_euclid",
        prob_note: f64 = 50.0 => "prob_note",
        prob_note_depth: f64 = 100.0 => "prob_note_depth",
        prob_arp: f64 = 50.0 => "prob_arp",
        /// Semitone span of pitch randomization, 1–24.
        pitch_range: f64 = 12.0 => "pitch_range",
        pitch_scale_lock: bool = false => "pitch_scale_lock",
        auto_variant: String = String::new() => "auto_variant",
        /// Per-loop density offset, -1–1 (-1 = auto).
        auto_density: f64 = -1.0 => "auto_density",
        filter_type: String = "allpass".to_owned() => "filterType",
        filter_freq_lfo: Option<Object> = None => "filterFreqLfo",
        /// Hz.
        filter_freq: f64 = 20.0 => "filterFreq",
        filter_q_lfo: Option<Object> = None => "filterQLfo",
        filter_q: f64 = 0.707 => "filterQ",
        reverb_type: String = "none".to_owned() => "reverbType",
        reverb_amount: f64 = 0.0 => "reverbAmount",
        delay_type: String = "tape".to_owned() => "delayType",
        delay_time: f64 = 1.0 => "delayTime",
        delay_depth: f64 = 0.0 => "delayDepth",
        fx_selected: String = "reverb".to_owned() => "fxSelected",
        saturation_type: String = "soft".to_owned() => "saturationType",
        saturation_amount: f64 = 0.0 => "saturationAmount",
        sat: bool = true => "sat",
        reverb_on: bool = true => "reverbOn",
        delay_on: bool = true => "delayOn",
        /// Synth preset played when `use_soft_synth` is on.
        synth_sound_key: Option<String> = None => "synthSoundKey",
    }
    other_fields {
        notes: Vec<Note> = Vec::new(),
        /// Keys this model does not know, kept and written back as they were.
        extra: Object = Object::new(),
    }
}

/// Keys written rounded to 2 decimals (`ROUND_2D` in exporter.js).
const ROUND_2D: [&str; 9] = [
    "velocity",
    "pan",
    "reverbAmount",
    "delayDepth",
    "delayTime",
    "saturationAmount",
    "swingAmount",
    "filterFreq",
    "filterQ",
];

impl Track {
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
                "notes" | "noteKeys" => {}
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
        if track.use_soft_synth {
            track.use_auto_assign_sound = false;
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
        assert!(!track.use_auto_assign_sound);
    }

    #[test]
    fn keeps_unknown_keys_and_reports_wrong_types() {
        let (track, issues) = load(r#"{"name":"X","custom":[1],"mono":"yes","loopAtStep":null}"#);
        assert_eq!(track.extra.get("custom"), Some(&json::parse("[1]").unwrap()));
        assert!(!track.mono);
        assert_eq!(track.loop_at_step, None);
        assert_eq!(issues, [Issue::new("t.mono", "wrong type (\"yes\"), default kept")]);
        assert_eq!(track.to_json().to_json(), r#"{"name":"X","custom":[1]}"#);
    }

    #[test]
    fn folds_steps_with_the_track_grid() {
        let (track, _) = load(r#"{"stepsPerBeat":3,"notes":[{"beatStep":7}]}"#);
        assert_eq!((track.notes[0].beat, track.notes[0].beat_step), (2, 1));
    }

    #[test]
    fn rounds_mix_parameters() {
        let (track, _) = load(r#"{"name":"KICK","velocity":0.876,"filterFreq":1234.567,"filterQ":0.707}"#);
        assert_eq!(track.to_json().to_json(), r#"{"name":"KICK","velocity":0.88,"filterFreq":1234.57}"#);
    }
}
