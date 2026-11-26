//! Software drawing into the window's pixel buffer (`0x00RRGGBB` per pixel).
//!
//! The whole UI is drawn here, without a GUI framework: rectangles and text
//! in the system's monospace font; lines and knob arcs come with the editors.

use crate::font;
use crate::image::Image;
use crate::scale::Scale;

/// A pixel colour, `0x00RRGGBB` (the softbuffer format).
pub type Rgb = u32;

/// Which way a shape grows from its baseline on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

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

    /// Cover the rectangle at `(x, y)` of size `w × h` with `color`, `alpha`
    /// of 255 opaque, letting what is under it show through the rest.
    pub fn tint_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb, alpha: u8) {
        for row in y.max(0)..y.saturating_add(h).min(self.height as i32) {
            for col in x.max(0)..x.saturating_add(w).min(self.width as i32) {
                self.blend(col, row, color, alpha);
            }
        }
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

    /// Lay `color` over the rectangle at `(x, y)` of size `w × h` at
    /// `opacity` (0–255), what is under it showing through; parts outside
    /// the canvas are dropped.
    pub fn shade_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb, opacity: u8) {
        for row in y.max(0)..y.saturating_add(h).min(self.height as i32) {
            for col in x.max(0)..x.saturating_add(w).min(self.width as i32) {
                self.blend(col, row, color, opacity);
            }
        }
    }

    /// Fill the rectangle at `(x, y)` of size `w × h` with a vertical gradient
    /// from `top` to `bottom` (the pair), its corners rounded to `radius` pixels and
    /// smoothed over what is under them; parts outside the canvas are dropped.
    pub fn fill_rounded_gradient(&mut self, x: i32, y: i32, w: i32, h: i32, (top, bottom): (Rgb, Rgb), radius: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let radius = radius.clamp(0, w.min(h) / 2);
        for row in 0..h {
            let color = lerp(top, bottom, row, h - 1);
            // Distance below the top or above the bottom edge, inside the corner band.
            let edge = row.min(h - 1 - row);
            if edge >= radius {
                self.fill_rect(x, y + row, w, 1, color);
                continue;
            }
            self.fill_rect(x + radius, y + row, w - 2 * radius, 1, color);
            for col in 0..radius {
                // Coverage of the pixel by the circle of the corner, from its centre.
                let (dx, dy) = (f64::from(radius - col) - 0.5, f64::from(radius - edge) - 0.5);
                let coverage = (f64::from(radius) + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
                let alpha = (coverage * 255.0).round() as u8;
                self.blend(x + col, y + row, color, alpha);
                self.blend(x + w - 1 - col, y + row, color, alpha);
            }
        }
    }

    /// A line from `a` to `b`, `t` thick.
    pub fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), t: i32, color: Rgb) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for i in 0..=steps {
            self.fill_rect(x0 + (x1 - x0) * i / steps - t / 2, y0 + (y1 - y0) * i / steps - t / 2, t, t, color);
        }
    }

    /// Lines through `points`, in order, `t` thick.
    pub fn polyline(&mut self, points: impl Iterator<Item = (i32, i32)>, t: i32, color: Rgb) {
        let mut previous = None;
        for p in points {
            if let Some(q) = previous {
                self.line(q, p, t, color);
            }
            previous = Some(p);
        }
    }

    /// Draw `text` with its top-left corner at `(x, y)`, one cell of
    /// `font::SIZE × scale` pixels per character, blended over what is there;
    /// returns the x after the last character.
    pub fn text(&mut self, x: i32, y: i32, text: &str, scale: Scale, color: Rgb) -> i32 {
        self.text_quarters(x, y, text, scale.quarters(), color)
    }

    /// The same in the small font: cells three quarters the size.
    pub fn small_text(&mut self, x: i32, y: i32, text: &str, scale: Scale, color: Rgb) -> i32 {
        self.text_quarters(x, y, text, small(scale).quarters(), color)
    }

    fn text_quarters(&mut self, x: i32, y: i32, text: &str, quarters: i32, color: Rgb) -> i32 {
        let mut pen = x;
        for c in text.chars() {
            let glyph = font::glyph_quarters(c, quarters);
            for (row, line) in glyph.coverage.chunks(glyph.width.max(1)).enumerate() {
                for (col, &coverage) in line.iter().enumerate() {
                    self.blend(pen + glyph.left + col as i32, y + glyph.top + row as i32, color, coverage);
                }
            }
            pen += font::advance(quarters);
        }
        pen
    }

    /// One side of a waveform: the size of `values` times `gain` (0–1,
    /// `reach` pixels at 1) interpolated to a column per pixel over `width`,
    /// growing from `baseline` toward `direction`. The body fades from a bright
    /// core at the baseline to the edge, and the outline is smoothed over its
    /// last pixel.
    pub fn draw_profile(
        &mut self,
        (x, baseline, width): (i32, i32, i32),
        reach: i32,
        (values, gain): (&[f64], f64),
        direction: Direction,
        color: Rgb,
    ) {
        const CORE_OPACITY: f64 = 215.0;
        const EDGE_OPACITY: f64 = 70.0;
        let outline = crate::theme::mix(color, crate::theme::text(), 0.35);
        let last = values.len().saturating_sub(1);
        // The row `distance` pixels from the baseline.
        let row = |distance: i32| match direction {
            Direction::Up => baseline - distance - 1,
            Direction::Down => baseline + distance,
        };
        let value = |i: usize| values.get(i).map_or(0.0, |v| (v.abs() * gain).clamp(0.0, 1.0));
        for column in 0..width {
            let position = if width > 1 { f64::from(column) / f64::from(width - 1) * last as f64 } else { 0.0 };
            let below = (position.floor() as usize).min(last);
            let above = (below + 1).min(last);
            let fraction = position - below as f64;
            let height = (value(below) * (1.0 - fraction) + value(above) * fraction) * f64::from(reach);
            let whole = height.floor() as i32;
            for distance in 0..whole {
                let opacity =
                    CORE_OPACITY + (EDGE_OPACITY - CORE_OPACITY) * f64::from(distance) / f64::from(reach.max(1));
                self.blend(x + column, row(distance), color, opacity as u8);
            }
            if height > 0.0 {
                let edge = ((height - f64::from(whole)) * 255.0) as u8;
                self.blend(x + column, row(whole), color, edge.max(EDGE_OPACITY as u8));
            }
            // Under a pixel high there is no row to outline on this side.
            if whole > 0 {
                self.blend(x + column, row(whole - 1), outline, 255);
            }
        }
    }

    /// `color` over the pixel at `(x, y)` at `coverage` (0–255), if inside.
    /// Draw `image` with its top-left corner at `(x, y)`, each of its pixels
    /// `scale` pixels wide (the nearest one), blended over what is there by
    /// its opacity; parts outside the canvas are dropped.
    pub fn draw_image(&mut self, x: i32, y: i32, image: &Image, scale: Scale) {
        let quarters = scale.quarters();
        let source = |at: i32| usize::try_from(at * Scale::ONE.quarters() / quarters).unwrap_or(0);
        for row in 0..image.height_at(scale) {
            for col in 0..image.width_at(scale) {
                if let Some((color, alpha)) = image.pixel(source(col), source(row)) {
                    self.blend(x + col, y + row, color, alpha);
                }
            }
        }
    }

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

