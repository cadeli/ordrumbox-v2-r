//! Software drawing into the window's pixel buffer (`0x00RRGGBB` per pixel).
//!
//! The whole UI is drawn here, without a GUI framework: rectangles now, then
//! lines, knob arcs and the 8 px bitmap font as the panels are ported.

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

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
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
}
