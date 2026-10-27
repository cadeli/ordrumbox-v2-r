//! The user's library file: what the JS app keeps in browser storage, kept
//! here in a `song.json`-format file in the user data folder, written a moment
//! after each edit (and on quit). The assets' `song.json` is only the
//! starting point, never written.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ordrumbox_core::Library;

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
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, library.to_file_text())?;
    fs::rename(&temporary, path)
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
}
