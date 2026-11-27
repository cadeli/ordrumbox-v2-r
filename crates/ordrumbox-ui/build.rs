//! Names the build the program is: `ORDRUMBOX_BUILD`, empty for a build of
//! the commit tagged `v<version>` with no local change, else
//! ` dev <commit>[+dirty] <commit date>`, read from git. Without git (a source
//! archive), ` dev unknown build`: shown as is, never passed for a release.
//! Also decodes the images built into the program into raw pixels (`image.rs`).

use std::error::Error;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The images built into the program, transparent where they should be: the
/// PNG, from the crate's folder, its size in pixels (which `image.rs` repeats),
/// the file of its pixels in `$OUT_DIR`. The logo of Help > About and of the synth
/// editor's head, the emblem of the synth editor's last card.
const IMAGES: [(&str, (u32, u32), &str); 2] = [
    ("../../assets/images/logo2.png", (320, 64), "logo.rgba"),
    ("../../assets/images/logo.png", (192, 192), "emblem.rgba"),
];

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

/// Decodes `png` (RGBA, 8 or 16 bits, as 8) of `size` into `$OUT_DIR/<out>`,
/// which `image.rs` includes.
fn image_pixels((png, size, out): (&str, (u32, u32), &str)) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={png}");
    let mut decoder = png::Decoder::new(BufReader::new(File::open(png)?));
    decoder.set_transformations(png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("the PNG is too large")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
        || (info.width, info.height) != size
    {
        return Err(format!("{png} must be {} x {}, RGBA", size.0, size.1).into());
    }
    pixels.truncate(info.buffer_size());
    fs::write(PathBuf::from(std::env::var("OUT_DIR")?).join(out), pixels)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    for image in IMAGES {
        image_pixels(image)?;
    }
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
