//! The drum kits and their editing (`ui/drumkit_manager*.js`,
//! `logic/services/drumkit_service.js`, `audio_import_service.js`): a
//! sample's instrument, kit, gain, tune and decay; samples added, replaced,
//! removed, normalized; folders imported as kits; kit mapping files.
//!
//! Unlike the JS app, where these edits live in memory only (but for an
//! imported folder), every change is saved: the kits to `kits.json`, the
//! audio of added, replaced or normalized samples to the user's `samples/`
//! folder, which wins over the assets' `kits/`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordrumbox_core::instruments::{NOT_FOUND, catalog};
use ordrumbox_engine::kit::{Kit, KitSample};
use ordrumbox_engine::{SampleBank, SampleBuffer, analysis, wav};

use crate::assets::SampleSources;
use crate::store;

/// Knob ranges of the JS kit editor (`SOUND_KNOB_DEFS`).
pub const GAIN_RANGE: (f64, f64) = (-24.0, 6.0);
pub const TRANSPOSE_RANGE: (f64, f64) = (-12.0, 12.0);
pub const DECAY_RANGE: (f64, f64) = (20.0, 5000.0);

/// What a kit knob changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Gain,
    Transpose,
    Decay,
}

/// The kits, the decoded audio of their samples, and where they are saved.
#[derive(Debug)]
pub struct KitSet {
    pub kits: Vec<Kit>,
    /// Audio by sample url.
    buffers: HashMap<String, Arc<SampleBuffer>>,
    sources: Option<SampleSources>,
    /// `kits.json`; `None`: kit edits are not saved.
    save_to: Option<PathBuf>,
    samples_dir: Option<PathBuf>,
}

