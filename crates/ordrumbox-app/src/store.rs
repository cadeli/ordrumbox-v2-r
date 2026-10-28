//! The user's files: what the JS app keeps in browser storage, kept here in
//! the user data folder — the library (`library.json`, `song.json` format,
//! written a moment after each edit and on quit), the kits once edited
//! (`kits.json`, `drumkits.json` format) and the samples added or changed
//! (`samples/`). The assets are only the starting point, never written.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ordrumbox_core::Library;
use ordrumbox_core::json;
use ordrumbox_engine::{Kit, kits_from_json, kits_to_json};

/// Environment variable naming the library file (overrides the default place).
pub const ENV_VAR: &str = "ORDRUMBOX_LIBRARY";
const FILE_NAME: &str = "library.json";

/// `%APPDATA%\orDrumbox` (Windows), `~/Library/Application Support/orDrumbox`
/// (macOS), `$XDG_DATA_HOME/ordrumbox` or `~/.local/share/ordrumbox` (others).
fn data_dir() -> Option<PathBuf> {
    let var = |name: &str| env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var("APPDATA").map(|d| d.join("orDrumbox"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support").join("orDrumbox"))
    } else {
        var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
            .map(|d| d.join("ordrumbox"))
    }
}

/// Where the library is saved.
pub fn path() -> Option<PathBuf> {
    env::var_os(ENV_VAR).filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| Some(data_dir()?.join(FILE_NAME)))
}

/// Where WAV exports go: `exports` in the user data folder (next to the
/// library file when `$ORDRUMBOX_LIBRARY` names one).
pub fn exports_dir() -> Option<PathBuf> {
    Some(path()?.parent()?.join("exports"))
}

/// The user data folder (the library's folder).
pub fn user_dir() -> Option<PathBuf> {
    Some(path()?.parent()?.to_path_buf())
}

/// Where the song panel saves songs (the JS IndexedDB `songs` store).
pub fn songs_dir() -> Option<PathBuf> {
    Some(user_dir()?.join("songs"))
}

/// A song name as a file name (`exportToFile`: what is not a letter, a
/// digit, `_` or `-` becomes `_`).
pub fn safe_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// The song files of `dir` (`.odbox`), by name.
pub fn list_songs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut songs: Vec<PathBuf> = entries
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("odbox")))
        .collect();
    songs.sort();
    songs
}

/// Write a song file (through a temporary file).
pub fn save_song(path: &Path, record: &json::Value) -> io::Result<()> {
    write_atomic(path, &(record.to_json_pretty(2) + "\n"))
}

/// The app settings (`settings.json`: `settings.master`, the master bus).
pub fn settings_path() -> Option<PathBuf> {
    Some(user_dir()?.join("settings.json"))
}

