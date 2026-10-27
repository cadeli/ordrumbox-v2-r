//! Drum kits (`assets/data/drumkits.json`), their decoded samples, and which
//! sample each track plays.

use std::sync::Arc;

use ordrumbox_core::drum_taxonomy::detect_track_type;
use ordrumbox_core::json::Value;
use ordrumbox_core::{Pattern, Track};
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

/// Sample of every track of `pattern` (`None`: nothing to play), preferring kit `kit`.
pub fn assign_samples(pattern: &Pattern, bank: &SampleBank, kit: &str) -> Vec<Option<usize>> {
    pattern.tracks.iter().map(|track| assign_track(track, bank, kit)).collect()
}

/// A simplified `AutoAssign.autoAssignTrackSounds()`:
/// a track with auto-assign off keeps its `sampleId`; otherwise its name (and
/// the name without a `_01`-style suffix) is matched against the instrument
/// keys, first in `kit` then in the other kits, then by coarse type
/// (`detectTrackType`). First match wins (the JS picks at random among them);
/// synth tracks and unmatched names play nothing.
fn assign_track(track: &Track, bank: &SampleBank, kit: &str) -> Option<usize> {
    if track.use_soft_synth {
        return None;
    }
    if !track.use_auto_assign_sound {
        return bank.find_url(&track.sample_id);
    }
    let name = track.name.trim().to_uppercase();
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches(['_', '-', ' ']).to_owned();
    let names = [name.as_str(), base.as_str()];
    let in_kit = |s: &BankSample| s.kit == kit;
    let exact = |s: &BankSample| names.contains(&s.instrument.key.trim().to_uppercase().as_str());
    let kind = detect_track_type(&base);
    let same_kind = |s: &BankSample| detect_track_type(&s.instrument.key) == kind;

    let find = |pred: &dyn Fn(&BankSample) -> bool| bank.samples.iter().position(pred);
    find(&|s| in_kit(s) && exact(s))
        .or_else(|| find(&exact))
        .or_else(|| find(&|s| in_kit(s) && same_kind(s)))
        .or_else(|| find(&same_kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    fn bank() -> SampleBank {
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
        bank
    }

    fn assigned(track: Track, kit: &str) -> Option<String> {
        let bank = bank();
        assign_track(&track, &bank, kit).map(|i| bank.samples[i].instrument.url.clone())
    }

    fn named(name: &str) -> Track {
        Track { name: name.into(), ..Track::default() }
    }

    #[test]
    fn tiers() {
        assert_eq!(assigned(named("KICK"), "b").as_deref(), Some("b/k.wav")); // same kit first
        assert_eq!(assigned(named("kick_01"), "a").as_deref(), Some("a/k.wav")); // suffix dropped
        assert_eq!(assigned(named("CHH"), "a").as_deref(), Some("b/h.wav")); // other kit
        assert_eq!(assigned(named("OHH"), "a").as_deref(), Some("b/h.wav")); // same type (hat)
        assert_eq!(assigned(named("TOM"), "a").as_deref(), Some("b/t.wav")); // perc
    }

    #[test]
    fn explicit_and_synth_tracks() {
        let explicit = Track { use_auto_assign_sound: false, sample_id: "a/s.wav".into(), ..named("KICK") };
        assert_eq!(assigned(explicit, "b").as_deref(), Some("a/s.wav"));
        assert_eq!(assigned(Track { use_soft_synth: true, ..named("KICK") }, "a"), None);
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
