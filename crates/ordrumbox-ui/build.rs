//! Names the build the program is: `ORDRUMBOX_BUILD`, empty for a build of
//! the commit tagged `v<version>` with no local change, else
//! ` dev <commit>[+dirty] <commit date>`, read from git. Without git (a source
//! archive), ` dev unknown build`: shown as is, never passed for a release.
//! Also decodes the logo of Help > About into raw pixels (`image.rs`).

use std::error::Error;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The logo of Help > About, from the crate's folder.
const LOGO_PNG: &str = "../../assets/images/logo2.png";
/// Its size in pixels, which `image.rs` repeats.
const LOGO_SIZE: (u32, u32) = (320, 64);

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

/// Decodes the logo PNG (8-bit RGBA) into `$OUT_DIR/logo.rgba`, which `image.rs` includes.
fn logo_pixels() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={LOGO_PNG}");
    let mut reader = png::Decoder::new(BufReader::new(File::open(LOGO_PNG)?)).read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("the logo PNG is too large")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || (info.width, info.height) != LOGO_SIZE
    {
        return Err(format!("{LOGO_PNG} must be {} x {}, 8-bit RGBA", LOGO_SIZE.0, LOGO_SIZE.1).into());
    }
    pixels.truncate(info.buffer_size());
    fs::write(PathBuf::from(std::env::var("OUT_DIR")?).join("logo.rgba"), pixels)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    logo_pixels()?;
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
    Ok(())
}
