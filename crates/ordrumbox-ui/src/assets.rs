//! The `assets` folder (`data/library.json`, `data/demos/*.json`, `data/kits.json`,
//! `data/scales.json`, `data/synth_presets.json`, `kits/**.wav`), found next
//! to the program, in this repository or given by `ORDRUMBOX_ASSETS`, and
//! loaded into the model.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use ordrumbox_core::json;
use ordrumbox_core::{Issue, Library, LoadReport, ScaleList, Scales, Styles};
use ordrumbox_engine::{Kit, KitFolder, SampleBank, SynthPresets, kits_from_folders, kits_from_json};
use ordrumbox_logic::demo_catalog::DemoStyle;

use crate::kits::is_wav;
use crate::store;

/// Environment variable naming the assets folder.
pub const ENV_VAR: &str = "ORDRUMBOX_ASSETS";

#[derive(Debug)]
pub struct Assets {
    pub library: Library,
    pub report: LoadReport,
    /// The demos, a style per file of `data/demos/`, in file name order.
    pub demos: Vec<DemoStyle>,
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
    /// Kits and samples left out, by source: the user's `kits.json` and the
    /// assets' (malformed), `kits/` (no such file, no folder).
    pub kit_issues: Vec<(String, Vec<Issue>)>,
    /// Asset files that could not be read (`kits.json`, `scales.json`,
    /// `synth_presets.json`, `styles.json`, `demos/`): what they hold is
    /// missing; a demo file with values repaired.
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
        match self.file(url) {
            Some(path) => fs::read(path),
            None => Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "unsafe sample path")),
        }
    }

    /// The file sample `url` is read from: the user's, else the assets'
    /// (which may not exist); `None` for a path outside both folders.
    pub fn file(&self, url: &str) -> Option<PathBuf> {
        if let Some(path) = self.user.as_deref().and_then(|dir| store::sample_path(dir, url))
            && path.is_file()
        {
            return Some(path);
        }
        store::sample_path(&self.assets, url)
    }
}

/// Folders searched, in order: `first`, `$ORDRUMBOX_ASSETS`, `assets/` next
/// to the program, `assets/` in the working folder, and this repository's
/// `assets/` (for `cargo run` from anywhere in the checkout).
pub fn candidates(first: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = first.to_vec();
    if let Some(dir) = env::var_os(ENV_VAR) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("assets"));
    }
    if let Ok(cwd) = env::current_dir() {
        dirs.push(cwd.join("assets"));
    }
    // This crate is `crates/ordrumbox-ui`: two folders under the checkout.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    dirs.push(manifest.ancestors().nth(2).unwrap_or(manifest).join("assets"));
    dirs
}

/// The first candidate holding `data/library.json`.
pub fn find(first: &[PathBuf]) -> Option<PathBuf> {
    candidates(first).into_iter().find(|dir| dir.join("data").join(LIBRARY).is_file())
}

fn read_json(path: &Path) -> Result<json::Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Asset file `name` of `data`, read and decoded; `None` when it cannot be,
/// the error pushed on `errors`.
fn read_asset<T>(
    data: &Path,
    name: &str,
    errors: &mut Vec<String>,
    decode: impl FnOnce(json::Value) -> Result<T, String>,
) -> Option<T> {
    let path = data.join(name);
    read_json(&path)
        .and_then(|v| decode(v).map_err(|e| format!("{}: {e}", path.display())))
        .map_err(|e| errors.push(e))
        .ok()
}

/// The starter library, the starting point of the user's.
pub const LIBRARY: &str = "library.json";

/// The folder of `data` holding the demos, a style per file.
pub const DEMOS: &str = "demos";

/// The demo styles of `data/demos/`, in file name order (the file names
/// start with their place); what cannot be read is pushed on `errors`.
fn load_demos(data: &Path, errors: &mut Vec<String>) -> Vec<DemoStyle> {
    let folder = data.join(DEMOS);
    let mut paths: Vec<PathBuf> = match fs::read_dir(&folder) {
        Ok(entries) => entries
            .filter_map(|entry| {
                entry.map(|e| e.path()).map_err(|e| errors.push(format!("{}: {e}", folder.display()))).ok()
            })
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect(),
        Err(e) => {
            errors.push(format!("{}: {e}", folder.display()));
            return Vec::new();
        }
    };
    paths.sort();
    let mut styles = Vec::new();
    for path in paths {
        let Ok(value) = read_json(&path).map_err(|e| errors.push(e)) else { continue };
        let (library, report) = Library::from_json(&value);
        let repaired = report.issues.len() + report.dropped_clips.len();
        if repaired > 0 {
            errors.push(format!("{}: {repaired} value(s) repaired", path.display()));
        }
        let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        styles.push(DemoStyle::new(library, &stem));
    }
    styles
}

/// The kits as loaded, with what went wrong.
struct LoadedKits {
    kits: Vec<Kit>,
    /// The user's `kits.json` could not be read.
    kits_error: Option<String>,
    /// What each source left out.
    kit_issues: Vec<(String, Vec<Issue>)>,
}

