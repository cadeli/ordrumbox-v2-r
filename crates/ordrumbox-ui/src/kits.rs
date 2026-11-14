//! The drum kits and their editing (`ui/drumkit_manager*.js`,
//! `logic/services/drumkit_service.js`, `audio_import_service.js`): a
//! sample's instrument, kit, gain, tune and decay; samples added, replaced,
//! removed, normalized; folders imported as kits; kit mapping files; a kit
//! exported with its samples to a folder of its own, and imported back.
//!
//! Unlike the JS app, where these edits live in memory only (but for an
//! imported folder), every change is saved: the kits to `kits.json`, the
//! audio of added, replaced or normalized samples to the user's `samples/`
//! folder, which wins over the assets' `kits/`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Component;
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
/// Longest attack and hold a knob sets, ms.
pub const ATTACK_RANGE: (f64, f64) = (0.0, 500.0);
pub const HOLD_RANGE: (f64, f64) = (0.0, 2000.0);

/// What a kit knob changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Gain,
    Transpose,
    Decay,
    /// Where reading starts and ends, ms into the sample.
    Start,
    End,
    Attack,
    Hold,
    /// On / off, set to 1 or 0.
    Reverse,
    Loop,
    Gate,
}

/// The kits, the decoded audio of their samples, and where they are saved.
#[derive(Debug)]
pub struct KitSet {
    kits: Vec<Kit>,
    /// Audio by sample url.
    buffers: HashMap<String, Arc<SampleBuffer>>,
    sources: Option<SampleSources>,
    /// `kits.json`; `None`: kit edits are not saved.
    save_to: Option<PathBuf>,
    samples_dir: Option<PathBuf>,
}

/// `findInstrumentFromFileName(name).id` (`NOT_FOUND` when nothing matches).
fn detect_key(file_name: &str) -> String {
    catalog().from_file_name(file_name).map_or_else(|| NOT_FOUND.to_owned(), |i| i.id.clone())
}

/// The url of a file copied into kit `kit_name`: `imported/<kit>/<file>`.
/// Kept apart per kit, so that two kits' files of the same name do not
/// overwrite each other, and apart from the assets' folders, which the
/// user's `samples/` would otherwise shadow.
fn imported_url(kit_name: &str, file_name: &str) -> String {
    format!("{IMPORTED_DIR}/{}/{file_name}", store::safe_name(kit_name))
}

/// The folder of `samples/` that the imported files are copied to.
const IMPORTED_DIR: &str = "imported";

/// The file of a kit folder that holds the kit: its mapping, each sample's
/// path the name of its file in the folder.
pub const KIT_FILE: &str = "kit.json";

/// `path` when it names a file of the folder itself (one plain component):
/// a kit file never leads out of its folder.
fn plain_file_name(path: &str) -> Option<&str> {
    let mut components = Path::new(path).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Some(path),
        _ => None,
    }
}

