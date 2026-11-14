//! Drum kits (`assets/data/drumkits.json`), their decoded samples, and which
//! sample each track plays.

use std::sync::Arc;

use ordrumbox_core::Issue;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_dsp::{SampleBuffer, SampleShape};

use crate::wav;

/// Decay used when a kit gives none, ms.
pub const DEFAULT_DECAY_MS: f64 = 500.0;
/// Attack used when a kit gives none, ms.
pub const DEFAULT_ATTACK_MS: f64 = 5.0;

/// One sample of a kit, as listed in drumkits.json.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KitSample {
    /// Matching key: `KICK`, `SNARE`, `CHH`…
    pub key: String,
    pub display_name: String,
    /// Path under `assets/kits/` (or the user's samples folder), also the
    /// sample id tracks refer to.
    pub path: String,
    /// Decay to -60 dB, ms, when the kit sets one (see [`KitSample::decay_ms`]).
    pub decay: Option<f64>,
    /// MIDI note the sample sounds at unpitched (`rootMidi`), when known:
    /// the note MIDI files map pitch 0 to.
    pub root_midi: Option<f64>,
    /// Peak level the kit measured, dB (`peakDb`), kept as written.
    pub peak_db: Option<f64>,
    /// Where reading starts, ms into the sample.
    pub start: f64,
    /// Where reading ends, ms into the sample, when the kit sets one: else the sample's end.
    pub end: Option<f64>,
    /// Read the region backwards.
    pub reverse: bool,
    /// Read the region over and over until the decay ends it (`loop`).
    pub looping: bool,
    /// Attack, ms, when the kit sets one (see [`KitSample::attack_ms`]).
    pub attack: Option<f64>,
    /// Time kept at the peak before the decay, ms.
    pub hold: f64,
    /// Hold the peak until the note ends, the decay then being its release (`gate`).
    pub gated: bool,
    /// Gain trim, dB (`gainDb`).
    pub gain_db: f64,
    /// Transposition, semitones.
    pub transpose: f64,
    /// Other keys of the entry, written back as they came.
    pub extra: Object,
}

impl KitSample {
    /// Decay to -60 dB, ms (500 when the kit sets none).
    pub fn decay_ms(&self) -> f64 {
        self.decay.unwrap_or(DEFAULT_DECAY_MS)
    }

    /// Attack, ms (5 when the kit sets none).
    pub fn attack_ms(&self) -> f64 {
        self.attack.unwrap_or(DEFAULT_ATTACK_MS)
    }

    /// How the sample is read and shaped.
    pub fn shape(&self) -> SampleShape {
        SampleShape {
            start: (self.start / 1000.0) as f32,
            end: self.end.map(|end| (end / 1000.0) as f32),
            reverse: self.reverse,
            looping: self.looping,
            attack: (self.attack_ms() / 1000.0) as f32,
            hold: (self.hold / 1000.0) as f32,
            gated: self.gated,
        }
    }

    /// Linear gain of the trim.
    pub fn gain(&self) -> f64 {
        10f64.powf(self.gain_db / 20.0)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let text =
            |keys: &[&str]| keys.iter().find_map(|key| object.get(key).and_then(Value::as_str)).map(str::to_owned);
        let number = |keys: &[&str]| keys.iter().find_map(|key| object.get(key).and_then(Value::as_f64));
        let flag = |key: &str| object.get(key).and_then(Value::as_bool).unwrap_or(false);
        let path = text(&["path", "url"])?;
        let mut extra = Object::new();
        for (key, value) in object.iter() {
            if !KNOWN_KEYS.contains(&key) {
                extra.insert(key, value.clone());
            }
        }
        Some(Self {
            key: text(&["key"]).unwrap_or_default(),
            display_name: text(&["displayName", "display_name"]).unwrap_or_default(),
            path,
            decay: number(&["decay"]),
            start: number(&["start"]).unwrap_or(0.0),
            end: number(&["end"]),
            reverse: flag("reverse"),
            looping: flag("loop"),
            attack: number(&["attack"]),
            hold: number(&["hold"]).unwrap_or(0.0),
            gated: flag("gate"),
            root_midi: number(&["rootMidi"]),
            peak_db: number(&["peakDb"]),
            gain_db: number(&["gainDb"]).unwrap_or(0.0),
            transpose: number(&["transpose", "tune"]).unwrap_or(0.0),
            extra,
        })
    }

