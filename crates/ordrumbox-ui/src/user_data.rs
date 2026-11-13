//! The user data folder as a whole: its files backed up to a folder of
//! their own and restored from one, moved aside before a factory reset or a
//! restore, copied to another folder chosen in the app.
//!
//! The user's work is [`FILES`] and [`FOLDERS`]. The rest of the folder is
//! not: `mcp.json` is the running app's address, `exports/` what the app
//! wrote out, `backups/` the files moved aside.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

use crate::store;

/// The files of the user's work, in the user data folder.
pub const FILES: [&str; 5] = ["library.json", "kits.json", "settings.json", store::SYNTHS_FILE, store::OLD_SYNTHS_FILE];
/// The folders of the user's work, in the user data folder.
pub const FOLDERS: [&str; 2] = ["samples", "songs"];
/// Where, in the user data folder, the files moved aside go.
pub const BACKUPS_DIR: &str = "backups";
/// The start of the name of a backup's folder.
const BACKUP_PREFIX: &str = "orDrumbox-backup-";

/// The user's files found in `dir`, by name (a folder's ends in `/`).
pub fn present(dir: &Path) -> Vec<String> {
    let files = FILES.iter().filter(|name| dir.join(name).is_file()).map(|name| (*name).to_owned());
    let folders = FOLDERS.iter().filter(|name| dir.join(name).is_dir()).map(|name| format!("{name}/"));
    files.chain(folders).collect()
}

/// Copy the user's files of `from` into `to` (made if needed), over the
/// ones of the same name; their names.
pub fn copy_files(from: &Path, to: &Path) -> io::Result<Vec<String>> {
    let names = present(from);
    for name in &names {
        let name = name.trim_end_matches('/');
        copy(&from.join(name), &to.join(name))?;
    }
    Ok(names)
}

/// Copy file or folder `from` to `to`, folders with everything inside.
fn copy(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        store::write_atomic(to, fs::read(from)?)
    }
}

/// Back the user's files of `dir` up to folder `to`, which must not exist
/// yet; their names. Nothing to back up is an error, and no folder is made.
pub fn back_up(dir: &Path, to: &Path) -> io::Result<Vec<String>> {
    if present(dir).is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("no user files in {}", dir.display())));
    }
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", to.display())));
    }
    copy_files(dir, to)
}

/// The name of the folder of a backup made at `stamp`.
pub fn backup_name(stamp: &str) -> String {
    format!("{BACKUP_PREFIX}{stamp}")
}

/// The user's files moved aside: the folder they are in now, and their names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedAside {
    pub folder: PathBuf,
    pub names: Vec<String>,
}

/// Move the user's files of `dir` to `backups/<stamp>-<why>` in `dir`
/// (another name if that one is taken): the app then starts as at the
/// first start, and nothing is lost. `None` when there were none.
pub fn move_aside(dir: &Path, stamp: &str, why: &str) -> io::Result<Option<MovedAside>> {
    let names = present(dir);
    if names.is_empty() {
        return Ok(None);
    }
    let backups = dir.join(BACKUPS_DIR);
    let mut folder = backups.join(format!("{stamp}-{why}"));
    let mut n = 2;
    while folder.exists() {
        folder = backups.join(format!("{stamp}-{why}-{n}"));
        n += 1;
    }
    fs::create_dir_all(&folder)?;
    for name in &names {
        let name = name.trim_end_matches('/');
        // Inside the same folder: a rename, never a copy.
        fs::rename(dir.join(name), folder.join(name))?;
    }
    Ok(Some(MovedAside { folder, names }))
}

/// What a restore did: the files that were there, moved aside, and the names restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub aside: Option<MovedAside>,
    pub names: Vec<String>,
}

/// Put the user's files of `backup` in the place of those of `dir`, which
/// are moved aside first ([`move_aside`], `before-restore`). A folder
/// without user files, or `dir` itself, is refused and nothing is moved.
pub fn restore(backup: &Path, dir: &Path, stamp: &str) -> Result<Restored, String> {
    if store::same_folder(backup, dir) {
        return Err("this is the user data folder itself, not a backup".into());
    }
    if present(backup).is_empty() {
        return Err(format!("no orDrumbox user files in {}", backup.display()));
    }
    let aside = move_aside(dir, stamp, "before-restore").map_err(|e| format!("{}: {e}", dir.display()))?;
    let names = copy_files(backup, dir).map_err(|e| match &aside {
        Some(aside) => format!("{e} (the files before it are in {})", aside.folder.display()),
        None => e.to_string(),
    })?;
    Ok(Restored { aside, names })
}