/// The kits: the folders of `kits/` (`sources.assets`) described by
/// `data/kits.json` and the user's `kits.json`
/// ([`ordrumbox_engine::kits_from_folders`]).
fn load_kits(data: &Path, user_kits: Option<PathBuf>, sources: &SampleSources, errors: &mut Vec<String>) -> LoadedKits {
    let mut kit_issues = Vec::new();
    let (edited, kits_error) = match user_kits.map(|p| (store::load_kits(&p), p)) {
        Some((Ok(Some((kits, issues))), path)) => {
            kit_issues.push((path.display().to_string(), issues));
            (kits, None)
        }
        Some((Err(e), _)) => (Vec::new(), Some(e)),
        _ => (Vec::new(), None),
    };
    let shipped =
        read_asset(data, "kits.json", errors, |v| kits_from_json(&v)).map_or_else(Vec::new, |(kits, issues)| {
            kit_issues.push(("kits.json".to_owned(), issues));
            kits
        });
    let folders = kit_folders(&sources.assets).unwrap_or_else(|e| {
        errors.push(format!("{}: {e}", sources.assets.display()));
        Vec::new()
    });
    let exists = |url: &str| sources.file(url).is_some_and(|path| path.is_file());
    let (kits, left_out) = kits_from_folders(&folders, &shipped, &edited, exists);
    kit_issues.push(("kits/".to_owned(), left_out));
    LoadedKits { kits, kits_error, kit_issues }
}

/// The folders of `kits` (hidden ones aside), by name, each with its WAV
/// files as sample paths (`kit/file.wav`), by name.
pub fn kit_folders(kits: &Path) -> io::Result<Vec<KitFolder>> {
    let mut folders = Vec::new();
    for entry in fs::read_dir(kits)? {
        let path = entry?.path();
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !path.is_dir() || name.starts_with('.') {
            continue;
        }
        let mut paths = Vec::new();
        for file in fs::read_dir(&path)? {
            let file = file?.path();
            if file.is_file()
                && is_wav(&file)
                && let Some(file_name) = file.file_name()
            {
                paths.push(format!("{name}/{}", file_name.to_string_lossy()));
            }
        }
        paths.sort();
        folders.push(KitFolder { name, paths });
    }
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

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
    let demos = load_demos(&data, &mut errors);
    let sources = SampleSources { assets: dir.join("kits"), user: user_samples };
    let LoadedKits { kits, kits_error, kit_issues } = load_kits(&data, user_kits, &sources, &mut errors);
    let scales_json = read_asset(&data, "scales.json", &mut errors, Ok);
    let synths =
        read_asset(&data, "synth_presets.json", &mut errors, |v| SynthPresets::from_json(&v)).unwrap_or_default();
    let scales = scales_json.as_ref().map(Scales::from_json).unwrap_or_default();
    let scale_list = scales_json.as_ref().map(ScaleList::from_json).unwrap_or_default();
    let (styles, style_issues) =
        read_asset(&data, "styles.json", &mut errors, |v| Ok(Styles::from_json(&v))).unwrap_or_default();
    let (bank, sample_errors) = SampleBank::load(&kits, |url| sources.read(url));
    Ok(Assets {
        library,
        report,
        demos,
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
        kit_issues,
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
        assert!(assets.kit_issues.iter().all(|(_, issues)| issues.is_empty()), "{:#?}", assets.kit_issues);
        assert_eq!(assets.report.issues, Vec::new());
        assert_eq!(assets.report.dropped_clips, Vec::new());
        assert_eq!(assets.style_issues, Vec::new());
        assert!(!assets.library.patterns.is_empty());
        assert!(assets.demos.iter().all(|style| !style.library().patterns.is_empty()));
        assert!(!assets.demos.is_empty() && !assets.kits.is_empty());
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
        let demos = assets.demos.iter().map(DemoStyle::library);
        for pattern in std::iter::once(&assets.library).chain(demos).flat_map(|library| &library.patterns) {
            for track in &pattern.tracks {
                let sound = assign_track(track, &assets.bank, kit, &assets.synths, &mut |_| 0);
                assert_ne!(sound, TrackSound::None, "pattern {:?}, track {}", pattern.name, track.name);
            }
        }
    }

    #[test]
    fn the_starter_library_is_among_the_demos() {
        let assets = repository_assets();
        let in_demos = |id: &str| assets.demos.iter().any(|style| style.library().pattern(id).is_some());
        for pattern in &assets.library.patterns {
            assert!(in_demos(&pattern.id), "starter pattern {} is in no demo style", pattern.id);
        }
        for song in &assets.library.songs {
            assert!(
                assets.demos.iter().any(|style| style.library().songs.iter().any(|s| s.id == song.id)),
                "{}",
                song.id
            );
        }
    }

    #[test]
    fn no_two_demos_share_an_id() {
        let assets = repository_assets();
        let mut ids = std::collections::HashSet::new();
        for library in assets.demos.iter().map(DemoStyle::library) {
            for id in library.patterns.iter().map(|p| &p.id).chain(library.songs.iter().map(|s| &s.id)) {
                assert!(ids.insert(id.clone()), "{id} is in two demo files");
            }
        }
    }
}