    /// The entry as kits.json holds it (`gainDb`/`transpose` only when set).
    pub fn to_json(&self) -> Value {
        let mut o = Object::new();
        o.insert("displayName", Value::from(self.display_name.as_str()));
        o.insert("key", Value::from(self.key.as_str()));
        o.insert("path", Value::from(self.path.as_str()));
        if let Some(peak) = self.peak_db {
            o.insert("peakDb", Value::Number(peak));
        }
        if let Some(root) = self.root_midi {
            o.insert("rootMidi", Value::Number(root));
        }
        if let Some(decay) = self.decay {
            o.insert("decay", Value::Number(decay));
        }
        self.insert_shape(&mut o);
        if self.gain_db != 0.0 {
            o.insert("gainDb", Value::Number(self.gain_db));
        }
        if self.transpose != 0.0 {
            o.insert("transpose", Value::Number(self.transpose));
        }
        for (key, value) in self.extra.iter() {
            o.insert(key, value.clone());
        }
        Value::Object(o)
    }

    /// The playback settings that differ from the defaults, as kits.json keys.
    fn insert_shape(&self, o: &mut Object) {
        if self.start != 0.0 {
            o.insert("start", Value::Number(self.start));
        }
        if let Some(end) = self.end {
            o.insert("end", Value::Number(end));
        }
        if self.reverse {
            o.insert("reverse", Value::Bool(true));
        }
        if self.looping {
            o.insert("loop", Value::Bool(true));
        }
        if let Some(attack) = self.attack {
            o.insert("attack", Value::Number(attack));
        }
        if self.hold != 0.0 {
            o.insert("hold", Value::Number(self.hold));
        }
        if self.gated {
            o.insert("gate", Value::Bool(true));
        }
    }

    /// The entry of a kit mapping file: every field,
    /// unset ones as null or 0.
    fn to_mapping_json(&self) -> Value {
        let mut o = Object::new();
        let opt = |v: Option<f64>| v.map_or(Value::Null, Value::Number);
        o.insert("path", Value::from(self.path.as_str()));
        o.insert("displayName", Value::from(self.display_name.as_str()));
        o.insert("key", Value::from(self.key.as_str()));
        o.insert("rootMidi", opt(self.root_midi));
        o.insert("peakDb", opt(self.peak_db));
        o.insert("decay", opt(self.decay));
        o.insert("gainDb", Value::Number(self.gain_db));
        o.insert("transpose", Value::Number(self.transpose));
        self.insert_shape(&mut o);
        Value::Object(o)
    }
}

/// `display_name`, `url`, `tune`: the keys of the JS app's drumkits.json and older saves.
const KNOWN_KEYS: [&str; 18] = [
    "key",
    "displayName",
    "display_name",
    "path",
    "url",
    "decay",
    "start",
    "end",
    "reverse",
    "loop",
    "attack",
    "hold",
    "gate",
    "rootMidi",
    "peakDb",
    "gainDb",
    "transpose",
    "tune",
];

/// A kit's samples: `instruments` in the JS app's drumkits.json and older saves.
fn sample_entries(value: &Value) -> Option<&[Value]> {
    ["samples", "instruments"].iter().find_map(|key| value.get(key).and_then(Value::as_array))
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Kit {
    pub name: String,
    pub samples: Vec<KitSample>,
    /// Other keys (`info`, `desc`…), written back as they came.
    pub extra: Object,
}

impl Kit {
    /// The kit at `at` (`[2]`), `None` without a name; a sample without a
    /// path is skipped. What is skipped goes to `issues`.
    fn from_json(value: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        let Some(name) = value.get("name").and_then(Value::as_str) else {
            issues.push(Issue::new(at, "kit without a name: skipped"));
            return None;
        };
        let object = value.as_object()?;
        let name = name.to_owned();
        let samples = sample_entries(value)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let sample = KitSample::from_json(entry);
                if sample.is_none() {
                    issues.push(Issue::new(format!("{at}.samples[{index}]"), "sample without a path: skipped"));
                }
                sample
            })
            .collect();
        let mut extra = Object::new();
        for (key, value) in object.iter() {
            match key {
                "name" | "samples" | "instruments" => {}
                "infos" => _ = extra.insert("info", value.clone()), // the JS key
                _ => _ = extra.insert(key, value.clone()),
            }
        }
        Some(Self { name, samples, extra })
    }

    pub fn to_json(&self) -> Value {
        let mut o = Object::new();
        o.insert("name", Value::from(self.name.as_str()));
        for (key, value) in self.extra.iter() {
            o.insert(key, value.clone());
        }
        o.insert("samples", Value::Array(self.samples.iter().map(KitSample::to_json).collect()));
        Value::Object(o)
    }

    /// A kit mapping file: name and samples, no audio.
    pub fn to_mapping_json(&self) -> Value {
        let mut o = Object::new();
        o.insert("name", Value::from(self.name.as_str()));
        o.insert("samples", Value::Array(self.samples.iter().map(KitSample::to_mapping_json).collect()));
        Value::Object(o)
    }

    /// A kit from a mapping file: a name and at least one
    /// entry with a string `path` and `key`; the display name defaults to the path.
    pub fn from_mapping_json(value: &Value) -> Result<Self, String> {
        let name = value.get("name").and_then(Value::as_str);
        let (Some(name), Some(entries)) = (name, sample_entries(value)) else {
            return Err("Missing kit name or samples".into());
        };
        let samples: Vec<KitSample> = entries
            .iter()
            .filter(|e| e.get("key").and_then(Value::as_str).is_some())
            .filter_map(|e| {
                let mut sample = KitSample::from_json(e)?;
                if ["displayName", "display_name"].iter().all(|key| e.get(key).is_none_or(Value::is_null)) {
                    sample.display_name.clone_from(&sample.path);
                }
                sample.extra = Object::new();
                Some(sample)
            })
            .collect();
        if samples.is_empty() {
            return Err("No valid samples".into());
        }
        Ok(Self { name: name.to_owned(), samples, extra: Object::new() })
    }
}

