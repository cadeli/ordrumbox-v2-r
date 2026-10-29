//! Drum kits (`assets/data/drumkits.json`), their decoded samples, and which
//! sample each track plays.

use std::sync::Arc;

use ordrumbox_core::json::{Object, Value};
use ordrumbox_dsp::SampleBuffer;

use crate::wav;

/// Decay used when a kit gives none (`sample.decay ?? 500` ms).
pub const DEFAULT_DECAY_MS: f64 = 500.0;

/// One sample of a kit, as listed in drumkits.json.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KitSample {
    /// Matching key: `KICK`, `SNARE`, `CHH`…
    pub key: String,
    pub display_name: String,
    /// Path under `assets/kits/` (or the user's samples folder), also the
    /// sample id tracks refer to.
    pub url: String,
    /// Decay to -60 dB, ms, when the kit sets one (see [`KitSample::decay_ms`]).
    pub decay: Option<f64>,
    /// MIDI note the sample sounds at unpitched (`rootMidi`), when known:
    /// the note MIDI files map pitch 0 to.
    pub root_midi: Option<f64>,
    /// Peak level the kit measured, dB (`peakDb`), kept as written.
    pub peak_db: Option<f64>,
    /// Gain trim, dB (`gainDb`).
    pub gain_db: f64,
    /// Transposition, semitones (`tune`).
    pub tune: f64,
    /// Other keys of the entry, written back as they came.
    pub extra: Object,
}

impl KitSample {
    /// Decay to -60 dB, ms (500 when the kit sets none).
    pub fn decay_ms(&self) -> f64 {
        self.decay.unwrap_or(DEFAULT_DECAY_MS)
    }

    /// Linear gain of the trim.
    pub fn gain(&self) -> f64 {
        10f64.powf(self.gain_db / 20.0)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let url = object.get("url")?.as_str()?.to_owned();
        let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
        let number = |key: &str| object.get(key).and_then(Value::as_f64);
        let mut extra = Object::new();
        for (key, value) in object.iter() {
            if !KNOWN_KEYS.contains(&key) {
                extra.insert(key, value.clone());
            }
        }
        Some(Self {
            key: text("key").unwrap_or_default(),
            display_name: text("display_name").unwrap_or_default(),
            url,
            decay: number("decay"),
            root_midi: number("rootMidi"),
            peak_db: number("peakDb"),
            gain_db: number("gainDb").unwrap_or(0.0),
            tune: number("tune").unwrap_or(0.0),
            extra,
        })
    }

    /// The entry as drumkits.json holds it (`gainDb`/`tune` only when set).
    pub fn to_json(&self) -> Value {
        let mut o = Object::new();
        o.insert("display_name", Value::from(self.display_name.as_str()));
        o.insert("key", Value::from(self.key.as_str()));
        o.insert("url", Value::from(self.url.as_str()));
        if let Some(peak) = self.peak_db {
            o.insert("peakDb", Value::Number(peak));
        }
        if let Some(root) = self.root_midi {
            o.insert("rootMidi", Value::Number(root));
        }
        if let Some(decay) = self.decay {
            o.insert("decay", Value::Number(decay));
        }
        if self.gain_db != 0.0 {
            o.insert("gainDb", Value::Number(self.gain_db));
        }
        if self.tune != 0.0 {
            o.insert("tune", Value::Number(self.tune));
        }
        for (key, value) in self.extra.iter() {
            o.insert(key, value.clone());
        }
        Value::Object(o)
    }

    /// The entry of a kit mapping file (`exportCurrentKit`): every field,
    /// unset ones as null or 0.
    fn to_mapping_json(&self) -> Value {
        let mut o = Object::new();
        let opt = |v: Option<f64>| v.map_or(Value::Null, Value::Number);
        o.insert("url", Value::from(self.url.as_str()));
        o.insert("display_name", Value::from(self.display_name.as_str()));
        o.insert("key", Value::from(self.key.as_str()));
        o.insert("rootMidi", opt(self.root_midi));
        o.insert("peakDb", opt(self.peak_db));
        o.insert("decay", opt(self.decay));
        o.insert("gainDb", Value::Number(self.gain_db));
        o.insert("tune", Value::Number(self.tune));
        Value::Object(o)
    }
}

