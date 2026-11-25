//! The drum kits and their editing: a sample's instrument, kit, gain, tune
//! and decay; samples added, replaced, removed, normalized. A kit's files in
//! and out (folders, mapping files) are in `import_export`.
//!
//! Every change is saved: the kits to `kits.json`, the
//! audio of added, replaced or normalized samples to the user's `samples/`
//! folder, which wins over the assets' `kits/`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordrumbox_engine::kit::{Kit, KitSample};
use ordrumbox_engine::{SampleBank, SampleBuffer, analysis, wav};

use crate::assets::SampleSources;
use crate::store;

mod import_export;

/// Knob ranges of the kit editor.
pub const GAIN_RANGE: (f64, f64) = (-24.0, 6.0);
pub const TRANSPOSE_RANGE: (f64, f64) = (-12.0, 12.0);
pub const DECAY_RANGE: (f64, f64) = (20.0, 5000.0);
/// Longest attack and hold a knob sets, ms.
pub const ATTACK_RANGE: (f64, f64) = (0.0, 500.0);
pub const HOLD_RANGE: (f64, f64) = (0.0, 2000.0);

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

/// The url of a file copied into kit `kit_name`: `imported/<kit>/<file>`.
/// Kept apart per kit, so that two kits' files of the same name do not
/// overwrite each other, and apart from the assets' folders, which the
/// user's `samples/` would otherwise shadow.
fn imported_url(kit_name: &str, file_name: &str) -> String {
    format!("{IMPORTED_DIR}/{}/{file_name}", store::safe_name(kit_name))
}

/// The folder of `samples/` that the imported files are copied to.
const IMPORTED_DIR: &str = "imported";

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

pub(crate) fn is_wav(path: &Path) -> bool {
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

    /// Kit `kit`, to edit its texts and samples (`kit::edit`).
    pub fn kit_mut(&mut self, kit: usize) -> Option<&mut Kit> {
        self.kits.get_mut(kit)
    }

    /// The kits, to move samples between them (`kit::edit`).
    pub fn kits_mut(&mut self) -> &mut [Kit] {
        &mut self.kits
    }

    /// Sample `sample` of kit `kit`, to edit its settings (`kit::edit`).
    pub fn sample_mut(&mut self, kit: usize, sample: usize) -> Option<&mut KitSample> {
        self.kits.get_mut(kit)?.samples.get_mut(sample)
    }

    /// The samples of kit `kit` scaled to a 0 dB peak; how many.
    pub fn normalize(&mut self, kit: usize) -> Result<usize, String> {
        let urls: Vec<String> =
            self.kits.get(kit).ok_or("no such kit")?.samples.iter().map(|i| i.path.clone()).collect();
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

    /// A WAV file into kit `kit`, its instrument guessed from
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
        let index = self.kits[kit].put_sample(KitSample::from_file(&url));
        Ok((index, format!("Added \"{name}\" to kit \"{kit_name}\"")))
    }

    /// New audio for sample `sample`, same url.
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
}

#[cfg(test)]
mod tests {
    use ordrumbox_engine::kit::edit::{SampleField, move_sample};

    use super::import_export::KIT_FILE;
    use super::*;

    fn set(dir: &Path) -> KitSet {
        let kits = vec![
            Kit {
                name: "a".into(),
                samples: vec![KitSample {
                    key: Some("KICK".into()),
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
        // The edits themselves are `kit::edit`'s; here, that they reach the saved kits.
        assert!(kits.sample_mut(0, 0).unwrap().set_value(SampleField::Gain, -3.0).is_some());
        assert!(kits.kit_mut(0).unwrap().set_text("desc", "dry, close-miked").is_some());

        assert_eq!(kits.normalize(0), Ok(1));
        // The gain comes from the peak in dB to 3 decimals: about 0 dB.
        let louder = kits.buffer("k.wav").unwrap().left.clone();
        assert!((louder[1] + 1.0).abs() < 1e-4, "{louder:?}");
        let saved = wav::decode(&fs::read(dir.join("samples").join("k.wav")).unwrap()).unwrap();
        assert_eq!(saved.left, louder);

        let file = dir.join("SnareDrum.wav");
        fs::write(&file, wav::encode_float(&SampleBuffer::mono(vec![0.3; 10], 8000))).unwrap();
        assert_eq!(kits.add_sample(0, &file).unwrap().0, 1);
        assert_eq!(kits.kits[0].samples[1].key.as_deref(), Some("SNARE"));
        let (bank, errors) = kits.bank();
        assert!(errors.is_empty());
        assert_eq!(bank.samples.len(), 2);

        assert!(move_sample(kits.kits_mut(), 0, 1, 1).is_some());
        assert!(kits.kit_mut(1).unwrap().remove_sample(0).is_some());

        let mapping = dir.join(kits.mapping_file_name(0).unwrap());
        assert!(mapping.ends_with("ordrumbox-drumkit-a.json"));
        kits.export_mapping(0, &mapping).unwrap();
        kits.kits[0].samples.clear();
        assert_eq!(kits.load_mapping(&mapping).unwrap().0, 0);
        assert_eq!(kits.kits[0].samples.len(), 1);
        kits.save().unwrap();
        let saved = store::load_kits(&dir.join("kits.json")).unwrap().unwrap().0;
        assert_eq!((saved[0].name.as_str(), saved[0].samples[0].gain_db), ("a", -3.0));
        assert_eq!(saved[1].removed, ["imported/a/SnareDrum.wav"], "kept, so that the folder does not add it back");
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
        (second.key, second.path, second.gain_db) = (Some("SNARE".into()), "other/k.wav".into(), -6.0);
        let again = KitSample { key: Some("RIM".into()), ..kits.kits[0].samples[0].clone() };
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
        assert_eq!((kit.samples[1].key.as_deref(), kit.samples[1].gain_db), (Some("SNARE"), -6.0));
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