/// Kits read from a file, and the kits and samples of it skipped.
pub type KitsRead = (Vec<Kit>, Vec<Issue>);

/// Read kits.json (an array of kits): the kits, and the kits and samples
/// skipped (malformed). Anything but an array is an error.
pub fn kits_from_json(value: &Value) -> Result<KitsRead, String> {
    let entries = value.as_array().ok_or("not an array of kits")?;
    let mut issues = Vec::new();
    let kits = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| Kit::from_json(entry, &format!("[{index}]"), &mut issues))
        .collect();
    Ok((kits, issues))
}

/// drumkits.json from kits.
pub fn kits_to_json(kits: &[Kit]) -> Value {
    Value::Array(kits.iter().map(Kit::to_json).collect())
}

/// A loaded sample.
#[derive(Debug, Clone, PartialEq)]
pub struct BankSample {
    pub kit: String,
    pub sample: KitSample,
    /// Shared: a bank rebuilt after a kit edit keeps the decoded audio.
    pub buffer: Arc<SampleBuffer>,
}

/// Every decoded sample, addressed by index (what the audio thread holds).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SampleBank {
    pub samples: Vec<BankSample>,
}

impl SampleBank {
    /// Decode every instrument of `kits`, reading files through `read(url)`.
    /// Returns the bank and one message per sample that could not be loaded.
    pub fn load(kits: &[Kit], mut read: impl FnMut(&str) -> std::io::Result<Vec<u8>>) -> (Self, Vec<String>) {
        Self::rebuild(kits, |_| None, |url| read(url))
    }

    /// The bank of `kits`, taking the audio of a sample from `known(url)`
    /// when it has it, else decoding what `read(url)` gives.
    pub fn rebuild(
        kits: &[Kit],
        known: impl Fn(&str) -> Option<Arc<SampleBuffer>>,
        mut read: impl FnMut(&str) -> std::io::Result<Vec<u8>>,
    ) -> (Self, Vec<String>) {
        let mut samples = Vec::new();
        let mut errors = Vec::new();
        for kit in kits {
            for instrument in &kit.samples {
                let buffer = match known(&instrument.path) {
                    Some(buffer) => Ok(buffer),
                    None => read(&instrument.path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| wav::decode(&b).map_err(|e| e.to_string()))
                        .map(Arc::new),
                };
                match buffer {
                    Ok(buffer) => {
                        samples.push(BankSample { kit: kit.name.clone(), sample: instrument.clone(), buffer })
                    }
                    Err(e) => errors.push(format!("{}: {e}", instrument.path)),
                }
            }
        }
        (Self { samples }, errors)
    }

    pub fn into_shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    pub fn find_path(&self, url: &str) -> Option<usize> {
        self.samples.iter().position(|s| s.sample.path == url)
    }

