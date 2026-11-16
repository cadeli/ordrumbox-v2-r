//! One window per user data folder: a lock file held while the app runs, and
//! the question asked when another orDrumbox already holds it. Two windows
//! would write the same library, and the second would take over the MCP link.
//! The plugin and `ordrumbox --mcp` never come here.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

use crate::store;

/// The lock file, in the user data folder.
const FILE_NAME: &str = "ordrumbox.lock";

const QUESTION: &str = "Another orDrumbox is already running.\n\n\
    Two windows editing the same library can overwrite each other's work.\n\n\
    Continue anyway?";

/// What the app does after the check.
#[derive(Debug)]
pub struct Entry {
    /// The held lock: dropping it (the app quitting, or crashing) frees it.
    /// None when the app goes on without one.
    pub lock: Option<File>,
    /// What could not be checked or asked, for the status line.
    pub notice: Option<String>,
}

/// Take the lock, or ask the user what to do when another window holds it.
/// None: the user chose to quit.
#[must_use]
pub fn enter() -> Option<Entry> {
    let Some(dir) = store::user_dir() else {
        return Some(Entry {
            lock: None,
            notice: Some("No user data folder: not checked for another orDrumbox".into()),
        });
    };
    match claim(&dir.join(FILE_NAME)) {
        Ok(Some(lock)) => Some(Entry { lock: Some(lock), notice: None }),
        Ok(None) => match ask_continue() {
            Ok(true) => Some(Entry { lock: None, notice: Some("Another orDrumbox is running too".into()) }),
            Ok(false) => None,
            Err(e) => Some(Entry { lock: None, notice: Some(format!("Another orDrumbox is running; not asked: {e}")) }),
        },
        Err(e) => Some(Entry { lock: None, notice: Some(format!("Not checked for another orDrumbox: {e}")) }),
    }
}

/// Lock `path`, creating it and its folder: None when another holder has it.
fn claim(path: &Path) -> io::Result<Option<File>> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Ask in a window of the system whether to go on: true to continue.
#[cfg(windows)]
fn ask_continue() -> Result<bool, String> {
    const VARIABLE: &str = "ORDRUMBOX_QUESTION";
    let script = format!(
        "Add-Type -AssemblyName PresentationFramework; \
         [System.Windows.MessageBox]::Show($env:{VARIABLE}, 'orDrumbox', 'YesNo', 'Warning')"
    );
    let output = crate::update::command("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script])
        .env(VARIABLE, QUESTION)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !output.status.success() {
        return Err(format!("powershell: {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "Yes")
}

/// Ask in a window of the system whether to go on: true to continue.
#[cfg(target_os = "macos")]
fn ask_continue() -> Result<bool, String> {
    const CONTINUE: &str = "Continue anyway";
    let script = format!(
        "on run argv\n display dialog (item 1 of argv) with title \"orDrumbox\" with icon caution \
         buttons {{\"Quit\", \"{CONTINUE}\"}} default button \"Quit\" cancel button \"Quit\"\nend run"
    );
    let output = crate::update::command("osascript", &["-e", &script, QUESTION])
        .output()
        .map_err(|e| format!("osascript: {e}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).contains(CONTINUE));
    }
    // Quit is the cancel button: osascript reports it as error -128.
    if String::from_utf8_lossy(&output.stderr).contains("-128") {
        return Ok(false);
    }
    Err(format!("osascript: {}", output.status))
}

/// No window to ask in: the program goes on, and says so.
#[cfg(not(any(windows, target_os = "macos")))]
fn ask_continue() -> Result<bool, String> {
    Err(format!("no dialog on this system: {QUESTION}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_waits_for_the_first_to_end() {
        let path = std::env::temp_dir().join(format!("ordrumbox_lock_{}/{FILE_NAME}", std::process::id()));
        let first = claim(&path).unwrap();
        assert!(first.is_some());
        assert!(claim(&path).unwrap().is_none(), "held: the second finds it taken");
        drop(first);
        assert!(claim(&path).unwrap().is_some(), "freed by the drop");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
