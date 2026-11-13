//! The user's files: what the JS app keeps in browser storage, kept here in
//! the user data folder — the library (`library.json`, `song.json` format,
//! written a moment after each edit and on quit, with the synth presets
//! edited), the kits once edited
//! (`kits.json`, `drumkits.json` format) and the samples added or changed
//! (`samples/`). The assets are only the starting point, never written.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ordrumbox_core::json;
use ordrumbox_core::{Library, LoadReport};
use ordrumbox_engine::{Kit, KitsRead, SynthPresets, kits_from_json, kits_to_json};

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

/// The library file `$ORDRUMBOX_LIBRARY` names, if it is set.
pub fn env_library() -> Option<PathBuf> {
    env::var_os(ENV_VAR).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Where the library is saved: the file `$ORDRUMBOX_LIBRARY` names, else
/// `library.json` in the folder chosen in the app, else in the default
/// folder. `None` when the folder chosen cannot be used ([`location_problem`]).
pub fn path() -> Option<PathBuf> {
    if let Some(file) = env_library() {
        return Some(file);
    }
    let default = data_dir()?;
    match chosen_dir_in(&default) {
        Ok(Some(dir)) => Some(dir.join(FILE_NAME)),
        Ok(None) => Some(default.join(FILE_NAME)),
        // Reported by `location_problem`, at start.
        Err(_) => None,
    }
}

/// The default user data folder (where the folder chosen in the app is noted).
pub fn default_dir() -> Option<PathBuf> {
    data_dir()
}

/// In the default folder: the user data folder chosen in the app, when it
/// is another one (`{"userFolder": "<path>"}`).
const LOCATION_FILE: &str = "location.json";
const LOCATION_KEY: &str = "userFolder";

/// Why the user data folder chosen in the app cannot be used, if it cannot.
pub fn location_problem() -> Option<String> {
    if env_library().is_some() {
        return None;
    }
    chosen_dir_in(&data_dir()?).err()
}

/// The folder chosen in the app, noted in `default`; `None` when none is.
/// One that is not there any more (a drive not plugged in) is an error:
/// files are never written to a folder made in its place.
fn chosen_dir_in(default: &Path) -> Result<Option<PathBuf>, String> {
    let path = default.join(LOCATION_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let value = json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = value
        .get(LOCATION_KEY)
        .and_then(json::Value::as_str)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| format!("{}: no \"{LOCATION_KEY}\"", path.display()))?;
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err(format!(
            "user data folder {} not found (chosen in the app, noted in {})",
            dir.display(),
            path.display()
        ));
    }
    Ok(Some(dir))
}

/// `a` and `b` are the same folder (compared as written when one cannot be resolved).
pub fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Use `dir` as the user data folder from now on (at the next load): noted
/// in the default folder, or the note removed when `dir` is the default.
pub fn choose_dir(dir: &Path) -> Result<(), String> {
    let default = data_dir().ok_or("no default user data folder to note the choice in")?;
    choose_dir_in(&default, dir)
}

fn choose_dir_in(default: &Path, dir: &Path) -> Result<(), String> {
    let path = default.join(LOCATION_FILE);
    if same_folder(dir, default) {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        };
    }
    let text = dir.to_str().ok_or_else(|| format!("{}: not a UTF-8 path, cannot be noted", dir.display()))?;
    let mut record = json::Object::new();
    record.insert(LOCATION_KEY, json::Value::from(text));
    write_atomic(&path, json::Value::Object(record).to_json_pretty(2) + "\n")
        .map_err(|e| format!("{}: {e}", path.display()))
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

/// The song files of `dir` (`.odbox`), by name; none when `dir` does not
/// exist yet.
pub fn list_songs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut songs = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("odbox")) {
            songs.push(path);
        }
    }
    songs.sort();
    Ok(songs)
}

/// Write a song file (through a temporary file).
pub fn save_song(path: &Path, record: &json::Value) -> io::Result<()> {
    write_atomic(path, &(record.to_json_pretty(2) + "\n"))
}

/// The app settings (`settings.json`: `master`, the master bus; `updates`,
/// the update check).
pub fn settings_path() -> Option<PathBuf> {
    Some(user_dir()?.join("settings.json"))
}

/// Setting `key` of the settings file, if it has one (an unreadable file is an error).
pub fn load_setting(path: &Path, key: &str) -> Result<Option<json::Value>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value = json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(value.get(key).cloned())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write setting `key`, keeping the file's other settings. A file that
/// cannot be read or is not a JSON object is an error, never written over.
pub fn save_setting(path: &Path, key: &str, value: json::Value) -> io::Result<()> {
    let mut settings = match fs::read_to_string(path) {
        Ok(text) => json::parse(&text)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a JSON object, not written over"))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => json::Object::new(),
        Err(e) => return Err(e),
    };
    settings.insert(key, value);
    write_atomic(path, &(json::Value::Object(settings).to_json_pretty(2) + "\n"))
}

/// Where the running app tells `ordrumbox --mcp` how to reach it
/// (`mcp_link.rs`): its port and the token a connection must give.
pub fn mcp_endpoint_path() -> Option<PathBuf> {
    Some(user_dir()?.join("mcp.json"))
}

/// Where earlier versions saved the synth presets edited, for every library:
/// read for a library saved without its own, never written.
pub fn synths_path() -> Option<PathBuf> {
    Some(user_dir()?.join(SYNTHS_FILE))
}

