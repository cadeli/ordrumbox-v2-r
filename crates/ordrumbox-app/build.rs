//! Names the build the program is: `ORDRUMBOX_BUILD`, empty for a build of
//! the commit tagged `v<version>` with no local change, else
//! ` dev <commit>[+dirty] <commit date>`, read from git. Without git (a source
//! archive), ` dev unknown build`: shown as is, never passed for a release.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The output of `git <args>` run in the workspace, trimmed, if it succeeded.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// What follows the version: empty for a release build.
fn build_suffix(root: &Path, version: &str) -> String {
    let Some(commit) = git(root, &["rev-parse", "--short=7", "HEAD"]) else {
        println!("cargo:warning=git not found: this build is named \"dev unknown build\"");
        return " dev unknown build".to_owned();
    };
    let dirty = git(root, &["status", "--porcelain", "--untracked-files=no"]).is_none_or(|s| !s.is_empty());
    let tag = format!("v{version}");
    let tagged = git(root, &["tag", "--points-at", "HEAD"]).is_some_and(|tags| tags.lines().any(|t| t == tag));
    if tagged && !dirty {
        return String::new();
    }
    let date = git(root, &["log", "-1", "--format=%cs"]).unwrap_or_else(|| "unknown date".to_owned());
    format!(" dev {commit}{} {date}", if dirty { "+dirty" } else { "" })
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    println!("cargo:rustc-env=ORDRUMBOX_BUILD={}", build_suffix(&root, env!("CARGO_PKG_VERSION")));
    // Named again when the commit, the tags, the index or a source changes.
    if let Some(git_dir) = git(&root, &["rev-parse", "--absolute-git-dir"]) {
        let git_dir = PathBuf::from(git_dir);
        for name in ["HEAD", "index", "refs", "packed-refs"] {
            println!("cargo:rerun-if-changed={}", git_dir.join(name).display());
        }
    }
    for name in ["crates", "assets", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
    }
}