/// Now, as [`stamp`] writes it.
pub fn now_stamp() -> String {
    stamp(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

/// `secs` after 1970-01-01 UTC as `YYYYMMDD-HHMMSS` (UTC): names that sort by date.
pub fn stamp(secs: u64) -> String {
    let (days, time) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_date(days as i64);
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", time / 3600, time / 60 % 60, time % 60)
}

/// The date `days` after 1970-01-01 (proleptic Gregorian calendar).
fn civil_date(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's `civil_from_days`: eras of 400 years from 0000-03-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for test `name`, empty.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ordrumbox-user-data-{name}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A user data folder with every kind of file, and what is not the user's work.
    fn user_folder(name: &str) -> PathBuf {
        let dir = folder(name);
        fs::create_dir_all(dir.join("samples").join("imported").join("a")).unwrap();
        fs::create_dir_all(dir.join("songs")).unwrap();
        fs::create_dir_all(dir.join("exports")).unwrap();
        fs::write(dir.join("library.json"), "{\"mine\":1}").unwrap();
        fs::write(dir.join("kits.json"), "[]").unwrap();
        fs::write(dir.join("settings.json"), "{}").unwrap();
        fs::write(dir.join("generated_sounds.json"), "{}").unwrap();
        fs::write(dir.join("mcp.json"), "{}").unwrap();
        fs::write(dir.join("samples").join("imported").join("a").join("kick.wav"), "wav").unwrap();
        fs::write(dir.join("songs").join("a.odbox"), "{}").unwrap();
        fs::write(dir.join("exports").join("e.wav"), "e").unwrap();
        dir
    }

    #[test]
    fn stamps_are_utc_dates_that_sort() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(951_782_400), "20000229-000000");
        assert_eq!(stamp(1_793_884_505), "20261105-131505");
        assert_eq!(stamp(4_107_542_399), "21000228-235959");
        assert!(stamp(1_000_000_000) < stamp(1_000_000_001));
    }

    #[test]
    fn a_backup_holds_the_users_work_and_nothing_else() {
        let dir = user_folder("backup");
        let to = dir.join("out").join(backup_name("20261105-115505"));
        let names = back_up(&dir, &to).unwrap();
        assert_eq!(
            names,
            ["library.json", "kits.json", "settings.json", "generated_sounds.json", "samples/", "songs/"]
        );
        assert_eq!(fs::read_to_string(to.join("library.json")).unwrap(), "{\"mine\":1}");
        assert_eq!(fs::read_to_string(to.join("samples/imported/a/kick.wav")).unwrap(), "wav");
        assert!(to.join("songs").join("a.odbox").is_file());
        assert!(!to.join("mcp.json").exists() && !to.join("exports").exists());
        // The folder is left as it was.
        assert_eq!(present(&dir), names);
        // Never over another backup, never an empty one.
        assert_eq!(back_up(&dir, &to).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        let empty = folder("backup-empty");
        assert_eq!(back_up(&empty, &empty.join("b")).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert!(!empty.join("b").exists());
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(empty).unwrap();
    }

    #[test]
    fn files_moved_aside_are_kept_in_backups() {
        let dir = user_folder("aside");
        let aside = move_aside(&dir, "20261105-115505", "reset").unwrap().unwrap();
        assert_eq!(aside.folder, dir.join("backups").join("20261105-115505-reset"));
        assert_eq!(aside.names.len(), 6);
        assert!(present(&dir).is_empty());
        assert_eq!(present(&aside.folder), aside.names);
        // What is not the user's work stays.
        assert!(dir.join("exports").join("e.wav").is_file() && dir.join("mcp.json").is_file());
        // Nothing left: nothing moved. Another the same second: a name of its own.
        assert_eq!(move_aside(&dir, "20261105-115505", "reset").unwrap(), None);
        fs::write(dir.join("kits.json"), "[]").unwrap();
        let again = move_aside(&dir, "20261105-115505", "reset").unwrap().unwrap();
        assert_eq!(again.folder, dir.join("backups").join("20261105-115505-reset-2"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_restore_puts_the_backup_in_place_and_keeps_what_was_there() {
        let dir = user_folder("restore");
        let backup = folder("restore-backup");
        fs::write(backup.join("library.json"), "{\"backup\":1}").unwrap();
        fs::create_dir_all(backup.join("songs")).unwrap();
        fs::write(backup.join("songs").join("b.odbox"), "{}").unwrap();

        let restored = restore(&backup, &dir, "20261105-115505").unwrap();
        assert_eq!(restored.names, ["library.json", "songs/"]);
        assert_eq!(fs::read_to_string(dir.join("library.json")).unwrap(), "{\"backup\":1}");
        assert!(dir.join("songs").join("b.odbox").is_file() && !dir.join("songs").join("a.odbox").exists());
        // Only what the backup holds: the kits it has not are the shipped ones again.
        assert!(!dir.join("kits.json").exists());
        let aside = restored.aside.unwrap();
        assert_eq!(aside.folder, dir.join("backups").join("20261105-115505-before-restore"));
        assert_eq!(fs::read_to_string(aside.folder.join("library.json")).unwrap(), "{\"mine\":1}");

        // Not a backup, or the folder itself: refused, nothing moved.
        let empty = folder("restore-empty");
        assert!(restore(&empty, &dir, "x").unwrap_err().starts_with("no orDrumbox user files"));
        assert!(restore(&dir, &dir, "x").is_err());
        assert_eq!(fs::read_to_string(dir.join("library.json")).unwrap(), "{\"backup\":1}");
        for d in [dir, backup, empty] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn copied_files_replace_those_of_the_same_name_only() {
        let from = user_folder("copy-from");
        let to = folder("copy-to");
        fs::write(to.join("kits.json"), "old").unwrap();
        fs::write(to.join("notes.txt"), "mine").unwrap();
        assert_eq!(copy_files(&from, &to).unwrap().len(), 6);
        assert_eq!(fs::read_to_string(to.join("kits.json")).unwrap(), "[]");
        assert_eq!(fs::read_to_string(to.join("notes.txt")).unwrap(), "mine");
        assert!(store::same_folder(&to, &to.join(".")));
        assert!(!store::same_folder(&to, &from));
        fs::remove_dir_all(from).unwrap();
        fs::remove_dir_all(to).unwrap();
    }
}