    pub fn get(&self, index: usize) -> Option<&BankSample> {
        self.samples.get(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn what_is_skipped_is_told() {
        let value = json::parse(r#"[{"name":"a","samples":[{"key":"k"},{"path":"a.wav"}]},{"samples":[]}]"#).unwrap();
        let (kits, issues) = kits_from_json(&value).unwrap();
        assert_eq!((kits.len(), kits[0].samples.len()), (1, 1));
        let at: Vec<_> = issues.iter().map(|i| i.at.as_str()).collect();
        assert_eq!(at, ["[0].samples[0]", "[1]"]);
        assert_eq!(kits_from_json(&json::parse("{}").unwrap()), Err("not an array of kits".to_owned()));
    }

    #[test]
    fn kits_and_samples_load() {
        let kits = kits_from_json(
            &json::parse(
                r#"[{"name":"a","instruments":[{"key":"KICK","url":"a/k.wav","decay":5000},{"key":"SNARE","url":"a/s.wav"}]},
                    {"name":"b","instruments":[{"key":"KICK","url":"b/k.wav"},{"key":"CHH","url":"b/h.wav"},{"key":"LO_TOM","url":"b/t.wav"}]},
                    {"instruments":[]}]"#,
            )
            .unwrap(),
        ).unwrap().0;
        assert_eq!(kits.len(), 2);
        assert_eq!(kits[0].samples[1].decay_ms(), DEFAULT_DECAY_MS);
        assert_eq!(kits[0].samples[0].decay, Some(5000.0));
        let silence = crate::wav::tests::wav(1, 1, 8_000, 16, &[0, 0]);
        let (bank, errors) = SampleBank::load(&kits, |_| Ok(silence.clone()));
        assert!(errors.is_empty());
        assert_eq!(bank.find_path("b/h.wav"), Some(3));
        assert_eq!(bank.samples[3].kit, "b");
    }

    #[test]
    fn load_reports_unreadable_files() {
        let kits = vec![Kit {
            name: "x".into(),
            samples: vec![KitSample {
                key: "K".into(),
                display_name: "k".into(),
                path: "x.wav".into(),
                decay: Some(1.0),
                ..Default::default()
            }],
            extra: Object::new(),
        }];
        let (bank, errors) = SampleBank::load(&kits, |_| Ok(b"garbage".to_vec()));
        assert!(bank.samples.is_empty());
        assert_eq!(errors, ["x.wav: not a RIFF/WAVE file"]);
    }

    #[test]
    fn kit_files_round_trip() {
        let text = r#"[{"name":"real","infos":" kit","instruments":[{"display_name":"kick","key":"KICK","url":"real/kick.wav","peakDb":-3,"rootMidi":72,"decay":5000,"x":1},{"display_name":"s","key":"SNARE","url":"s.wav","gainDb":-2.5,"tune":1}]}]"#;
        let kits = kits_from_json(&json::parse(text).unwrap()).unwrap().0;
        // Read under the JS key, written under the current one.
        // Read under the JS keys, written under the current ones.
        let current = [
            ("display_name", "displayName"),
            ("infos", "info"),
            ("instruments", "samples"),
            ("url", "path"),
            ("tune", "transpose"),
        ]
        .iter()
        .fold(text.to_owned(), |text, (old, new)| text.replace(&format!("\"{old}\""), &format!("\"{new}\"")));
        assert_eq!(kits_to_json(&kits).to_json(), current);
        assert_eq!(kits_from_json(&kits_to_json(&kits)).unwrap().0, kits);
        assert_eq!(
            kits[0].to_mapping_json().to_json(),
            r#"{"name":"real","samples":[{"path":"real/kick.wav","displayName":"kick","key":"KICK","rootMidi":72,"peakDb":-3,"decay":5000,"gainDb":0,"transpose":0},{"path":"s.wav","displayName":"s","key":"SNARE","rootMidi":null,"peakDb":null,"decay":null,"gainDb":-2.5,"transpose":1}]}"#
        );
        let restored = Kit::from_mapping_json(&kits[0].to_mapping_json()).unwrap();
        assert_eq!(restored.samples[0].decay, Some(5000.0));
        assert_eq!(restored.samples[1].gain_db, -2.5);
        let missing = json::parse(r#"{"name":"n","instruments":[{"url":"a.wav"}]}"#).unwrap();
        assert_eq!(Kit::from_mapping_json(&missing), Err("No valid samples".into()));
        let unnamed = json::parse(r#"{"instruments":[{"url":"a.wav","key":"K"}]}"#).unwrap();
        assert_eq!(Kit::from_mapping_json(&unnamed), Err("Missing kit name or samples".into()));
        let defaults = json::parse(r#"{"name":"n","instruments":[{"url":"a.wav","key":"K"}]}"#).unwrap();
        assert_eq!(Kit::from_mapping_json(&defaults).unwrap().samples[0].display_name, "a.wav");
    }

    #[test]
    fn playback_settings_round_trip_and_stay_out_of_the_file_when_default() {
        let plain = KitSample { path: "a.wav".into(), ..KitSample::default() };
        assert_eq!(plain.to_json().to_json(), r#"{"displayName":"","key":"","path":"a.wav"}"#);
        assert_eq!(plain.shape(), SampleShape::default());
        let shaped = KitSample {
            start: 20.0,
            end: Some(300.0),
            reverse: true,
            looping: true,
            attack: Some(1.0),
            hold: 50.0,
            gated: true,
            ..plain
        };
        let back = KitSample::from_json(&shaped.to_json()).unwrap();
        assert_eq!(back, shaped);
        let shape = back.shape();
        assert_eq!((shape.start, shape.end, shape.attack, shape.hold), (0.02, Some(0.3), 0.001, 0.05));
        assert!(shape.reverse && shape.looping);
        assert!(back.extra.get("loop").is_none(), "known keys are not kept as extra ones");
    }
}
