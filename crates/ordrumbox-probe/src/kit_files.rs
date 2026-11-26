//! The kits of a kits folder, read from disk as the app reads them: one folder
//! per kit, its WAV files its samples, described by `kits.json`.

use std::fs;
use std::path::Path;

use ordrumbox_engine::{Kit, KitFolder, KitsRead, kits_from_folders};

const WAV_EXTENSION: &str = ".wav";

/// The WAV files of `folder`, as paths `kit/file` under the kits folder, sorted.
fn kit_folder(folder: &Path) -> Result<KitFolder, String> {
    let name = folder.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let mut paths = Vec::new();
    for entry in fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))? {
        let file = entry.map_err(|e| format!("{}: {e}", folder.display()))?.file_name();
        let file = file.to_string_lossy();
        if file.to_ascii_lowercase().ends_with(WAV_EXTENSION) {
            paths.push(format!("{name}/{file}"));
        }
    }
    paths.sort();
    Ok(KitFolder { name, paths })
}

/// The kits of the folders of `root`, as `described` (the kits of `kits.json`)
/// describes them, and what of them could not be used.
pub fn read_kits(root: &Path, described: &[Kit]) -> Result<KitsRead, String> {
    let mut folders = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))? {
        let path = entry.map_err(|e| format!("{}: {e}", root.display()))?.path();
        if path.is_dir() {
            folders.push(kit_folder(&path)?);
        }
    }
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(kits_from_folders(&folders, described, &[], |path| root.join(path).is_file()))
}
