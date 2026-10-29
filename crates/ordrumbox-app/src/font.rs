//! The text font: the system's monospace font, read at start and drawn with
//! `fontdue` (Consolas on Windows, Menlo on macOS, DejaVu or Liberation Mono
//! elsewhere; `$ORDRUMBOX_FONT` names another file). Nothing is bundled.
//!
//! Text sits on a grid of [`SIZE`] × [`SIZE`] font pixels per character, the
//! layout's unit: glyphs are sized so a capital is 7 font pixels high on an
//! 8-high line, and centred in their cell.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use fontdue::{Font, FontSettings};

/// Cell size in font pixels: one character is `SIZE * scale` wide and high.
pub const SIZE: i32 = 8;
/// Font pixels from the top of a cell to the baseline (a capital's height).
const BASELINE: i32 = 7;
/// Environment variable naming a font file to use instead.
pub const ENV_VAR: &str = "ORDRUMBOX_FONT";

/// A rasterised glyph: coverage (0–255) rows, and where they go in the cell.
#[derive(Debug)]
pub struct Glyph {
    pub width: usize,
    pub coverage: Vec<u8>,
    /// Offset of the bitmap's top-left corner from the cell's top-left corner.
    pub left: i32,
    pub top: i32,
}

#[derive(Debug)]
struct Loaded {
    font: Font,
    /// Pixel size per scale unit: a capital `BASELINE` font pixels high.
    px_per_scale: f32,
    /// Glyphs drawn so far, by character and scale.
    glyphs: Mutex<HashMap<(char, i32), &'static Glyph>>,
}

static FONT: OnceLock<Loaded> = OnceLock::new();

/// The font files tried, in order.
fn candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::env::var_os(ENV_VAR).map(PathBuf::from).into_iter().collect();
    if let Some(windir) = std::env::var_os("WINDIR") {
        paths.push(PathBuf::from(windir).join("Fonts").join("consola.ttf"));
    }
    paths.extend(
        [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Monaco.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "/usr/share/fonts/liberation-mono/LiberationMono-Regular.ttf",
        ]
        .map(PathBuf::from),
    );
    paths
}

/// Read the first font found; the error lists where it was looked for.
/// Calling it again keeps the font already loaded.
pub fn load() -> Result<(), String> {
    if FONT.get().is_some() {
        return Ok(());
    }
    let mut tried = Vec::new();
    for path in candidates() {
        let font = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Font::from_bytes(bytes, FontSettings::default()).map_err(str::to_owned));
        match font {
            Ok(font) => {
                // A capital's height at 1 px, from a large rendering.
                let cap = font.metrics('H', 100.0).height as f32 / 100.0;
                if cap <= 0.0 {
                    tried.push(format!("{}: no capital H", path.display()));
                    continue;
                }
                let px_per_scale = BASELINE as f32 / cap;
                // Tests load it from several threads at once: the first one stays.
                FONT.get_or_init(|| Loaded { font, px_per_scale, glyphs: Mutex::default() });
                return Ok(());
            }
            Err(e) => tried.push(format!("{}: {e}", path.display())),
        }
    }
    Err(format!("no monospace font found (set {ENV_VAR} to a .ttf file); tried {}", tried.join("; ")))
}

/// The font; `main` loads it first and stops when there is none, so a
/// failure here (a test machine without one) is a broken setup.
fn loaded() -> &'static Loaded {
    if FONT.get().is_none() {
        load().expect("a system monospace font");
    }
    FONT.get().expect("loaded above")
}

/// The glyph of `c` drawn at `scale` (the font's own box for a character it lacks).
pub fn glyph(c: char, scale: i32) -> &'static Glyph {
    let font = loaded();
    let mut glyphs = font.glyphs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    glyphs.entry((c, scale)).or_insert_with(|| {
        let (metrics, coverage) = font.font.rasterize(c, font.px_per_scale * scale as f32);
        let cell = SIZE * scale;
        // Centred on the advance in the cell, standing on the baseline.
        let left = (cell - metrics.advance_width.round() as i32) / 2 + metrics.xmin;
        let top = BASELINE * scale - (metrics.ymin + metrics.height as i32);
        // Kept for the whole run: one per character and scale shown.
        Box::leak(Box::new(Glyph { width: metrics.width, coverage, left, top }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capital_stands_on_the_baseline() {
        let h = glyph('H', 2);
        let rows = h.coverage.len() / h.width.max(1);
        assert!(h.top >= 0 && h.top + rows as i32 <= BASELINE * 2 + 1, "{h:?}");
        assert!(h.left >= 0 && h.left + h.width as i32 <= SIZE * 2, "{h:?}");
        assert!(h.coverage.iter().any(|&c| c > 128));
        assert!(std::ptr::eq(glyph('H', 2), h), "drawn once");
    }
}