pub const SYNTHS_FILE: &str = "synth_presets.json";
/// Where still older versions saved them: read when there is no
/// `synth_presets.json`.
pub const OLD_SYNTHS_FILE: &str = "generated_sounds.json";

/// The synth presets file to read: the current one, else an older save.
pub fn saved_synths_path() -> Option<PathBuf> {
    let current = synths_path()?;
    let old = current.with_file_name(OLD_SYNTHS_FILE);
    Some(if !current.exists() && old.is_file() { old } else { current })
}

/// The synth presets of a presets file, if there is one (an unreadable file is an error).
pub fn load_synths(path: &Path) -> Result<Option<SynthPresets>, String> {
    match fs::read_to_string(path) {
        Ok(text) => json::parse(&text)
            .map_err(|e| e.to_string())
            .and_then(|v| SynthPresets::from_json(&v))
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write a presets file (the synth editor's export).
pub fn save_synths(path: &Path, synths: &SynthPresets) -> io::Result<()> {
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

/// The saved kits, if there are some, with the kits and samples skipped (to
/// be shown). An unreadable file is an error.
pub fn load_kits(path: &Path) -> Result<Option<KitsRead>, String> {
    match fs::read_to_string(path) {
        Ok(text) => json::parse(&text)
            .map_err(|e| e.to_string())
            .and_then(|v| kits_from_json(&v))
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the kits (through a temporary file).
pub fn save_kits(path: &Path, kits: &[Kit]) -> io::Result<()> {
    write_atomic(path, &(kits_to_json(kits).to_json_pretty(2) + "\n"))
}

/// Write `path` through a temporary file next to it (`<name>.tmp`), renamed
/// over it once complete: a crash never leaves it half written. The folder
/// is made if needed.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path)
}

/// The saved library, if there is one, with what loading it repaired (to be
/// shown). A file that cannot be read or parsed is an error (the caller must
/// not overwrite it).
pub fn load(path: &Path) -> Result<Option<(Library, LoadReport)>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Library::parse(&text).map(Some).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write `library` to `path` through a temporary file, so a crash never
/// leaves half a file.
pub fn save(path: &Path, library: &Library) -> io::Result<()> {
    write_atomic(path, library.to_file_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load() {
        let dir = env::temp_dir().join(format!("ordrumbox-store-{}", std::process::id()));
        let path = dir.join("sub").join(FILE_NAME);
        assert_eq!(load(&path), Ok(None));
        let (library, _) = Library::parse(r#"{"bpm":99,"patterns":[{"name":"p"}]}"#).unwrap();
        save(&path, &library).unwrap();
        assert_eq!(load(&path).unwrap().unwrap().0.bpm, 99.0);
        fs::write(&path, "{").unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn song_files() {
        assert_eq!(safe_name("My song #1 é"), "My_song__1__");
        let dir = env::temp_dir().join(format!("ordrumbox-songs-{}", std::process::id()));
        assert!(list_songs(&dir).unwrap().is_empty());
        save_song(&dir.join("b.odbox"), &json::parse(r#"{"name":"b"}"#).unwrap()).unwrap();
        save_song(&dir.join("a.odbox"), &json::parse(r#"{"name":"a"}"#).unwrap()).unwrap();
        fs::write(dir.join("c.json"), "{}").unwrap();
        assert_eq!(list_songs(&dir).unwrap(), [dir.join("a.odbox"), dir.join("b.odbox")]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_setting_keeps_the_other_settings() {
        let dir = env::temp_dir().join(format!("ordrumbox-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(load_setting(&path, "master"), Ok(None));
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, r#"{"colorScheme":3,"master":{"volume":0.5}}"#).unwrap();
        assert_eq!(load_setting(&path, "master").unwrap().unwrap().to_json(), r#"{"volume":0.5}"#);
        save_setting(&path, "updates", json::Value::from(true)).unwrap();
        assert_eq!(load_setting(&path, "updates"), Ok(Some(json::Value::from(true))));
        assert!(fs::read_to_string(&path).unwrap().contains("\"colorScheme\": 3"));
        fs::write(&path, "{").unwrap();
        assert!(save_setting(&path, "updates", json::Value::Null).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{");
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
    fn the_folder_chosen_is_noted_in_the_default_one() {
        let default = env::temp_dir().join(format!("ordrumbox-location-{}", std::process::id()));
        let chosen = default.join("elsewhere");
        fs::create_dir_all(&chosen).unwrap();
        assert_eq!(chosen_dir_in(&default), Ok(None));
        choose_dir_in(&default, &chosen).unwrap();
        assert_eq!(chosen_dir_in(&default), Ok(Some(chosen.clone())));
        // A folder gone (a drive unplugged) is not made again: an error.
        fs::remove_dir(&chosen).unwrap();
        assert!(chosen_dir_in(&default).unwrap_err().contains("not found"));
        // Back to the default: the note goes.
        choose_dir_in(&default, &default).unwrap();
        assert!(!default.join(LOCATION_FILE).exists());
        assert_eq!(chosen_dir_in(&default), Ok(None));
        fs::write(default.join(LOCATION_FILE), "{").unwrap();
        assert!(chosen_dir_in(&default).is_err());
        fs::remove_dir_all(default).unwrap();
    }
}

#[cfg(test)]
#[path = "store_files.rs"]
mod store_files;
