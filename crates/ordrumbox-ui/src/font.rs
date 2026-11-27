//! The text font: Pixel Operator Mono 8, built into the program and drawn
//! with `fontdue`, so text looks the same on every system (`assets/fonts/`,
//! CC0: `LICENSE-PixelOperator.txt` beside it). `$ORDRUMBOX_FONT` names
//! another file. The symbols it lacks are drawn by `symbols.rs`.
//!
//! Text sits on a grid of [`WIDTH`] × [`SIZE`] font pixels per character, the
//! layout's unit: glyphs are sized so a capital is 7 font pixels high on an
//! 8-high line, and centred in their cell, about as wide as the font's own
//! advance: letters keep their natural spacing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use fontdue::{Font, FontSettings};

mod symbols;

/// Cell height in font pixels: a line is `SIZE * scale` high.
pub const SIZE: i32 = 8;
/// Cell width in font pixels: one character is `WIDTH * scale` wide.
pub const WIDTH: i32 = 6;
/// Font pixels from the top of a cell to the baseline (a capital's height).
const BASELINE: i32 = 7;
/// Environment variable naming a font file to use instead.
pub const ENV_VAR: &str = "ORDRUMBOX_FONT";

/// The built-in font.
const BUILT_IN: &[u8] = include_bytes!("../../../assets/fonts/PixelOperatorMono8.ttf");

/// The symbols the views draw, each one in the built-in font or drawn by
/// `symbols.rs` (tested).
/// A new line, a pattern made again.
pub const REGENERATE: &str = "\u{266b}";
/// Step to the choice before.
pub const PREVIOUS: &str = "\u{25c4}";
/// Step to the choice after; also a folded section.
pub const NEXT: &str = "\u{25ba}";
/// An unfolded section.
pub const UNFOLDED: &str = "\u{25bc}";

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
    /// Glyphs drawn so far, by character and size in quarters of a scale unit.
    glyphs: Mutex<HashMap<(char, i32), &'static Glyph>>,
}

impl Loaded {
    /// The font of a TrueType file's bytes, sized by its capital H.
    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let font = Font::from_bytes(bytes, FontSettings::default()).map_err(str::to_owned)?;
        // A capital's height at 1 px, from a rendering large enough that its
        // rounding leaves a pixel font on whole pixels.
        let cap = font.metrics('H', 1000.0).height as f32 / 1000.0;
        if cap <= 0.0 {
            return Err("no capital H".to_owned());
        }
        Ok(Self { font, px_per_scale: BASELINE as f32 / cap, glyphs: Mutex::default() })
    }

    /// The built-in font.
    #[expect(clippy::expect_used, reason = "a constant of the program, read by a test")]
    fn built_in() -> Self {
        Self::from_bytes(BUILT_IN).expect("the built-in font")
    }

    /// The font's glyph of `c` (its own box for a character it lacks) in cells
    /// of `quarters` quarters of a scale unit.
    fn rasterize(&self, c: char, quarters: i32) -> Glyph {
        let (metrics, coverage) = self.font.rasterize(c, self.px_per_scale * quarters as f32 / 4.0);
        let cell = advance(quarters);
        // Centred on the advance in the cell, standing on the baseline.
        let left = (cell - metrics.advance_width.round() as i32) / 2 + metrics.xmin;
        let top = BASELINE * quarters / 4 - (metrics.ymin + metrics.height as i32);
        Glyph { width: metrics.width, coverage, left, top }
    }
}

static FONT: OnceLock<Loaded> = OnceLock::new();

/// Read the font: the file `$ORDRUMBOX_FONT` names, else the built-in one.
/// The error says why that file cannot be used. Calling it again keeps the
/// font already loaded.
pub fn load() -> Result<(), String> {
    if FONT.get().is_some() {
        return Ok(());
    }
    let font = match std::env::var_os(ENV_VAR).filter(|v| !v.is_empty()) {
        Some(path) => std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Loaded::from_bytes(&bytes))
            .map_err(|e| format!("{ENV_VAR}={}: {e}", Path::new(&path).display()))?,
        None => Loaded::built_in(),
    };
    // Tests load it from several threads at once: the first one stays.
    FONT.get_or_init(|| font);
    Ok(())
}

/// The font: the one `main` loaded, else the built-in one (tests).
fn loaded() -> &'static Loaded {
    FONT.get_or_init(Loaded::built_in)
}

/// Quarters of a scale unit in the small font: its cells are three quarters
/// of the regular ones.
pub const SMALL_QUARTERS: i32 = 3;

/// The width of a character in cells of `quarters` quarters of a scale unit,
/// in pixels, rounded.
pub fn advance(quarters: i32) -> i32 {
    (WIDTH * quarters + 2) / 4
}

/// The glyph of `c` (a symbol of `symbols.rs` the font lacks, else the font's
/// own, or its box) in cells of `SIZE × quarters / 4` pixels: `4 * scale` is the regular font,
/// `SMALL_QUARTERS * scale` the small one.
pub fn glyph_quarters(c: char, quarters: i32) -> &'static Glyph {
    let font = loaded();
    let mut glyphs = font.glyphs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    glyphs.entry((c, quarters)).or_insert_with(|| {
        let drawn = (font.font.lookup_glyph_index(c) == 0).then(|| symbols::glyph(c, quarters)).flatten();
        // Kept for the whole run: one per character and size shown.
        Box::leak(Box::new(drawn.unwrap_or_else(|| font.rasterize(c, quarters))))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capital_stands_on_the_baseline() {
        let h = glyph_quarters('H', 8);
        let rows = h.coverage.len() / h.width.max(1);
        assert!(h.top >= 0 && h.top + rows as i32 <= BASELINE * 2 + 1, "{h:?}");
        assert!(h.left >= 0 && h.left + h.width as i32 <= WIDTH * 2, "{h:?}");
        assert!(h.coverage.iter().any(|&c| c > 128));
        assert!(std::ptr::eq(glyph_quarters('H', 8), h), "drawn once");
    }

    #[test]
    fn the_built_in_font_has_every_character_the_views_draw() {
        let font = Loaded::built_in().font;
        let symbols = [REGENERATE, PREVIOUS, NEXT, UNFOLDED].concat();
        let text = (' '..='~').chain("→←↔—–×±·…°µéÉàç".chars()).chain(symbols.chars());
        let missing: String =
            text.filter(|&c| font.lookup_glyph_index(c) == 0 && symbols::glyph(c, 4).is_none()).collect();
        assert!(missing.is_empty(), "neither in the built-in font nor drawn: {missing}");
    }
}
