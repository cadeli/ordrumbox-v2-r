//! A kit's files in and out: a folder of WAV files imported as a kit, a kit
//! exported with its samples to a folder of its own and imported back, and
//! kit mapping files (no audio).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ordrumbox_engine::kit::edit::{put_kit, set_instruments_from_names};
use ordrumbox_engine::kit::{Kit, KitSample};
use ordrumbox_engine::wav;

use super::{KitSet, file_name, imported_url, is_wav};
use crate::store;

/// The file of a kit folder that holds the kit: its mapping, each sample's
/// path the name of its file in the folder.
pub(super) const KIT_FILE: &str = "kit.json";

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

impl KitSet {
    /// The WAV files of a folder as a kit named after it
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
                    instruments.push(KitSample::from_file(&url));
                }
                Err(_) => failed += 1,
            }
        }
        set_instruments_from_names(&mut instruments);
        let count = instruments.len();
        let index = put_kit(&mut self.kits, Kit { name: kit_name.clone(), samples: instruments, ..Kit::default() });
        let skipped = if failed > 0 { format!(" ({failed} not decoded)") } else { String::new() };
        Ok((index, format!("Imported {count} files into kit \"{kit_name}\"{skipped}")))
    }

    /// A kit mapping file, replacing the kit of its name; its index.
    pub fn load_mapping(&mut self, path: &Path) -> Result<(usize, String), String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value = ordrumbox_core::json::parse(&text).map_err(|_| "Invalid drumkit JSON".to_owned())?;
        let kit = Kit::from_mapping_json(&value).map_err(|_| "Invalid drumkit JSON".to_owned())?;
        let name = kit.name.clone();
        let index = put_kit(&mut self.kits, kit);
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
        let index = put_kit(&mut self.kits, kit);
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

    /// Kit `kit`'s mapping (no audio) to `path`.
    pub fn export_mapping(&self, kit: usize, path: &Path) -> Result<String, String> {
        let kit = self.kits.get(kit).ok_or("No drumkit selected")?;
        store::write_atomic(path, kit.to_mapping_json().to_json_pretty(2))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(format!("Saved drumkit \"{}\" to {}", kit.name, path.display()))
    }
}