/// `kit.name.replaceAll(/[^a-z0-9_-]/gi, '_')`.
pub fn safe_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// `findInstrumentFromFileName(name).id` (`NOT_FOUND` when nothing matches).
fn detect_key(file_name: &str) -> String {
    catalog().from_file_name(file_name).map_or_else(|| NOT_FOUND.to_owned(), |i| i.id.clone())
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn is_wav(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

impl KitSet {
    pub fn new(
        kits: Vec<Kit>,
        bank: &SampleBank,
        sources: Option<SampleSources>,
        save_to: Option<PathBuf>,
        samples_dir: Option<PathBuf>,
    ) -> Self {
        let buffers = bank.samples.iter().map(|s| (s.sample.path.clone(), s.buffer.clone())).collect();
        Self { kits, buffers, sources, save_to, samples_dir }
    }

    /// No kits (no assets folder).
    pub fn empty() -> Self {
        Self { kits: Vec::new(), buffers: HashMap::new(), sources: None, save_to: None, samples_dir: None }
    }

    /// The bank of the kits as they are now (audio reused, missing samples
    /// read); with a message per sample that could not be loaded.
    pub fn bank(&mut self) -> (SampleBank, Vec<String>) {
        let sources = self.sources.clone();
        let (bank, errors) = SampleBank::rebuild(
            &self.kits,
            |url| self.buffers.get(url).cloned(),
            |url| match &sources {
                Some(s) => s.read(url),
                None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no assets folder")),
            },
        );
        for sample in &bank.samples {
            self.buffers.entry(sample.sample.path.clone()).or_insert_with(|| sample.buffer.clone());
        }
        (bank, errors)
    }

    /// Save the kits (when they may be).
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.save_to else { return Ok(()) };
        store::save_kits(path, &self.kits).map_err(|e| format!("kits not saved: {}: {e}", path.display()))
    }

    pub fn buffer(&self, url: &str) -> Option<&Arc<SampleBuffer>> {
        self.buffers.get(url)
    }

    pub fn sample(&self, kit: usize, sample: usize) -> Option<&KitSample> {
        self.kits.get(kit)?.samples.get(sample)
    }

    /// Keep the audio of `url` in the user's samples folder (as 32-bit float).
    fn store_audio(&self, url: &str, buffer: &SampleBuffer) -> Result<(), String> {
        self.store_bytes(url, &wav::encode_float(buffer))
    }

    fn store_bytes(&self, url: &str, bytes: &[u8]) -> Result<(), String> {
        let Some(dir) = &self.samples_dir else { return Err("no user data folder for samples".into()) };
        let path = store::sample_path(dir, url).ok_or_else(|| format!("{url}: not a sample path"))?;
        store::write_atomic(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    // ─── Sample settings ─────────────────────────────────────────────────────

    /// Where the sample of `url` is: kit and index.
    pub fn find_path(&self, url: &str) -> Option<(usize, usize)> {
        self.kits.iter().enumerate().find_map(|(k, kit)| Some((k, kit.samples.iter().position(|i| i.path == url)?)))
    }

    /// Set gain (dB), tune (semitones) or decay (ms) of a sample, from a
    /// knob; the message when it changed.
    pub fn set_value(&mut self, kit: usize, sample: usize, field: Field, value: f64) -> Option<String> {
        let instrument = self.kits.get_mut(kit)?.samples.get_mut(sample)?;
        let name = instrument.display_name.clone();
        match field {
            Field::Gain if value != instrument.gain_db => {
                instrument.gain_db = value;
                Some(format!("{name}: gain {value:+.1} dB"))
            }
            Field::Transpose if value != instrument.transpose => {
                instrument.transpose = value;
                Some(format!("{name}: tune {value:+.1} st"))
            }
            Field::Decay if Some(value) != instrument.decay => {
                instrument.decay = Some(value);
                Some(format!("{name}: decay {value} ms"))
            }
            _ => None,
        }
    }

    /// `setInstrument`: the instrument a sample is.
    pub fn set_instrument(&mut self, kit: usize, sample: usize, id: &str) -> Option<String> {
        let instrument = self.kits.get_mut(kit)?.samples.get_mut(sample)?;
        (instrument.key != id).then(|| {
            instrument.key = id.to_owned();
            format!("Set \"{}\" to instrument \"{id}\"", instrument.display_name)
        })
    }

    /// `moveToKit`: a sample into kit `target`.
    pub fn move_to_kit(&mut self, kit: usize, sample: usize, target: usize) -> Option<String> {
        if target == kit || target >= self.kits.len() || sample >= self.kits.get(kit)?.samples.len() {
            return None;
        }
        let moved = self.kits[kit].samples.remove(sample);
        let message = format!("Moved \"{}\" to kit \"{}\"", moved.display_name, self.kits[target].name);
        self.kits[target].samples.push(moved);
        Some(message)
    }

    /// `removeSample`.
    pub fn remove(&mut self, kit: usize, sample: usize) -> Option<String> {
        let instruments = &mut self.kits.get_mut(kit)?.samples;
        (sample < instruments.len()).then(|| format!("Removed \"{}\"", instruments.remove(sample).display_name))
    }

    /// `normalizeAll`: the samples of kit `kit` scaled to a 0 dB peak; how many.
    pub fn normalize(&mut self, kit: usize) -> Result<usize, String> {
        let urls: Vec<String> =
            self.kits.get(kit).map(|k| k.samples.iter().map(|i| i.path.clone()).collect()).unwrap_or_default();
        let mut count = 0;
        for url in urls {
            let Some(buffer) = self.buffers.get(&url) else { continue };
            let Some(louder) = analysis::normalized(buffer) else { continue };
            self.store_audio(&url, &louder)?;
            self.buffers.insert(url, Arc::new(louder));
            count += 1;
        }
        Ok(count)
    }

    // ─── Files ───────────────────────────────────────────────────────────────

    fn decode(path: &Path) -> Result<(Vec<u8>, SampleBuffer), String> {
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let buffer = wav::decode(&bytes).map_err(|e| format!("Failed to decode WAV: {e}"))?;
        Ok((bytes, buffer))
    }

    /// `addSample`: a WAV file into kit `kit`, its instrument guessed from
    /// its name; the index of the sample.
    pub fn add_sample(&mut self, kit: usize, path: &Path) -> Result<(usize, String), String> {
        let (bytes, buffer) = Self::decode(path)?;
        let name = file_name(path);
        self.store_bytes(&name, &bytes)?;
        self.buffers.insert(name.clone(), Arc::new(buffer));
        let kit_name = self.kits.get(kit).map_or_else(|| "imported".to_owned(), |k| k.name.clone());
        if self.kits.get(kit).is_none() {
            self.kits.push(Kit { name: kit_name.clone(), ..Kit::default() });
        }
        let kit = self.kits.iter().position(|k| k.name == kit_name).unwrap_or(0);
        let instruments = &mut self.kits[kit].samples;
        let entry = KitSample {
            key: detect_key(&name),
            display_name: name.clone(),
            path: name.clone(),
            ..KitSample::default()
        };
        let index = match instruments.iter().position(|i| i.path == name) {
            Some(i) => {
                instruments[i] = entry;
                i
            }
            None => {
                instruments.push(entry);
                instruments.len() - 1
            }
        };
        Ok((index, format!("Added \"{name}\" to kit \"{kit_name}\"")))
    }

    /// `replaceSampleBuffer`: new audio for sample `sample`, same url.
    pub fn replace_sample(&mut self, kit: usize, sample: usize, path: &Path) -> Result<String, String> {
        let url = self.sample(kit, sample).ok_or("no sample selected")?.path.clone();
        let (_, buffer) = Self::decode(path)?;
        self.store_audio(&url, &buffer)?;
        self.buffers.insert(url, Arc::new(buffer));
        let name = file_name(path);
        if let Some(instrument) = self.kits[kit].samples.get_mut(sample) {
            instrument.display_name.clone_from(&name);
        }
        Ok(format!("Replaced with \"{name}\""))
    }

    /// `importDirectory`: the WAV files of a folder as a kit named after it
    /// (replacing a kit of that name); its index.
    pub fn import_folder(&mut self, dir: &Path) -> Result<(usize, String), String> {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| p.is_file() && is_wav(p))
            .collect();
        if files.is_empty() {
            return Err("No audio files found in selected directory".into());
        }
        files.sort();
        let kit_name = file_name(dir);
        let mut instruments = Vec::new();
        let mut failed = 0;
        for path in &files {
            let name = file_name(path);
            match Self::decode(path) {
                Ok((bytes, buffer)) => {
                    self.store_bytes(&name, &bytes)?;
                    self.buffers.insert(name.clone(), Arc::new(buffer));
                    instruments.push(KitSample {
                        key: detect_key(&name),
                        display_name: name.clone(),
                        path: name,
                        ..KitSample::default()
                    });
                }
                Err(_) => failed += 1,
            }
        }
        let count = instruments.len();
        let kit = Kit { name: kit_name.clone(), samples: instruments, ..Kit::default() };
        let index = match self.kits.iter().position(|k| k.name == kit_name) {
            Some(i) => {
                self.kits[i] = kit;
                i
            }
            None => {
                self.kits.push(kit);
                self.kits.len() - 1
            }
        };
        let skipped = if failed > 0 { format!(" ({failed} not decoded)") } else { String::new() };
        Ok((index, format!("Imported {count} files into kit \"{kit_name}\"{skipped}")))
    }

    /// `restoreDrumkit`: a kit mapping file, replacing the kit of its name; its index.
    pub fn load_mapping(&mut self, path: &Path) -> Result<(usize, String), String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value = ordrumbox_core::json::parse(&text).map_err(|_| "Invalid drumkit JSON".to_owned())?;
        let kit = Kit::from_mapping_json(&value).map_err(|_| "Invalid drumkit JSON".to_owned())?;
        let name = kit.name.clone();
        let index = match self.kits.iter().position(|k| k.name == name) {
            Some(i) => {
                self.kits[i] = kit;
                i
            }
            None => {
                self.kits.push(kit);
                self.kits.len() - 1
            }
        };
        Ok((index, format!("Loaded drumkit \"{name}\"")))
    }

    /// `exportCurrentKit`: kit `kit`'s mapping (no audio) to `dir`.
    pub fn export_mapping(&self, kit: usize, dir: &Path) -> Result<String, String> {
        let kit = self.kits.get(kit).ok_or("No drumkit selected")?;
        let path = dir.join(format!("ordrumbox-drumkit-{}.json", safe_name(&kit.name)));
        store::write_atomic(&path, kit.to_mapping_json().to_json_pretty(2))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(format!("Saved drumkit \"{}\" to {}", kit.name, path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(dir: &Path) -> KitSet {
        let kits = vec![
            Kit {
                name: "a".into(),
                samples: vec![KitSample {
                    key: "KICK".into(),
                    display_name: "kick".into(),
                    path: "k.wav".into(),
                    ..KitSample::default()
                }],
                ..Kit::default()
            },
            Kit { name: "b".into(), ..Kit::default() },
        ];
        let bank = SampleBank {
            samples: vec![ordrumbox_engine::kit::BankSample {
                kit: "a".into(),
                sample: kits[0].samples[0].clone(),
                buffer: Arc::new(SampleBuffer::mono(vec![0.25, -0.5, 0.1], 8000)),
            }],
        };
        KitSet::new(kits, &bank, None, Some(dir.join("kits.json")), Some(dir.join("samples")))
    }

    #[test]
    fn edits_and_files() {
        let dir = std::env::temp_dir().join(format!("ordrumbox-kitset-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut kits = set(&dir);
        assert_eq!(kits.set_value(0, 0, Field::Gain, -3.0).as_deref(), Some("kick: gain -3.0 dB"));
        assert_eq!(kits.set_value(0, 0, Field::Gain, -3.0), None); // no change
        kits.set_value(0, 0, Field::Transpose, 0.3);
        assert_eq!(kits.kits[0].samples[0].transpose, 0.3);
        kits.set_value(0, 0, Field::Decay, 630.0);
        assert_eq!(kits.kits[0].samples[0].decay, Some(630.0));
        assert!(kits.set_instrument(0, 0, "SNARE").is_some());
        assert_eq!(kits.kits[0].samples[0].key, "SNARE");
        assert_eq!(kits.find_path("k.wav"), Some((0, 0)));

        assert_eq!(kits.normalize(0), Ok(1));
        // The gain comes from the peak in dB to 3 decimals, as in the JS: about 0 dB.
        let louder = kits.buffer("k.wav").unwrap().left.clone();
        assert!((louder[1] + 1.0).abs() < 1e-4, "{louder:?}");
        let saved = wav::decode(&fs::read(dir.join("samples").join("k.wav")).unwrap()).unwrap();
        assert_eq!(saved.left, louder);

        let file = dir.join("SnareDrum.wav");
        fs::write(&file, wav::encode_float(&SampleBuffer::mono(vec![0.3; 10], 8000))).unwrap();
        assert_eq!(kits.add_sample(0, &file).unwrap().0, 1);
        assert_eq!(kits.kits[0].samples[1].key, "SNARE");
        let (bank, errors) = kits.bank();
        assert!(errors.is_empty());
        assert_eq!(bank.samples.len(), 2);

        assert_eq!(kits.move_to_kit(0, 1, 1).as_deref(), Some("Moved \"SnareDrum.wav\" to kit \"b\""));
        assert_eq!(kits.move_to_kit(0, 0, 0), None);
        assert_eq!(kits.kits[1].samples.len(), 1);
        assert!(kits.remove(1, 0).is_some());

        kits.export_mapping(0, &dir).unwrap();
        let mapping = dir.join("ordrumbox-drumkit-a.json");
        kits.kits[0].samples.clear();
        assert_eq!(kits.load_mapping(&mapping).unwrap().0, 0);
        assert_eq!(kits.kits[0].samples.len(), 1);
        kits.save().unwrap();
        assert_eq!(store::load_kits(&dir.join("kits.json")).unwrap().unwrap()[0].name, "a");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("My kit/2 é"), "My_kit_2__");
    }
}
