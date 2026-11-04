//! The assets the plugin plays with (`data/*.json`, `kits/**.wav`), found in
//! its bundle, next to its binary, in `ORDRUMBOX_ASSETS` or in this
//! repository.

use std::fs;
use std::path::{Component, Path, PathBuf};

use ordrumbox_core::json;
use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::{SampleBank, SynthPresets, kits_from_json};

/// Environment variable naming the assets folder (the app reads it too).
pub const ENV_VAR: &str = "ORDRUMBOX_ASSETS";

/// The demo library, the plugin's library until a project gives one.
const LIBRARY: &str = "library.json";

/// What the plugin loads once, before it first plays.
#[derive(Debug, Default)]
pub struct Assets {
    pub library: Library,
    pub bank: SampleBank,
    pub scales: Scales,
    pub synths: SynthPresets,
    /// What could not be read: the folder, a data file, a sample.
    pub errors: Vec<String>,
}

/// Folders searched, in order: `$ORDRUMBOX_ASSETS`, `Contents/Resources/assets`
/// of the bundle the binary is in, `assets/` next to the binary, and this
/// repository's `assets/`.
pub fn candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os(ENV_VAR) {
        dirs.push(PathBuf::from(dir));
    }
    // A binary that cannot tell where it is only loses these two candidates.
    if let Some(binary) = process_path::get_dylib_path()
        && let Some(dir) = binary.parent()
    {
        // `X.clap/Contents/MacOS/X`, `X.vst3/Contents/x86_64-win/X.vst3`.
        if let Some(contents) = dir.parent() {
            dirs.push(contents.join("Resources").join("assets"));
        }
        dirs.push(dir.join("assets"));
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    dirs
}

/// The first candidate holding `data/library.json`.
pub fn find() -> Option<PathBuf> {
    candidates().into_iter().find(|dir| dir.join("data").join(LIBRARY).is_file())
}

/// Everything in `dir`; what cannot be read is in `errors` and takes its
/// default.
pub fn load(dir: &Path) -> Assets {
    let data = dir.join("data");
    let mut errors = Vec::new();
    let mut read = |name: &str| match read_json(&data.join(name)) {
        Ok(value) => Some(value),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let library = read(LIBRARY).map(|v| Library::from_json(&v).0).unwrap_or_default();
    let kits = read("kits.json").map(|v| kits_from_json(&v)).unwrap_or_default();
    let scales = read("scales.json").map(|v| Scales::from_json(&v)).unwrap_or_default();
    let synths = read("synth_presets.json").map(|v| SynthPresets::from_json(&v)).unwrap_or_default();
    let samples = dir.join("kits");
    let (bank, sample_errors) = SampleBank::load(&kits, |url| match sample_path(&samples, url) {
        Some(path) => fs::read(path),
        None => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "unsafe sample path")),
    });
    errors.extend(sample_errors);
    Assets { library, bank, scales, synths, errors }
}

/// The assets found, or none with the folders searched as the error.
pub fn find_and_load() -> Assets {
    match find() {
        Some(dir) => load(&dir),
        None => {
            let searched: Vec<String> = candidates().iter().map(|d| d.display().to_string()).collect();
            Assets { errors: vec![format!("no assets folder in {}", searched.join(", "))], ..Assets::default() }
        }
    }
}

fn read_json(path: &Path) -> Result<json::Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// `url` inside `dir`, if it stays inside (no `..`, no absolute path).
fn sample_path(dir: &Path, url: &str) -> Option<PathBuf> {
    let relative = Path::new(url);
    let safe = relative.components().all(|c| matches!(c, Component::Normal(_)));
    (safe && !url.is_empty()).then(|| dir.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_assets_load() {
        let assets = load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        assert_eq!(assets.errors, Vec::<String>::new());
        assert!(!assets.library.patterns.is_empty());
        assert!(!assets.bank.samples.is_empty());
    }

    #[test]
    fn a_sample_path_stays_in_its_folder() {
        assert!(sample_path(Path::new("k"), "a/b.wav").is_some());
        assert!(sample_path(Path::new("k"), "../b.wav").is_none());
        assert!(sample_path(Path::new("k"), "/b.wav").is_none());
        assert!(sample_path(Path::new("k"), "").is_none());
    }
}
