//! Drum kits (`assets/data/drumkits.json`), their decoded samples, and which
//! sample each track plays.

use std::sync::Arc;

use ordrumbox_core::json::Value;
use ordrumbox_dsp::SampleBuffer;

use crate::wav;

/// Decay used when a kit gives none (`sample.decay ?? 500` ms).
const DEFAULT_DECAY_MS: f64 = 500.0;

/// One sample of a kit, as listed in drumkits.json.
#[derive(Debug, Clone, PartialEq)]
pub struct Instrument {
    /// Matching key: `KICK`, `SNARE`, `CHH`…
    pub key: String,
    pub display_name: String,
    /// Path under `assets/kits/`, also the sample id tracks refer to.
    pub url: String,
    /// Decay to -60 dB, ms.
    pub decay_ms: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kit {
    pub name: String,
    pub instruments: Vec<Instrument>,
}

/// Read drumkits.json (an array of kits); malformed entries are skipped.
pub fn kits_from_json(value: &Value) -> Vec<Kit> {
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    value
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter(|kit| kit.get("name").and_then(Value::as_str).is_some())
        .map(|kit| Kit {
            name: text(kit, "name"),
            instruments: kit
                .get("instruments")
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .filter(|i| i.get("url").and_then(Value::as_str).is_some())
                .map(|i| Instrument {
                    key: text(i, "key"),
                    display_name: text(i, "display_name"),
                    url: text(i, "url"),
                    decay_ms: i.get("decay").and_then(Value::as_f64).unwrap_or(DEFAULT_DECAY_MS),
                })
                .collect(),
        })
        .collect()
}

/// A loaded sample.
#[derive(Debug, Clone, PartialEq)]
pub struct BankSample {
    pub kit: String,
    pub instrument: Instrument,
    pub buffer: SampleBuffer,
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
        let mut samples = Vec::new();
        let mut errors = Vec::new();
        for kit in kits {
            for instrument in &kit.instruments {
                match read(&instrument.url)
                    .map_err(|e| e.to_string())
                    .and_then(|b| wav::decode(&b).map_err(|e| e.to_string()))
                {
                    Ok(buffer) => {
                        samples.push(BankSample { kit: kit.name.clone(), instrument: instrument.clone(), buffer })
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
        self.samples.iter().position(|s| s.instrument.url == url)
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
        assert_eq!(kits[0].instruments[1].decay_ms, DEFAULT_DECAY_MS);
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
            instruments: vec![Instrument {
                key: "K".into(),
                display_name: "k".into(),
                url: "x.wav".into(),
                decay_ms: 1.0,
            }],
        }];
        let (bank, errors) = SampleBank::load(&kits, |_| Ok(b"garbage".to_vec()));
        assert!(bank.samples.is_empty());
        assert_eq!(errors, ["x.wav: not a RIFF/WAVE file"]);
    }
}
