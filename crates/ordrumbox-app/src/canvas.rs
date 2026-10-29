//! Software drawing into the window's pixel buffer (`0x00RRGGBB` per pixel).
//!
//! The whole UI is drawn here, without a GUI framework: rectangles and text
//! in the system's monospace font; lines and knob arcs come with the editors.

use crate::font;

/// A pixel colour, `0x00RRGGBB` (the softbuffer format).
pub type Rgb = u32;

/// A borrowed frame to draw into, clipped to its bounds.
#[derive(Debug)]
pub struct Canvas<'a> {
    pixels: &'a mut [Rgb],
    width: usize,
    height: usize,
}

impl<'a> Canvas<'a> {
    /// `pixels` holds `width * height` pixels, row by row.
    pub fn new(pixels: &'a mut [Rgb], width: usize, height: usize) -> Self {
        debug_assert_eq!(pixels.len(), width * height);
        Self { pixels, width, height }
    }

    pub fn width(&self) -> i32 {
        self.width as i32
    }

    pub fn height(&self) -> i32 {
        self.height as i32
    }

    pub fn clear(&mut self, color: Rgb) {
        self.pixels.fill(color);
    }

    /// Fill the rectangle at `(x, y)` of size `w × h`; parts outside the
    /// canvas are dropped.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb) {
        let x0 = x.clamp(0, self.width as i32) as usize;
        let y0 = y.clamp(0, self.height as i32) as usize;
        let x1 = x.saturating_add(w).clamp(0, self.width as i32) as usize;
        let y1 = y.saturating_add(h).clamp(0, self.height as i32) as usize;
        if x0 >= x1 {
            return;
        }
        for row in y0..y1 {
            let start = row * self.width;
            self.pixels[start + x0..start + x1].fill(color);
        }
    }

    /// Draw `text` with its top-left corner at `(x, y)`, one cell of
    /// `font::SIZE × scale` pixels per character, blended over what is there;
    /// returns the x after the last character.
    pub fn text(&mut self, x: i32, y: i32, text: &str, scale: i32, color: Rgb) -> i32 {
        let mut pen = x;
        for c in text.chars() {
            let glyph = font::glyph(c, scale);
            for (row, line) in glyph.coverage.chunks(glyph.width.max(1)).enumerate() {
                for (col, &coverage) in line.iter().enumerate() {
                    self.blend(pen + glyph.left + col as i32, y + glyph.top + row as i32, color, coverage);
                }
            }
            pen += font::SIZE * scale;
        }
        pen
    }

    /// `color` over the pixel at `(x, y)` at `coverage` (0–255), if inside.
    fn blend(&mut self, x: i32, y: i32, color: Rgb, coverage: u8) {
        if coverage == 0 || !(0..self.width as i32).contains(&x) || !(0..self.height as i32).contains(&y) {
            return;
        }
        let pixel = &mut self.pixels[y as usize * self.width + x as usize];
        let a = u32::from(coverage);
        let mix = |shift: u32| {
            let (over, under) = ((color >> shift) & 0xff, (*pixel >> shift) & 0xff);
            ((over * a + under * (255 - a) + 127) / 255) << shift
        };
        *pixel = mix(16) | mix(8) | mix(0);
    }
}

/// Width of `text` drawn at `scale`.
pub fn text_width(text: &str, scale: i32) -> i32 {
    text.chars().count() as i32 * font::SIZE * scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_clips_to_bounds() {
        let mut pixels = vec![0; 4 * 3];
        let mut canvas = Canvas::new(&mut pixels, 4, 3);
        canvas.fill_rect(-1, 1, 3, 10, 7);
        #[rustfmt::skip]
        assert_eq!(pixels, [
            0, 0, 0, 0,
            7, 7, 0, 0,
            7, 7, 0, 0,
        ]);
    }

    #[test]
    fn empty_or_outside_rect_draws_nothing() {
        let mut pixels = vec![0; 4];
        let mut canvas = Canvas::new(&mut pixels, 2, 2);
        canvas.fill_rect(0, 0, 0, 2, 1);
        canvas.fill_rect(5, 5, 2, 2, 1);
        canvas.fill_rect(0, 0, 2, -3, 1);
        assert_eq!(pixels, [0; 4]);
    }

    #[test]
    fn text_is_blended_inside_its_cells() {
        let mut pixels = vec![0; 32 * 16];
        let mut canvas = Canvas::new(&mut pixels, 32, 16);
        assert_eq!(canvas.text(0, 0, "Hé", 2, 0xff_ff_ff), 32);
        let lit =
            |x0: usize, x1: usize| (0..16).any(|y| pixels[y * 32 + x0..y * 32 + x1].iter().any(|&p| p > 0x80_80_80));
        assert!(lit(0, 16) && lit(16, 32), "both cells drawn");
        // Coverage blends: some pixels are neither background nor full colour.
        assert!(pixels.iter().any(|&p| p != 0 && p != 0xff_ff_ff));
        assert_eq!(text_width("abc", 2), 48);
    }
}