/// The colour `index / last` of the way from `from` to `to`.
fn lerp(from: Rgb, to: Rgb, index: i32, last: i32) -> Rgb {
    let (index, last) = (index.max(0) as u32, last.max(1) as u32);
    let channel = |shift: u32| {
        let (a, b) = ((from >> shift) & 0xff, (to >> shift) & 0xff);
        ((a * (last - index.min(last)) + b * index.min(last) + last / 2) / last) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Width of `text` drawn at `scale`.
pub fn text_width(text: &str, scale: Scale) -> i32 {
    text.chars().count() as i32 * char_width(scale)
}

/// Width of one character at `scale`.
pub fn char_width(scale: Scale) -> i32 {
    font::advance(scale.quarters())
}

/// `text` cut to `width` pixels at `scale`.
pub fn fit(text: &str, width: i32, scale: Scale) -> String {
    text.chars().take(chars_in(width, scale)).collect()
}

/// How many characters fit in `width` pixels at `scale`.
pub fn chars_in(width: i32, scale: Scale) -> usize {
    usize::try_from(width / char_width(scale).max(1)).unwrap_or(0)
}

/// Height of a line of the small font at `scale`.
pub fn small_cell(scale: Scale) -> i32 {
    font::SIZE * small(scale)
}

/// Width of one character of the small font at `scale`.
pub fn small_char_width(scale: Scale) -> i32 {
    font::advance(small(scale).quarters())
}

/// The scale of the small font's cells at `scale`.
fn small(scale: Scale) -> Scale {
    scale.part(font::SMALL_QUARTERS)
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
    fn rounded_gradient_smooths_corners_and_blends_top_to_bottom() {
        let mut pixels = vec![0; 8 * 8];
        let mut canvas = Canvas::new(&mut pixels, 8, 8);
        canvas.fill_rounded_gradient(0, 0, 8, 8, (0x40_40_40, 0xff_ff_ff), 3);
        assert!(pixels[0] < 0x20_20_20, "the corner is mostly background");
        assert_ne!(pixels[2], 0, "the corner's edge is partly covered");
        assert!(pixels[3 * 8 + 4] < pixels[4 * 8 + 4], "darker on top");
        assert_eq!(pixels[7 * 8 + 4], 0xff_ff_ff);
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
    fn an_image_covers_its_size_at_the_scale() {
        let mut pixels = vec![0x123456; 700 * 140];
        let mut canvas = Canvas::new(&mut pixels, 700, 140);
        canvas.draw_image(10, 5, &crate::image::LOGO, Scale::whole(2));
        let changed: Vec<(usize, usize)> =
            pixels.iter().enumerate().filter(|&(_, &p)| p != 0x123456).map(|(i, _)| (i % 700, i / 700)).collect();
        assert!(!changed.is_empty());
        assert!(changed.iter().all(|&(x, y)| (10..650).contains(&x) && (5..133).contains(&y)));
    }

    #[test]
    fn text_is_blended_inside_its_cells() {
        let mut pixels = vec![0; 32 * 16];
        let mut canvas = Canvas::new(&mut pixels, 32, 16);
        assert_eq!(canvas.text(0, 0, "Hé", Scale::whole(2), 0xff_ff_ff), 24);
        let lit =
            |x0: usize, x1: usize| (0..16).any(|y| pixels[y * 32 + x0..y * 32 + x1].iter().any(|&p| p > 0x80_80_80));
        assert!(lit(0, 12) && lit(12, 24), "both cells drawn");
        assert!(!lit(24, 32), "nothing past them");
        // Coverage blends: some pixels are neither background nor full colour.
        assert!(pixels.iter().any(|&p| p != 0 && p != 0xff_ff_ff));
        assert_eq!(text_width("abc", Scale::whole(2)), 36);
    }
}