/// The master bus settings saved, if there are some (an unreadable file is an error).
pub fn load_master(path: &Path) -> Result<Option<ordrumbox_engine::MasterSettings>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value = json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(value.get("master").map(crate::output::from_json))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the master bus settings, keeping the file's other settings.
pub fn save_master(path: &Path, master: &ordrumbox_engine::MasterSettings) -> io::Result<()> {
    let mut settings = fs::read_to_string(path)
        .ok()
        .and_then(|text| json::parse(&text).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    settings.insert("master", crate::output::to_json(master));
    write_atomic(path, &(json::Value::Object(settings).to_json_pretty(2) + "\n"))
}

/// The synth presets, once edited (`generated_sounds.json` format).
pub fn synths_path() -> Option<PathBuf> {
    Some(user_dir()?.join("generated_sounds.json"))
}

/// The synth presets saved, if there are some (an unreadable file is an error).
pub fn load_synths(path: &Path) -> Result<Option<ordrumbox_engine::SynthPresets>, String> {
    match fs::read_to_string(path) {
        Ok(text) => json::parse(&text)
            .map(|v| Some(ordrumbox_engine::SynthPresets::from_json(&v)))
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the synth presets.
pub fn save_synths(path: &Path, synths: &ordrumbox_engine::SynthPresets) -> io::Result<()> {
    write_atomic(path, &(synths.to_json().to_json_pretty(2) + "\n"))
}

/// The kits, once edited.
pub fn kits_path() -> Option<PathBuf> {
    Some(user_dir()?.join("kits.json"))
}

/// Samples added, replaced or normalized, by url (they win over the assets').
pub fn samples_dir() -> Option<PathBuf> {
    Some(user_dir()?.join("samples"))
}

/// Where a sample url lives under `dir`: `None` for an absolute url or one
/// going up (`..`), never written.
pub fn sample_path(dir: &Path, url: &str) -> Option<PathBuf> {
    let relative = Path::new(url);
    let safe = relative.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    (safe && !url.is_empty()).then(|| dir.join(relative))
}

/// The saved kits, if there are some (an unreadable file is an error).
pub fn load_kits(path: &Path) -> Result<Option<Vec<Kit>>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            json::parse(&text).map(|v| Some(kits_from_json(&v))).map_err(|e| format!("{}: {e}", path.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the kits (through a temporary file).
pub fn save_kits(path: &Path, kits: &[Kit]) -> io::Result<()> {
    write_atomic(path, &(kits_to_json(kits).to_json_pretty(2) + "\n"))
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

/// The saved library, if there is one. A file that cannot be read or parsed
/// is an error (the caller must not overwrite it).
pub fn load(path: &Path) -> Result<Option<Library>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            Library::parse(&text).map(|(library, _)| Some(library)).map_err(|e| format!("{}: {e}", path.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write `library` to `path` through a temporary file, so a crash never
/// leaves half a file.
pub fn save(path: &Path, library: &Library) -> io::Result<()> {
    write_atomic(path, &library.to_file_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load() {
        let dir = env::temp_dir().join(format!("ordrumbox-store-{}", std::process::id()));
        let path = dir.join("sub").join(FILE_NAME);
        assert_eq!(load(&path), Ok(None));
        let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","bpm":99}]}"#).unwrap();
        save(&path, &library).unwrap();
        assert_eq!(load(&path).unwrap().unwrap().patterns[0].bpm, 99.0);
        fs::write(&path, "{").unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn song_files() {
        assert_eq!(safe_name("My song #1 é"), "My_song__1__");
        let dir = env::temp_dir().join(format!("ordrumbox-songs-{}", std::process::id()));
        assert!(list_songs(&dir).is_empty());
        save_song(&dir.join("b.odbox"), &json::parse(r#"{"name":"b"}"#).unwrap()).unwrap();
        save_song(&dir.join("a.odbox"), &json::parse(r#"{"name":"a"}"#).unwrap()).unwrap();
        fs::write(dir.join("c.json"), "{}").unwrap();
        assert_eq!(list_songs(&dir), [dir.join("a.odbox"), dir.join("b.odbox")]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn master_settings_keep_the_other_settings() {
        let dir = env::temp_dir().join(format!("ordrumbox-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(load_master(&path), Ok(None));
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, r#"{"colorScheme":3,"master":{"volume":0.5}}"#).unwrap();
        let mut master = load_master(&path).unwrap().unwrap();
        assert_eq!((master.master, master.threshold), (0.5, -18.0));
        master.threshold = -30.0;
        save_master(&path, &master).unwrap();
        assert_eq!(load_master(&path).unwrap().unwrap(), master);
        assert!(fs::read_to_string(&path).unwrap().contains("\"colorScheme\": 3"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sample_paths_stay_inside() {
        let dir = Path::new("/data/samples");
        assert_eq!(sample_path(dir, "real/kick.wav"), Some(dir.join("real").join("kick.wav")));
        assert_eq!(sample_path(dir, "../x.wav"), None);
        assert_eq!(sample_path(dir, "/etc/x.wav"), None);
        assert_eq!(sample_path(dir, ""), None);
    }

    #[test]
    fn kits_save_then_load() {
        let dir = env::temp_dir().join(format!("ordrumbox-kits-{}", std::process::id()));
        let path = dir.join("kits.json");
        assert_eq!(load_kits(&path), Ok(None));
        let kits = kits_from_json(
            &json::parse(r#"[{"name":"a","instruments":[{"key":"KICK","url":"k.wav","gainDb":-3}]}]"#).unwrap(),
        );
        save_kits(&path, &kits).unwrap();
        assert_eq!(load_kits(&path).unwrap().unwrap(), kits);
        fs::remove_dir_all(dir).unwrap();
    }
}
