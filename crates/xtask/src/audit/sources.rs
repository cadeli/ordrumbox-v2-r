//! The production sources of every crate, and the size of each file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Finding, Level, shown};

/// A file of more code lines than this is a warning.
const FILE_LINES: usize = 500;
/// A file of more code lines than this must be split before it grows.
const FILE_LINES_MAX: usize = 800;

/// One source file of a crate's `src/`, tests apart.
#[derive(Debug)]
pub(super) struct Source {
    pub path: String,
    pub crate_name: String,
    pub lines: Vec<String>,
    /// Where the unit tests start (`#[cfg(test)]`), else the end.
    pub code_end: usize,
}

impl Source {
    /// The lines before the tests.
    pub fn code(&self) -> &[String] {
        &self.lines[..self.code_end]
    }

    /// The lines of code: not blank, not a comment.
    fn code_line_count(&self) -> usize {
        let is_code = |l: &&String| !l.trim().is_empty() && !l.trim_start().starts_with("//");
        self.code().iter().filter(is_code).count()
    }
}

/// Every `.rs` file under `crates/*/src`, without tests, examples and the
/// literal tables.
pub(super) fn load(root: &Path) -> io::Result<Vec<Source>> {
    let mut found = Vec::new();
    let mut crates: Vec<PathBuf> =
        fs::read_dir(root.join("crates"))?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    crates.sort();
    for dir in crates {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        let src = dir.join("src");
        if src.is_dir() {
            collect(root, &src, &name, &mut found)?;
        }
    }
    Ok(found)
}

fn collect(root: &Path, dir: &Path, crate_name: &str, found: &mut Vec<Source>) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if path.is_dir() {
            if !name.ends_with("_tests") && name != "tests" {
                collect(root, &path, crate_name, found)?;
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && name != "data.rs" && !name.ends_with("_tests.rs") {
            let lines: Vec<String> = fs::read_to_string(&path)?.lines().map(str::to_owned).collect();
            let code_end = lines.iter().position(|l| l.trim() == "#[cfg(test)]").unwrap_or(lines.len());
            found.push(Source { path: shown(root, &path), crate_name: crate_name.to_owned(), lines, code_end });
        }
    }
    Ok(())
}

/// The files that are too long.
pub(super) fn size_findings(files: &[Source]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let count = file.code_line_count();
        if count > FILE_LINES_MAX {
            let message = format!("{count} lines of code: split it before adding to it (limit {FILE_LINES_MAX})");
            findings.push(Finding::new(Level::Error, "file size", file.path.clone(), message));
        } else if count > FILE_LINES {
            let message = format!("{count} lines of code (about {FILE_LINES} is the size)");
            findings.push(Finding::new(Level::Warning, "file size", file.path.clone(), message));
        }
    }
    findings
}
