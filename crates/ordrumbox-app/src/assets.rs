//! The `assets` folder (`data/library.json`, `data/kits.json`,
//! `data/scales.json`, `data/synth_presets.json`, `kits/**.wav`), found next
//! to the program, in this repository or given by `ORDRUMBOX_ASSETS`, and
//! loaded into the model.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json;
use ordrumbox_core::{Issue, Library, LoadReport, ScaleList, Scales, Styles};
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
    /// The scale names in file order (the note editor's arpeggio scales).
    pub scale_list: ScaleList,
    /// Synth presets (`synth_presets.json`).
    pub synths: SynthPresets,
    /// The styles the drum generation follows (`styles.json`).
    pub styles: Styles,
    /// Values of `styles.json` that could not be read (their defaults used).
    pub style_issues: Vec<Issue>,
    pub bank: SampleBank,
    /// Samples that could not be read or decoded.
    pub sample_errors: Vec<String>,
    /// Where samples are read from.
    pub sources: SampleSources,
    /// The user's `kits.json` could not be read: the assets' kits are used
    /// and kit edits are not saved (so as not to overwrite it).
    pub kits_error: Option<String>,
    /// Asset files that could not be read (`kits.json`, `scales.json`,
    /// `synth_presets.json`, `styles.json`): what they hold is missing.
    pub errors: Vec<String>,
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
/// program, `assets/` in the working folder, and this repository's
/// `assets/` (for `cargo run` from anywhere in the checkout).
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
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    dirs
}

/// The first candidate holding `data/library.json`.
pub fn find() -> Option<PathBuf> {
    candidates().into_iter().find(|dir| dir.join("data").join(LIBRARY).is_file())
}

fn read_json(path: &Path) -> Result<json::Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The demo library, the starting point of the user's.
pub const LIBRARY: &str = "library.json";

/// Load everything, the user's kits and samples first; only an unreadable
/// `library.json` is fatal.
pub fn load(dir: &Path) -> Result<Assets, String> {
    load_with(dir, store::kits_path(), store::samples_dir())
}

/// [`load`] with the user's `kits.json` and samples folder given (`None`:
/// the assets' only).
fn load_with(dir: &Path, user_kits: Option<PathBuf>, user_samples: Option<PathBuf>) -> Result<Assets, String> {
    let data = dir.join("data");
    let (library, report) = Library::from_json(&read_json(&data.join(LIBRARY))?);
    let mut errors = Vec::new();
    // An asset file that cannot be read: its error reported, `None` used.
    let mut read = |name: &str| read_json(&data.join(name)).map_err(|e| errors.push(e)).ok();
    let saved = user_kits.map(|p| store::load_kits(&p));
    let (kits, kits_error) = match saved {
        Some(Ok(Some(kits))) => (Some(kits), None),
        Some(Err(e)) => (None, Some(e)),
        _ => (None, None),
    };
    let kits = kits.or_else(|| read("kits.json").map(|v| kits_from_json(&v))).unwrap_or_default();
    let scales_json = read("scales.json");
    let synths = read("synth_presets.json").map(|v| SynthPresets::from_json(&v)).unwrap_or_default();
    let scales = scales_json.as_ref().map(Scales::from_json).unwrap_or_default();
    let scale_list = scales_json.as_ref().map(ScaleList::from_json).unwrap_or_default();
    let (styles, style_issues) = read("styles.json").map(|v| Styles::from_json(&v)).unwrap_or_default();
    let sources = SampleSources { assets: dir.join("kits"), user: user_samples };
    let (bank, sample_errors) = SampleBank::load(&kits, |url| sources.read(url));
    Ok(Assets {
        library,
        report,
        kits,
        scales,
        scale_list,
        synths,
        styles,
        style_issues,
        bank,
        sample_errors,
        sources,
        kits_error,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use ordrumbox_engine::TrackSound;
    use ordrumbox_engine::assign::assign_track;

    use super::*;

    /// This repository's `assets/`, without the user's kits and samples.
    fn repository_assets() -> Assets {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        load_with(&dir, None, None).unwrap()
    }

    #[test]
    fn the_assets_load_without_an_error() {
        let assets = repository_assets();
        assert_eq!(assets.errors, Vec::<String>::new());
        assert_eq!(assets.kits_error, None);
        assert_eq!(assets.report.issues, Vec::new());
        assert_eq!(assets.report.dropped_clips, Vec::new());
        assert_eq!(assets.style_issues, Vec::new());
        assert!(!assets.library.patterns.is_empty());
        assert!(!assets.kits.is_empty());
        assert!(!assets.synths.is_empty());
        assert!(!assets.scale_list.names.is_empty());
    }

    #[test]
    fn every_sample_of_the_kits_decodes() {
        let assets = repository_assets();
        assert_eq!(assets.sample_errors, Vec::<String>::new());
        let sample_count: usize = assets.kits.iter().map(|k| k.samples.len()).sum();
        assert_eq!(assets.bank.samples.len(), sample_count);
        for s in &assets.bank.samples {
            assert!(s.buffer.frames() > 0 && s.buffer.sample_rate > 0, "{} is empty", s.sample.path);
        }
    }

    #[test]
    fn every_demo_track_has_a_sound() {
        let assets = repository_assets();
        // The kit the app starts with.
        let kit = &assets.kits[0].name;
        for pattern in &assets.library.patterns {
            for track in &pattern.tracks {
                let sound = assign_track(track, &assets.bank, kit, &assets.synths, &mut |_| 0);
                assert_ne!(sound, TrackSound::None, "pattern {:?}, track {}", pattern.name, track.name);
            }
        }
    }
}
