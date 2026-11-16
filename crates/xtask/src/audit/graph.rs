//! The crates' dependencies against the table of AGENTS.md, "Layout":
//! they only point down, and each crate takes only the third-party crates
//! that its role allows.

use std::fs;
use std::io;
use std::path::Path;

use super::{Finding, Level};

/// A crate, the workspace crates it may depend on, and the third-party
/// crates it may depend on (by the name of its `Cargo.toml`).
struct Allowed {
    name: &'static str,
    internal: &'static [&'static str],
    external: &'static [&'static str],
}

const ALLOWED: [Allowed; 9] = [
    Allowed { name: "ordrumbox-core", internal: &[], external: &[] },
    Allowed { name: "ordrumbox-dsp", internal: &[], external: &[] },
    Allowed { name: "ordrumbox-logic", internal: &["ordrumbox-core"], external: &[] },
    Allowed { name: "ordrumbox-engine", internal: &["ordrumbox-core", "ordrumbox-dsp"], external: &[] },
    Allowed {
        name: "ordrumbox-probe",
        internal: &["ordrumbox-core", "ordrumbox-dsp", "ordrumbox-engine"],
        external: &[],
    },
    Allowed {
        name: "ordrumbox-ui",
        internal: &["ordrumbox-core", "ordrumbox-logic", "ordrumbox-engine"],
        external: &["fontdue"],
    },
    Allowed {
        name: "ordrumbox-app",
        internal: &["ordrumbox-core", "ordrumbox-engine", "ordrumbox-ui"],
        external: &["winit", "softbuffer", "cpal", "midir"],
    },
    Allowed {
        name: "ordrumbox-plugin",
        internal: &["ordrumbox-core", "ordrumbox-engine", "ordrumbox-ui"],
        external: &[
            "nih_plug",
            "baseview",
            "softbuffer",
            "raw-window-handle",
            "raw-window-handle-06",
            "keyboard-types",
            "process_path",
            "serde",
        ],
    },
    Allowed { name: "xtask", internal: &[], external: &["nih_plug_xtask"] },
];

/// The `[dependencies]` of every crate that is in the table, and every crate
/// of `crates/` that is not.
pub(super) fn check(root: &Path) -> io::Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for entry in fs::read_dir(root.join("crates"))? {
        let dir = entry?.path();
        let manifest = dir.join("Cargo.toml");
        let Some(folder) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        if !manifest.is_file() {
            continue;
        }
        let place = format!("crates/{folder}/Cargo.toml");
        let Some(allowed) = ALLOWED.iter().find(|a| a.name == folder) else {
            let message = "a crate that AGENTS.md's table does not list: add it there and to the audit";
            findings.push(Finding::new(Level::Error, "crate graph", place, message));
            continue;
        };
        for dependency in dependencies(&fs::read_to_string(&manifest)?) {
            let permitted = if dependency.starts_with("ordrumbox-") {
                allowed.internal.contains(&dependency.as_str())
            } else {
                allowed.external.contains(&dependency.as_str())
            };
            if !permitted {
                let message = format!("{} may not depend on {dependency}", allowed.name);
                findings.push(Finding::new(Level::Error, "crate graph", place.clone(), message));
            }
        }
    }
    Ok(findings)
}

/// The names under the `[dependencies]` tables of a manifest (target-specific
/// ones too; dev and build dependencies do not ship).
fn dependencies(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_dependencies = false;
    for line in manifest.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_end_matches(']');
            in_dependencies =
                header == "dependencies" || (header.starts_with("target.") && header.ends_with(".dependencies"));
        } else if in_dependencies && !line.is_empty() && !line.starts_with('#') {
            let end = line.find(|c: char| c == '=' || c == '.' || c.is_whitespace()).unwrap_or(line.len());
            names.push(line[..end].to_owned());
        }
    }
    names
}
