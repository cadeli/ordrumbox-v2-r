//! The `assets` folder of ordrumbox-v2 (`data/song.json`, `data/drumkits.json`,
//! `data/scales.json`, `data/generated_sounds.json`, `kits/**.wav`), found
//! next to the program or given by `ORDRUMBOX_ASSETS`, and loaded into the model.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json;
use ordrumbox_core::{Library, LoadReport, Scales};
use ordrumbox_engine::{Kit, SampleBank, SynthPresets, kits_from_json};

use crate::store;

/// Environment variable naming the assets folder.
pub const ENV_VAR: &str = "ORDRUMBOX_ASSETS";

#[derive(Debug)]
pub struct Assets {
    pub library: Library,
    pub report: LoadReport,
    pub kits: Vec<Kit>,
    pub scales: Scales,
    /// Synth presets (`generated_sounds.json`).
    pub synths: SynthPresets,
    pub bank: SampleBank,
    /// Samples that could not be read or decoded.
    pub sample_errors: Vec<String>,
    /// Where samples are read from.
    pub sources: SampleSources,
    /// The user's `kits.json` could not be read: the assets' kits are used
    /// and kit edits are not saved (so as not to overwrite it).
    pub kits_error: Option<String>,
}

/// Where a sample url is read: the user's samples folder first, then the
/// assets' `kits/`.
#[derive(Debug, Clone)]
pub struct SampleSources {
    pub assets: PathBuf,
    pub user: Option<PathBuf>,
}

impl SampleSources {
    pub fn read(&self, url: &str) -> std::io::Result<Vec<u8>> {
        if let Some(path) = self.user.as_deref().and_then(|dir| store::sample_path(dir, url))
            && path.is_file()
        {
            return fs::read(path);
        }
        match store::sample_path(&self.assets, url) {
            Some(path) => fs::read(path),
            None => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "unsafe sample path")),
        }
    }
}

/// Folders searched, in order: `$ORDRUMBOX_ASSETS`, `assets/` next to the
/// program, `assets/` in the working folder, and a sibling ordrumbox-v2
/// checkout (`../ordrumbox-v2/assets`, for `cargo run` from this repository).
pub fn candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = env::var_os(ENV_VAR) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("assets"));
    }
    if let Ok(cwd) = env::current_dir() {
        dirs.push(cwd.join("assets"));
        dirs.push(cwd.join("..").join("ordrumbox-v2").join("assets"));
    }
    dirs
}

/// The first candidate holding `data/song.json`.
pub fn find() -> Option<PathBuf> {
    candidates().into_iter().find(|dir| dir.join("data").join("song.json").is_file())
}

fn read_json(path: &Path) -> Result<json::Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Load everything; only an unreadable `song.json` is fatal.
pub fn load(dir: &Path) -> Result<Assets, String> {
    let data = dir.join("data");
    let (library, report) = Library::from_json(&read_json(&data.join("song.json"))?);
    let saved = store::kits_path().map(|p| store::load_kits(&p));
    let assets_kits = || read_json(&data.join("drumkits.json")).map(|v| kits_from_json(&v)).unwrap_or_default();
    let (kits, kits_error) = match saved {
        Some(Ok(Some(kits))) => (kits, None),
        Some(Err(e)) => (assets_kits(), Some(e)),
        _ => (assets_kits(), None),
    };
    let scales = read_json(&data.join("scales.json")).map(|v| Scales::from_json(&v)).unwrap_or_default();
    let synths =
        read_json(&data.join("generated_sounds.json")).map(|v| SynthPresets::from_json(&v)).unwrap_or_default();
    let sources = SampleSources { assets: dir.join("kits"), user: store::samples_dir() };
    let (bank, sample_errors) = SampleBank::load(&kits, |url| sources.read(url));
    Ok(Assets { library, report, kits, scales, synths, bank, sample_errors, sources, kits_error })
}