const KNOWN_KEYS: [&str; 8] = ["key", "display_name", "url", "decay", "rootMidi", "peakDb", "gainDb", "tune"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Kit {
    pub name: String,
    pub samples: Vec<KitSample>,
    /// Other keys (`infos`, `desc`…), written back as they came.
    pub extra: Object,
}

impl Kit {
    fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let name = object.get("name")?.as_str()?.to_owned();
        let instruments = object
            .get("instruments")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(KitSample::from_json)
            .collect();
        let mut extra = Object::new();
        for (key, value) in object.iter() {
            if key != "name" && key != "instruments" {
                extra.insert(key, value.clone());
            }
        }
        Some(Self { name, samples: instruments, extra })
    }

    pub fn to_json(&self) -> Value {
        let mut o = Object::new();
        o.insert("name", Value::from(self.name.as_str()));
        for (key, value) in self.extra.iter() {
            o.insert(key, value.clone());
        }
        o.insert("instruments", Value::Array(self.samples.iter().map(KitSample::to_json).collect()));
        Value::Object(o)
    }

    /// A kit mapping file (`exportCurrentKit`): name and samples, no audio.
    pub fn to_mapping_json(&self) -> Value {
        let mut o = Object::new();
        o.insert("name", Value::from(self.name.as_str()));
        o.insert("instruments", Value::Array(self.samples.iter().map(KitSample::to_mapping_json).collect()));
        Value::Object(o)
    }

    /// A kit from a mapping file (`restoreDrumkit`): a name and at least one
    /// entry with a string `url` and `key`; the display name defaults to the url.
    pub fn from_mapping_json(value: &Value) -> Result<Self, String> {
        let name = value.get("name").and_then(Value::as_str);
        let entries = value.get("instruments").and_then(Value::as_array);
        let (Some(name), Some(entries)) = (name, entries) else {
            return Err("Missing drumkit name or instruments".into());
        };
        let instruments: Vec<KitSample> = entries
            .iter()
            .filter(|e| {
                e.get("url").and_then(Value::as_str).is_some() && e.get("key").and_then(Value::as_str).is_some()
            })
            .filter_map(|e| {
                let mut instrument = KitSample::from_json(e)?;
                if e.get("display_name").is_none_or(Value::is_null) {
                    instrument.display_name.clone_from(&instrument.url);
                }
                instrument.extra = Object::new();
                Some(instrument)
            })
            .collect();
        if instruments.is_empty() {
            return Err("No valid instruments".into());
        }
        Ok(Self { name: name.to_owned(), samples: instruments, extra: Object::new() })
    }
}

/// Read drumkits.json (an array of kits); malformed entries are skipped.
pub fn kits_from_json(value: &Value) -> Vec<Kit> {
    value.as_array().unwrap_or_default().iter().filter_map(Kit::from_json).collect()
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
                let buffer = match known(&instrument.url) {
                    Some(buffer) => Ok(buffer),
                    None => read(&instrument.url)
                        .map_err(|e| e.to_string())
                        .and_then(|b| wav::decode(&b).map_err(|e| e.to_string()))
                        .map(Arc::new),
                };
                match buffer {
                    Ok(buffer) => {
                        samples.push(BankSample { kit: kit.name.clone(), sample: instrument.clone(), buffer })
                    }
                    Err(e) => errors.push(format!("{}: {e}", instrument.url)),
                }
            }
        }
        (Self { samples }, errors)
    }

    pub fn into_shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    pub fn find_url(&self, url: &str) -> Option<usize> {
        self.samples.iter().position(|s| s.sample.url == url)
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
    fn kits_and_samples_load() {
        let kits = kits_from_json(
            &json::parse(
                r#"[{"name":"a","instruments":[{"key":"KICK","url":"a/k.wav","decay":5000},{"key":"SNARE","url":"a/s.wav"}]},
                    {"name":"b","instruments":[{"key":"KICK","url":"b/k.wav"},{"key":"CHH","url":"b/h.wav"},{"key":"LO_TOM","url":"b/t.wav"}]},
                    {"instruments":[]}]"#,
            )
            .unwrap(),
        );
        assert_eq!(kits.len(), 2);
        assert_eq!(kits[0].samples[1].decay_ms(), DEFAULT_DECAY_MS);
        assert_eq!(kits[0].samples[0].decay, Some(5000.0));
        let silence = crate::wav::tests::wav(1, 1, 8_000, 16, &[0, 0]);
        let (bank, errors) = SampleBank::load(&kits, |_| Ok(silence.clone()));
        assert!(errors.is_empty());
        assert_eq!(bank.find_url("b/h.wav"), Some(3));
        assert_eq!(bank.samples[3].kit, "b");
    }

    #[test]
    fn load_reports_unreadable_files() {
        let kits = vec![Kit {
            name: "x".into(),
            samples: vec![KitSample {
                key: "K".into(),
                display_name: "k".into(),
                url: "x.wav".into(),
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
        let kits = kits_from_json(&json::parse(text).unwrap());
        assert_eq!(kits_to_json(&kits).to_json(), text);
        assert_eq!(
            kits[0].to_mapping_json().to_json(),
            r#"{"name":"real","instruments":[{"url":"real/kick.wav","display_name":"kick","key":"KICK","rootMidi":72,"peakDb":-3,"decay":5000,"gainDb":0,"tune":0},{"url":"s.wav","display_name":"s","key":"SNARE","rootMidi":null,"peakDb":null,"decay":null,"gainDb":-2.5,"tune":1}]}"#
        );
        let restored = Kit::from_mapping_json(&kits[0].to_mapping_json()).unwrap();
        assert_eq!(restored.samples[0].decay, Some(5000.0));
        assert_eq!(restored.samples[1].gain_db, -2.5);
        let missing = json::parse(r#"{"name":"n","instruments":[{"url":"a.wav"}]}"#).unwrap();
        assert_eq!(Kit::from_mapping_json(&missing), Err("No valid instruments".into()));
        let unnamed = json::parse(r#"{"instruments":[{"url":"a.wav","key":"K"}]}"#).unwrap();
        assert_eq!(Kit::from_mapping_json(&unnamed), Err("Missing drumkit name or instruments".into()));
        let defaults = json::parse(r#"{"name":"n","instruments":[{"url":"a.wav","key":"K"}]}"#).unwrap();
        assert_eq!(Kit::from_mapping_json(&defaults).unwrap().samples[0].display_name, "a.wav");
    }
}
