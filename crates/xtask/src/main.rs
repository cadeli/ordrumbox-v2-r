//! `cargo xtask bundle ordrumbox-plugin --release [--features vst3]`: nih-plug's
//! bundler, then `assets/` copied into each orDrumbox bundle of
//! `target/bundled/` (`Contents/Resources/assets`, or next to a bundle that
//! is a single file, the CLAP on Windows), where the plugin looks for it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use nih_plug_xtask::Result;

/// The bundle name `bundler.toml` gives the plugin.
const BUNDLE_NAME: &str = "orDrumbox";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bundling = args.first().is_some_and(|command| command.starts_with("bundle"));
    let bundled =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from).join("bundled");
    // The bundles of an earlier build go first: a build without `vst3` would
    // otherwise leave an old `orDrumbox.vst3` there (a CI cache restores
    // `target/`), which the loop below would sign as if it were new.
    if bundling && bundled.exists() {
        for bundle in bundles(&bundled)? {
            remove(&bundle)?;
        }
    }
    nih_plug_xtask::main_with_args("cargo xtask", args.iter().cloned())?;
    // nih-plug's bundler left the current folder at the workspace root.
    if bundling {
        for bundle in bundles(&bundled)? {
            add_assets(&bundle, Path::new("assets"))?;
            println!("Added assets to {}", bundle.display());
        }
    }
    Ok(())
}

/// The orDrumbox bundles in `dir`.
fn bundles(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let stem = path.file_stem().and_then(|s| s.to_str());
        let extension = path.extension().and_then(|s| s.to_str());
        if stem == Some(BUNDLE_NAME) && matches!(extension, Some("clap" | "vst3")) {
            found.push(path);
        }
    }
    Ok(found)
}

/// `assets` into `bundle`, then the bundle signed again on macOS (its
/// signature covers its resources).
fn add_assets(bundle: &Path, assets: &Path) -> Result<()> {
    let destination = if bundle.is_dir() {
        bundle.join("Contents").join("Resources").join("assets")
    } else {
        bundle.with_file_name("assets")
    };
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    copy_dir(assets, &destination)?;
    if cfg!(target_os = "macos") && bundle.is_dir() {
        let status = Command::new("codesign").args(["-f", "-s", "-"]).arg(bundle).status()?;
        anyhow_ensure(status.success(), format!("codesign {} failed: {status}", bundle.display()))?;
    }
    Ok(())
}

/// `path`, a folder with its contents or a single file.
fn remove(path: &Path) -> io::Result<()> {
    if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }
}

fn anyhow_ensure(ok: bool, message: String) -> Result<()> {
    if ok { Ok(()) } else { Err(io::Error::other(message).into()) }
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if entry.file_name() != ".DS_Store" {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
