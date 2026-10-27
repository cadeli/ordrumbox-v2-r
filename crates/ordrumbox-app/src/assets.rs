//! The `assets` folder of ordrumbox-v2 (`data/song.json`, `data/drumkits.json`,
//! `data/scales.json`, `kits/**.wav`), found next to the program or given by
//! `ORDRUMBOX_ASSETS`, and loaded into the model.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json;
use ordrumbox_core::{Library, LoadReport, Scales};
use ordrumbox_engine::{Kit, SampleBank, kits_from_json};

/// Environment variable naming the assets folder.
pub const ENV_VAR: &str = "ORDRUMBOX_ASSETS";

#[derive(Debug)]
pub struct Assets {
    pub library: Library,
    pub report: LoadReport,
    pub kits: Vec<Kit>,
    pub scales: Scales,
    pub bank: SampleBank,
    /// Samples that could not be read or decoded.
    pub sample_errors: Vec<String>,
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
    let kits = read_json(&data.join("drumkits.json")).map(|v| kits_from_json(&v)).unwrap_or_default();
    let scales = read_json(&data.join("scales.json")).map(|v| Scales::from_json(&v)).unwrap_or_default();
    let kits_dir = dir.join("kits");
    let (bank, sample_errors) = SampleBank::load(&kits, |url| fs::read(kits_dir.join(url)));
    Ok(Assets { library, report, kits, scales, bank, sample_errors })
}
