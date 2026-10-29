//! Software drawing into the window's pixel buffer (`0x00RRGGBB` per pixel).
//!
//! The whole UI is drawn here, without a GUI framework: rectangles and the
//! 8 px pixel font; lines and knob arcs come with the editors.

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

    /// Draw `text` with its top-left corner at `(x, y)`, each font pixel a
    /// `scale × scale` square; returns the x after the last character.
    pub fn text(&mut self, x: i32, y: i32, text: &str, scale: i32, color: Rgb) -> i32 {
        let mut pen = x;
        for c in shown(text) {
            for (row, bits) in font::glyph(c).iter().enumerate() {
                for col in 0..8 {
                    if bits & (0x80 >> col) != 0 {
                        self.fill_rect(pen + col * scale, y + row as i32 * scale, scale, scale, color);
                    }
                }
            }
            pen += font::SIZE * scale;
        }
        pen
    }
}

/// Width of `text` drawn at `scale`.
pub fn text_width(text: &str, scale: i32) -> i32 {
    shown(text).count() as i32 * font::SIZE * scale
}

/// The characters drawn for `text`: the font is ASCII, so the punctuation the
/// app's messages use is spelled out (`→` as `->`); anything else shows as `?`.
fn shown(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(|c| {
        let replacement: &[char] = match c {
            '→' => &['-', '>'],
            '←' => &['<', '-'],
            '—' | '–' => &['-'],
            '…' => &['.', '.', '.'],
            '×' => &['x'],
            '·' => &['/'],
            _ => &[],
        };
        let own = replacement.is_empty().then_some(c);
        replacement.iter().copied().chain(own)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_ascii_punctuation_is_spelled_out() {
        assert_eq!(shown("a → b…").collect::<String>(), "a -> b...");
        assert_eq!(text_width("→", 1), 2 * font::SIZE);
    }

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
    fn text_draws_glyph_pixels() {
        let mut pixels = vec![0; 16 * 8];
        let mut canvas = Canvas::new(&mut pixels, 16, 8);
        assert_eq!(canvas.text(0, 0, "A", 1, 1), 8);
        // Top row of 'A' is 0x30: pixels 2, 3.
        assert_eq!(&pixels[..8], &[0, 0, 1, 1, 0, 0, 0, 0]);
        assert_eq!(text_width("abc", 2), 48);
    }
}
