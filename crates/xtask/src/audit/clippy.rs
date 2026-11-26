//! What clippy measures beyond CI's `cargo clippy`: functions that are too
//! long or too complex (the thresholds are in `clippy.toml`), a value taken
//! by value and not used, a clone that is not needed.

use std::io;
use std::path::Path;
use std::process::Command;

use super::{Finding, Level};

/// The lints asked for, a phrase of the message each one gives, and the
/// rule it is reported under.
const LINTS: [(&str, &str, &str); 4] = [
    ("clippy::too_many_lines", "too many lines", "function size"),
    ("clippy::cognitive_complexity", "cognitive complexity", "complexity"),
    ("clippy::needless_pass_by_value", "passed by value", "by value"),
    ("clippy::redundant_clone", "redundant clone", "clone"),
];

/// Run clippy on the production code (libraries and programs) with `LINTS`.
pub(super) fn check(root: &Path) -> io::Result<Vec<Finding>> {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command.current_dir(root).args(["clippy", "--workspace", "--lib", "--bins", "--message-format=short", "--"]);
    for (lint, _, _) in LINTS {
        command.args(["-W", lint]);
    }
    let output = command.output()?;
    let report = String::from_utf8_lossy(&output.stderr);
    let mut findings = Vec::new();
    for line in report.lines() {
        // `path:line:col: warning: message`
        let Some((place, message)) = line.split_once(": warning: ") else { continue };
        let Some(&(_, _, rule)) = LINTS.iter().find(|(_, phrase, _)| message.contains(phrase)) else { continue };
        // The sources' paths use `/` on every system (`shown`), so `exempt` can match them.
        findings.push(Finding::new(Level::Warning, rule, place.replace('\\', "/"), message));
    }
    if !output.status.success() {
        let message = "clippy did not build: its report is incomplete (run `cargo clippy --workspace`)";
        findings.push(Finding::new(Level::Error, "clippy", "cargo clippy", message));
    }
    Ok(findings)
}
