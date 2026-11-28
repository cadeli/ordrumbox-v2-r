//! Images built into the program: 8-bit RGBA pixels decoded at build time
//! (`build.rs`), and their size at a UI scale.

use crate::scale::Scale;

/// An RGBA image, row by row, drawn at its pixel size times the UI scale.
#[derive(Debug, Clone, Copy)]
pub struct Image {
    width: usize,
    height: usize,
    rgba: &'static [u8],
}

/// The orDrumbox logo (`assets/images/logo2.png`), at the top of Help > About.
pub const LOGO: Image = Image { width: 320, height: 64, rgba: include_bytes!(concat!(env!("OUT_DIR"), "/logo.rgba")) };
/// The orDrumbox emblem, a round badge (`assets/images/logo.png`), in the
/// synth editor's last card.
pub const EMBLEM: Image =
    Image { width: 192, height: 192, rgba: include_bytes!(concat!(env!("OUT_DIR"), "/emblem.rgba")) };

impl Image {
    /// Its size in source pixels.
    #[must_use]
    pub const fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Its width in pixels at `scale`.
    #[must_use]
    pub fn width_at(&self, scale: Scale) -> i32 {
        scaled(self.width, scale)
    }

    /// Its height in pixels at `scale`.
    #[must_use]
    pub fn height_at(&self, scale: Scale) -> i32 {
        scaled(self.height, scale)
    }

    /// The colour (`0x00RRGGBB`) and opacity of the pixel at `(x, y)`, or
    /// `None` outside the image.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Option<(u32, u8)> {
        if x >= self.width {
            return None;
        }
        let start = (y * self.width + x) * 4;
        let &[r, g, b, a] = self.rgba.get(start..start + 4)? else { return None };
        Some(((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b), a))
    }
}

/// `pixels` image pixels at `scale`, one pixel at ×1.
fn scaled(pixels: usize, scale: Scale) -> i32 {
    i32::try_from(pixels).map_or(i32::MAX, |p| p.saturating_mul(scale.quarters()) / Scale::ONE.quarters())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_pixels_fill_the_logo() {
        assert_eq!(EMBLEM.rgba.len(), EMBLEM.width * EMBLEM.height * 4);
        assert_eq!(LOGO.rgba.len(), LOGO.width * LOGO.height * 4);
        assert!(LOGO.pixel(LOGO.width - 1, LOGO.height - 1).is_some());
        assert!(LOGO.pixel(LOGO.width, 0).is_none());
        assert!(LOGO.pixel(0, LOGO.height).is_none());
    }

    #[test]
    fn the_logo_grows_with_the_scale() {
        assert_eq!((LOGO.width_at(Scale::ONE), LOGO.height_at(Scale::ONE)), (320, 64));
        assert_eq!((LOGO.width_at(Scale::whole(2)), LOGO.height_at(Scale::whole(2))), (640, 128));
        assert_eq!(LOGO.width_at(Scale::from_quarters(5)), 400);
    }
}
