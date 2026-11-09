//! A failure that keeps the window from opening, told where the user sees
//! it: the Windows release build has no console, so stderr reaches no one.

use std::path::PathBuf;

use crate::store;

/// The file a startup failure is written to, in the user data folder.
const FILE_NAME: &str = "startup-error.txt";

/// Tell `message`: written to `startup-error.txt` in the user data folder
/// and, on Windows, shown in a message box. Returns `message` with what of
/// this failed, for stderr.
pub fn report(message: &str) -> String {
    let mut told = message.to_owned();
    match write(message) {
        Ok(path) => told.push_str(&format!("\n(written to {})", path.display())),
        Err(e) => told.push_str(&format!("\n(not written to a file: {e})")),
    }
    #[cfg(windows)]
    if let Err(e) = show(&told) {
        told.push_str(&format!("\n(not shown: {e})"));
    }
    told
}

fn write(message: &str) -> Result<PathBuf, String> {
    let path = store::user_dir().ok_or("no user data folder")?.join(FILE_NAME);
    store::write_atomic(&path, format!("orDrumbox {}\n{message}\n", crate::BUILD))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// A message box, through the system's PowerShell: the text goes in an
/// environment variable, never into the script.
#[cfg(windows)]
fn show(message: &str) -> Result<(), String> {
    const VARIABLE: &str = "ORDRUMBOX_STARTUP_ERROR";
    let script = format!(
        "Add-Type -AssemblyName PresentationFramework; \
         [System.Windows.MessageBox]::Show($env:{VARIABLE}, 'orDrumbox', 'OK', 'Error') | Out-Null"
    );
    let status = crate::update::command("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script])
        .env(VARIABLE, message)
        .status()
        .map_err(|e| format!("powershell: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("powershell: {status}")) }
}