/// A file name for the sample of `url` in a kit folder, not one of `taken`
/// (two samples of one name from two folders), then taken.
fn folder_file_name(url: &str, taken: &mut HashSet<String>) -> String {
    let name = Path::new(url).file_name().map_or_else(|| "sample.wav".to_owned(), |n| n.to_string_lossy().into_owned());
    let mut unique = name.clone();
    let mut n = 2;
    while taken.contains(&unique) {
        unique = format!("{n}-{name}");
        n += 1;
    }
    taken.insert(unique.clone());
    unique
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

    /// The file sample `url` plays from; `None` without sample folders (tests).
    pub fn file(&self, url: &str) -> Option<PathBuf> {
        self.sources.as_ref()?.file(url)
    }

    /// The kits, in order. They change through the methods below, which keep
    /// the audio and the files with them.
    pub fn kits(&self) -> &[Kit] {
        &self.kits
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

    /// Set gain (dB), tune (semitones), decay, start, end, attack or hold (ms)
    /// of a sample, from a knob, or its reverse or loop switch (1 or 0); the
    /// message when it changed.
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
            Field::Start if value != instrument.start => {
                instrument.start = value;
                Some(format!("{name}: start {value} ms"))
            }
            Field::End if Some(value) != instrument.end => {
                instrument.end = Some(value);
                Some(format!("{name}: end {value} ms"))
            }
            Field::Attack if Some(value) != instrument.attack => {
                instrument.attack = Some(value);
                Some(format!("{name}: attack {value} ms"))
            }
            Field::Hold if value != instrument.hold => {
                instrument.hold = value;
                Some(format!("{name}: hold {value} ms"))
            }
            Field::Reverse if (value >= 0.5) != instrument.reverse => {
                instrument.reverse = value >= 0.5;
                Some(format!("{name}: reverse {}", if instrument.reverse { "on" } else { "off" }))
            }
            Field::Gate if (value >= 0.5) != instrument.gated => {
                instrument.gated = value >= 0.5;
                Some(format!("{name}: follow note length {}", if instrument.gated { "on" } else { "off" }))
            }
            Field::Loop if (value >= 0.5) != instrument.looping => {
                instrument.looping = value >= 0.5;
                Some(format!("{name}: loop {}", if instrument.looping { "on" } else { "off" }))
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

    /// The instrument of sample `sample` of kit `kit` guessed from its file
    /// name; `Err` when the name gives none.
    pub fn guess_instrument(&self, kit: usize, sample: usize) -> Result<String, String> {
        let Some(s) = self.sample(kit, sample) else { return Err("No such sample".into()) };
        let file = s.path.rsplit_once('/').map_or(s.path.as_str(), |(_, name)| name);
        match detect_key(file) {
            key if key == NOT_FOUND => Err(format!("No instrument in the name \"{file}\"")),
            key => Ok(key),
        }
    }

    /// A text of kit `kit` (its title, its description) by its `kits.json`
    /// key; empty, the key goes. The message when it changed.
    pub fn set_text(&mut self, kit: usize, key: &str, text: &str) -> Option<String> {
        let kit = self.kits.get_mut(kit)?;
        let before = kit.extra.get(key).and_then(|v| v.as_str()).unwrap_or_default();
        if before == text {
            return None;
        }
        if text.is_empty() {
            kit.extra.remove(key);
        } else {
            kit.extra.insert(key, ordrumbox_core::json::Value::from(text));
        }
        Some(format!("Kit \"{}\": {key} changed", kit.name))
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
        let kit_name = self.kits.get(kit).map_or_else(|| "imported".to_owned(), |k| k.name.clone());
        let url = imported_url(&kit_name, &name);
        self.store_bytes(&url, &bytes)?;
        self.buffers.insert(url.clone(), Arc::new(buffer));
        if self.kits.get(kit).is_none() {
            self.kits.push(Kit { name: kit_name.clone(), ..Kit::default() });
        }
        let kit = self.kits.iter().position(|k| k.name == kit_name).unwrap_or(0);
        let instruments = &mut self.kits[kit].samples;
        let entry =
            KitSample { key: detect_key(&name), display_name: name.clone(), path: url.clone(), ..KitSample::default() };
        let index = match instruments.iter().position(|i| i.path == url) {
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

    /// `kit` in the place of the kit of its name, else added; its index.
    fn put_kit(&mut self, kit: Kit) -> usize {
        match self.kits.iter().position(|k| k.name == kit.name) {
            Some(i) => {
                self.kits[i] = kit;
                i
            }
            None => {
                self.kits.push(kit);
                self.kits.len() - 1
            }
        }
    }

    /// `importDirectory`: the WAV files of a folder as a kit named after it
    /// (replacing a kit of that name); its index. A folder with a
    /// [`KIT_FILE`] is a kit exported with its samples: read as it says.
    pub fn import_folder(&mut self, dir: &Path) -> Result<(usize, String), String> {
        if dir.join(KIT_FILE).is_file() {
            return self.import_kit_folder(dir);
        }
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
                    let url = imported_url(&kit_name, &name);
                    self.store_bytes(&url, &bytes)?;
                    self.buffers.insert(url.clone(), Arc::new(buffer));
                    instruments.push(KitSample {
                        key: detect_key(&name),
                        display_name: name,
                        path: url,
                        ..KitSample::default()
                    });
                }
                Err(_) => failed += 1,
            }
        }
        // Each file is read beside the others: a Roland CL is the claves next to its RS.
        let names: Vec<&str> = instruments.iter().map(|i: &KitSample| i.display_name.as_str()).collect();
        let keys: Vec<String> = catalog()
            .from_kit_file_names(&names)
            .iter()
            .map(|i| i.map_or_else(|| NOT_FOUND.to_owned(), |i| i.id.clone()))
            .collect();
        for (instrument, key) in instruments.iter_mut().zip(keys) {
            instrument.key = key;
        }
        let count = instruments.len();
        let index = self.put_kit(Kit { name: kit_name.clone(), samples: instruments, ..Kit::default() });
        let skipped = if failed > 0 { format!(" ({failed} not decoded)") } else { String::new() };
        Ok((index, format!("Imported {count} files into kit \"{kit_name}\"{skipped}")))
    }

    /// `restoreDrumkit`: a kit mapping file, replacing the kit of its name; its index.
    pub fn load_mapping(&mut self, path: &Path) -> Result<(usize, String), String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value = ordrumbox_core::json::parse(&text).map_err(|_| "Invalid drumkit JSON".to_owned())?;
        let kit = Kit::from_mapping_json(&value).map_err(|_| "Invalid drumkit JSON".to_owned())?;
        let name = kit.name.clone();
        let index = self.put_kit(kit);
        Ok((index, format!("Loaded drumkit \"{name}\"")))
    }

    /// A kit folder ([`Self::export_folder`]): the kit of its [`KIT_FILE`],
    /// replacing a kit of that name, its samples copied to the user's
    /// `samples/`; its index. A sample whose file is missing or cannot be
    /// decoded is left out, and said.
    fn import_kit_folder(&mut self, dir: &Path) -> Result<(usize, String), String> {
        let file = dir.join(KIT_FILE);
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let value = ordrumbox_core::json::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut kit = Kit::from_mapping_json(&value).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut stored = HashSet::new();
        let mut left_out = Vec::new();
        let mut samples = Vec::new();
        for mut sample in std::mem::take(&mut kit.samples) {
            let Some(name) = plain_file_name(&sample.path) else {
                left_out.push(sample.path);
                continue;
            };
            let url = imported_url(&kit.name, name);
            if !stored.contains(&url) {
                match Self::decode(&dir.join(name)) {
                    Ok((bytes, buffer)) => {
                        self.store_bytes(&url, &bytes)?;
                        self.buffers.insert(url.clone(), Arc::new(buffer));
                        stored.insert(url.clone());
                    }
                    Err(_) => {
                        left_out.push(sample.path);
                        continue;
                    }
                }
            }
            sample.path = url;
            samples.push(sample);
        }
        if samples.is_empty() {
            return Err(format!("{}: no sample could be read", file.display()));
        }
        kit.samples = samples;
        let (name, count) = (kit.name.clone(), kit.samples.len());
        let index = self.put_kit(kit);
        let left_out =
            if left_out.is_empty() { String::new() } else { format!(" (not read: {})", left_out.join(", ")) };
        Ok((index, format!("Imported kit \"{name}\" with {count} sample(s){left_out}")))
    }

    /// The folder kit `kit` is exported to, with its samples.
    pub fn folder_name(&self, kit: usize) -> Option<String> {
        Some(store::safe_name(&self.kits.get(kit)?.name))
    }

    /// The audio file of sample `url`, as it plays: the user's over the
    /// assets'; without sample folders (tests), the audio held, as WAV.
    fn sample_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        match &self.sources {
            Some(sources) => sources.read(url).map_err(|e| format!("{url}: {e}")),
            None => self.buffers.get(url).map(|b| wav::encode_float(b)).ok_or_else(|| format!("{url}: no audio")),
        }
    }

    /// Kit `kit` with its samples to the new folder `dir`: a [`KIT_FILE`] and
    /// the samples' files, which Import folder as kit reads back. Nothing is
    /// written when a sample cannot be read; a folder half written is removed.
    pub fn export_folder(&self, kit: usize, dir: &Path) -> Result<String, String> {
        let kit = self.kits.get(kit).ok_or("No drumkit selected")?;
        if dir.exists() {
            return Err(format!("{} already exists", dir.display()));
        }
        let mut copy = kit.clone();
        let mut names: HashMap<String, String> = HashMap::new();
        let mut taken = HashSet::new();
        let mut files = Vec::new();
        for sample in &mut copy.samples {
            let name = match names.get(&sample.path) {
                Some(name) => name.clone(),
                None => {
                    let name = folder_file_name(&sample.path, &mut taken);
                    files.push((name.clone(), self.sample_bytes(&sample.path)?));
                    names.insert(sample.path.clone(), name.clone());
                    name
                }
            };
            sample.path = name;
        }
        let write = || -> std::io::Result<()> {
            fs::create_dir_all(dir)?;
            for (name, bytes) in &files {
                fs::write(dir.join(name), bytes)?;
            }
            store::write_atomic(&dir.join(KIT_FILE), copy.to_mapping_json().to_json_pretty(2) + "\n")
        };
        if let Err(e) = write() {
            let cleaned = match fs::remove_dir_all(dir) {
                Ok(()) => String::new(),
                Err(left) => format!(" ({} left half written: {left})", dir.display()),
            };
            return Err(format!("{}: {e}{cleaned}", dir.display()));
        }
        Ok(format!("Exported kit \"{}\" with {} sample(s) to {}", kit.name, files.len(), dir.display()))
    }

    /// The file kit `kit`'s mapping is exported to.
    pub fn mapping_file_name(&self, kit: usize) -> Option<String> {
        Some(format!("ordrumbox-drumkit-{}.json", store::safe_name(&self.kits.get(kit)?.name)))
    }

    /// `exportCurrentKit`: kit `kit`'s mapping (no audio) to `path`.
    pub fn export_mapping(&self, kit: usize, path: &Path) -> Result<String, String> {
        let kit = self.kits.get(kit).ok_or("No drumkit selected")?;
        store::write_atomic(path, kit.to_mapping_json().to_json_pretty(2))
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
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
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
        // The kit's texts: set, unchanged, then gone when empty.
        assert!(kits.set_text(0, "desc", "dry, close-miked").is_some());
        assert_eq!(kits.set_text(0, "desc", "dry, close-miked"), None);
        assert_eq!(kits.kits[0].to_json().as_object().unwrap().get("desc").unwrap().as_str(), Some("dry, close-miked"));
        assert!(kits.set_text(0, "desc", "").is_some());
        assert!(kits.kits[0].extra.get("desc").is_none());

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

        let mapping = dir.join(kits.mapping_file_name(0).unwrap());
        assert!(mapping.ends_with("ordrumbox-drumkit-a.json"));
        kits.export_mapping(0, &mapping).unwrap();
        kits.kits[0].samples.clear();
        assert_eq!(kits.load_mapping(&mapping).unwrap().0, 0);
        assert_eq!(kits.kits[0].samples.len(), 1);
        kits.save().unwrap();
        assert_eq!(store::load_kits(&dir.join("kits.json")).unwrap().unwrap().0[0].name, "a");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imported_files_of_the_same_name_stay_apart() {
        let dir = std::env::temp_dir().join(format!("ordrumbox-kitset-import-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        let mut kits = set(&dir);
        for (folder, level) in [("808", 0.25), ("909", -0.5)] {
            let source = dir.join("sources").join(folder);
            fs::create_dir_all(&source).unwrap();
            fs::write(source.join("kick.wav"), wav::encode_float(&SampleBuffer::mono(vec![level; 4], 8000))).unwrap();
            kits.import_folder(&source).unwrap();
        }
        let file = dir.join("sources").join("808").join("kick.wav");
        kits.add_sample(0, &file).unwrap();

        let urls: Vec<&str> = kits.kits.iter().flat_map(|k| k.samples.iter().map(|s| s.path.as_str())).collect();
        assert_eq!(urls, ["k.wav", "imported/a/kick.wav", "imported/808/kick.wav", "imported/909/kick.wav"]);
        for (url, level) in [("imported/808/kick.wav", 0.25), ("imported/909/kick.wav", -0.5)] {
            assert_eq!(kits.buffer(url).unwrap().left[0], level, "{url}");
            let saved = wav::decode(&fs::read(dir.join("samples").join(url)).unwrap()).unwrap();
            assert_eq!(saved.left[0], level, "{url}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_kit_goes_out_to_a_folder_with_its_samples_and_comes_back() {
        let dir = std::env::temp_dir().join(format!("ordrumbox-kit-folder-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        let mut kits = set(&dir);
        // A second sample of the same file name, from another folder, and the first one twice.
        let snare = Arc::new(SampleBuffer::mono(vec![0.5, -0.25], 8000));
        kits.buffers.insert("other/k.wav".into(), snare.clone());
        let mut second = kits.kits[0].samples[0].clone();
        (second.key, second.path, second.gain_db) = ("SNARE".into(), "other/k.wav".into(), -6.0);
        let again = KitSample { key: "RIM".into(), ..kits.kits[0].samples[0].clone() };
        kits.kits[0].samples.extend([second, again]);
        let kick = kits.buffer("k.wav").unwrap().clone();

        let out = dir.join("out").join("a");
        let message = kits.export_folder(0, &out).unwrap();
        assert_eq!(message, format!("Exported kit \"a\" with 2 sample(s) to {}", out.display()));
        let mut files: Vec<String> = fs::read_dir(&out).unwrap().map(|e| file_name(&e.unwrap().path())).collect();
        files.sort();
        assert_eq!(files, ["2-k.wav", "k.wav", KIT_FILE]);
        let mapping = ordrumbox_core::json::parse(&fs::read_to_string(out.join(KIT_FILE)).unwrap()).unwrap();
        let paths: Vec<String> =
            Kit::from_mapping_json(&mapping).unwrap().samples.into_iter().map(|s| s.path).collect();
        assert_eq!(paths, ["k.wav", "2-k.wav", "k.wav"]);
        assert_eq!(kits.export_folder(0, &out), Err(format!("{} already exists", out.display())));

        // Back in: the kit of its name replaced, the samples copied to the user's
        // samples/, its settings kept.
        kits.kits[0].samples.clear();
        let (index, message) = kits.import_folder(&out).unwrap();
        assert_eq!((index, message.as_str()), (0, "Imported kit \"a\" with 3 sample(s)"));
        let kit = &kits.kits[0];
        let paths: Vec<&str> = kit.samples.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, ["imported/a/k.wav", "imported/a/2-k.wav", "imported/a/k.wav"]);
        assert_eq!((kit.samples[1].key.as_str(), kit.samples[1].gain_db), ("SNARE", -6.0));
        assert_eq!(kits.buffer("imported/a/2-k.wav").unwrap().left, snare.left);
        assert_eq!(kits.buffer("imported/a/k.wav").unwrap().left, kick.left);
        assert!(dir.join("samples").join("imported").join("a").join("2-k.wav").is_file());

        // A kit file naming what is not a file of its folder, or a file not
        // there: left out, and said; nothing readable at all is an error.
        let bad = dir.join("bad");
        fs::create_dir_all(&bad).unwrap();
        fs::copy(out.join("k.wav"), bad.join("k.wav")).unwrap();
        let entry = |path: &str| format!(r#"{{"path":"{path}","key":"KICK"}}"#);
        let kit_file = |entries: &[&str]| {
            let entries: Vec<String> = entries.iter().map(|p| entry(p)).collect();
            fs::write(bad.join(KIT_FILE), format!(r#"{{"name":"bad","samples":[{}]}}"#, entries.join(","))).unwrap();
        };
        kit_file(&["../k.wav", "none.wav", "k.wav"]);
        let (_, message) = kits.import_folder(&bad).unwrap();
        assert_eq!(message, "Imported kit \"bad\" with 1 sample(s) (not read: ../k.wav, none.wav)");
        kit_file(&["../k.wav"]);
        assert!(kits.import_folder(&bad).unwrap_err().ends_with("no sample could be read"));

        // A sample without audio: nothing exported, no folder made.
        kits.kits[0].samples[0].path = "gone.wav".into();
        let none = dir.join("none");
        assert_eq!(kits.export_folder(0, &none), Err("gone.wav: no audio".into()));
        assert!(!none.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
