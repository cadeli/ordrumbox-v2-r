//! Scrollbars around the UI when the window is smaller than the layout at
//! its zoom: the zoom never depends on the window's size. The UI is then
//! drawn whole into a buffer of its own, and the window shows the part the
//! scrollbars choose.

use crate::canvas::Canvas;
use crate::scale::Scale;
use crate::theme;

/// The smallest window, in logical pixels (scale units at the display's
/// scale), app or plugin: what the layout needs past it scrolls.
pub const MIN_WINDOW: (u32, u32) = (320, 200);

/// A scrollbar's thickness in scale units, at the display's scale (the zoom
/// leaves it alone).
const THICKNESS: i32 = 12;
/// The shortest thumb, in thicknesses.
const MIN_THUMB: i32 = 2;
/// The pixels one line of the wheel scrolls, in scale units.
const WHEEL_STEP: i32 = 40;

/// The side of a resize grip in the window's bottom-right corner, at the
/// display's scale `display`: a scrollbar's thickness, so that the bars
/// stop short of it ([`Viewport::new`] with `grip`).
#[must_use]
pub fn grip_side(display: Scale) -> i32 {
    THICKNESS * display
}

/// One of the two directions the UI scrolls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    /// The part of `(x, y)` along this axis.
    const fn of(self, (x, y): (i32, i32)) -> i32 {
        match self {
            Self::Horizontal => x,
            Self::Vertical => y,
        }
    }
}

/// How the window shows the UI: all of it, or a part with scrollbars along
/// its right and bottom edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// The UI's size in pixels: the layout's, or the view's when larger.
    pub content: (i32, i32),
    /// The part of the window that shows the UI: the window less the scrollbars.
    pub view: (i32, i32),
    horizontal: bool,
    vertical: bool,
    thickness: i32,
    /// A resize grip holds the window's bottom-right corner: a bar alone
    /// stops short of it, as it does of the other bar.
    grip: bool,
}

impl Viewport {
    /// A window of `window` pixels showing a layout of `needed` pixels, on a
    /// display at `display`; `grip`: a resize grip in its bottom-right
    /// corner, which the bars leave free.
    #[must_use]
    pub fn new(window: (i32, i32), needed: (i32, i32), display: Scale, grip: bool) -> Self {
        let thickness = THICKNESS * display;
        let (width, height) = (window.0.max(0), window.1.max(0));
        let less = |bar: bool| if bar { thickness } else { 0 };
        // A bar takes room from the other direction, which may need a bar too.
        let horizontal = needed.0 > width;
        let vertical = needed.1 > height - less(horizontal);
        let horizontal = horizontal || needed.0 > width - less(vertical);
        let vertical = needed.1 > height - less(horizontal);
        let view = ((width - less(vertical)).max(0), (height - less(horizontal)).max(0));
        Self { content: (needed.0.max(view.0), needed.1.max(view.1)), view, horizontal, vertical, thickness, grip }
    }

    /// The window shows only a part of the UI.
    #[must_use]
    pub const fn scrolls(&self) -> bool {
        self.horizontal || self.vertical
    }

    const fn shows(&self, axis: Axis) -> bool {
        match axis {
            Axis::Horizontal => self.horizontal,
            Axis::Vertical => self.vertical,
        }
    }

    /// The length of `axis`'s bar: along the view, short of the grip's
    /// corner when the other bar does not already leave it free.
    const fn track(&self, axis: Axis) -> i32 {
        let other = match axis {
            Axis::Horizontal => self.vertical,
            Axis::Vertical => self.horizontal,
        };
        let length = axis.of(self.view) - if self.grip && !other { self.thickness } else { 0 };
        if length > 0 { length } else { 0 }
    }

    /// How far the UI scrolls along `axis`.
    const fn range(&self, axis: Axis) -> i32 {
        let range = axis.of(self.content) - axis.of(self.view);
        if range > 0 { range } else { 0 }
    }

    /// The thumb on `axis`'s bar for `offset`: where it starts along the
    /// bar, and its length.
    fn thumb(&self, axis: Axis, offset: i32) -> (i32, i32) {
        let (track, content, range) = (self.track(axis), axis.of(self.content), self.range(axis));
        if range == 0 || content == 0 {
            return (0, track);
        }
        let length = (track * track / content).max(MIN_THUMB * self.thickness).min(track);
        (((track - length) * offset).checked_div(range).unwrap_or(0), length)
    }

    /// The bar under the window's `(x, y)` and the place along it; `None`
    /// off the bars.
    fn bar_at(&self, (x, y): (i32, i32)) -> Option<(Axis, i32)> {
        let (view_w, view_h) = self.view;
        let across = |at: i32, from: i32| at >= from && at < from + self.thickness;
        if self.horizontal && x < self.track(Axis::Horizontal) && across(y, view_h) {
            Some((Axis::Horizontal, x))
        } else if self.vertical && y < self.track(Axis::Vertical) && across(x, view_w) {
            Some((Axis::Vertical, y))
        } else {
            None
        }
    }

    /// The window's `(x, y)` is past the view: on a bar or in the corner
    /// between them.
    const fn off_view(&self, (x, y): (i32, i32)) -> bool {
        self.scrolls() && (x >= self.view.0 || y >= self.view.1)
    }
}

/// A thumb held by the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    axis: Axis,
    /// The pointer along the bar when pressed.
    from: i32,
    /// The offset when pressed.
    offset: i32,
}

/// Where the window shows the UI from, and the thumb held, if one is.
#[derive(Debug, Default)]
pub struct Scroll {
    /// The UI's pixel at the view's top-left corner.
    offset: (i32, i32),
    drag: Option<Drag>,
    /// The UI drawn whole while the window shows a part of it.
    content: Vec<u32>,
}

impl Scroll {
    /// The offset kept within what `viewport` scrolls.
    pub fn fit(&mut self, viewport: &Viewport) {
        let clamp = |offset: i32, axis| offset.clamp(0, viewport.range(axis));
        self.offset = (clamp(self.offset.0, Axis::Horizontal), clamp(self.offset.1, Axis::Vertical));
    }

    /// The UI's pixel under the window's `(x, y)`.
    #[must_use]
    pub const fn to_content(&self, (x, y): (i32, i32)) -> (i32, i32) {
        (x + self.offset.0, y + self.offset.1)
    }

    fn set(&mut self, viewport: &Viewport, axis: Axis, offset: i32) -> bool {
        let before = self.offset;
        match axis {
            Axis::Horizontal => self.offset.0 = offset,
            Axis::Vertical => self.offset.1 = offset,
        }
        self.fit(viewport);
        self.offset != before
    }

    /// The left button pressed at the window's `(x, y)`: a thumb is held, or
    /// the bar pages toward the pointer. Whether the press was the bars'.
    pub fn press(&mut self, viewport: &Viewport, at: (i32, i32)) -> bool {
        let Some((axis, along)) = viewport.bar_at(at) else { return viewport.off_view(at) };
        let offset = axis.of(self.offset);
        let (start, length) = viewport.thumb(axis, offset);
        if (start..start + length).contains(&along) {
            self.drag = Some(Drag { axis, from: along, offset });
        } else {
            let page = axis.of(viewport.view);
            self.set(viewport, axis, if along < start { offset - page } else { offset + page });
        }
        true
    }

    /// A thumb is held.
    #[must_use]
    pub const fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The held thumb follows the pointer, at the window's `(x, y)`; whether
    /// the view moved.
    pub fn drag_to(&mut self, viewport: &Viewport, at: (i32, i32)) -> bool {
        let Some(drag) = self.drag else { return false };
        let (_, length) = viewport.thumb(drag.axis, drag.offset);
        let travel = viewport.track(drag.axis) - length;
        let moved = (drag.axis.of(at) - drag.from) * viewport.range(drag.axis);
        self.set(viewport, drag.axis, drag.offset + moved.checked_div(travel).unwrap_or(0))
    }

    /// The left button released: the thumb is let go; whether one was held.
    pub fn release(&mut self) -> bool {
        self.drag.take().is_some()
    }

    /// The wheel turned by `(columns, lines)` over the UI, which left it
    /// alone; whether the view moved. Without a vertical bar, lines scroll
    /// across.
    pub fn wheel(&mut self, viewport: &Viewport, (columns, lines): (isize, isize), display: Scale) -> bool {
        let step = WHEEL_STEP * display;
        let lines = i32::try_from(lines).unwrap_or(0);
        let columns = i32::try_from(columns).unwrap_or(0);
        let (columns, lines) = if viewport.vertical { (columns, lines) } else { (columns + lines, 0) };
        let mut moved = false;
        if columns != 0 && viewport.horizontal {
            moved |= self.set(viewport, Axis::Horizontal, self.offset.0 - columns * step);
        }
        if lines != 0 && viewport.vertical {
            moved |= self.set(viewport, Axis::Vertical, self.offset.1 - lines * step);
        }
        moved
    }

    /// A buffer of `viewport.content` pixels to draw the UI into.
    pub fn content_buffer(&mut self, viewport: &Viewport) -> &mut [u32] {
        let (width, height) = viewport.content;
        // Sizes of a layout and a window: positive and small.
        self.content.resize(width.unsigned_abs() as usize * height.unsigned_abs() as usize, 0);
        &mut self.content
    }

    /// The view's part of the UI drawn into `pixels` (`width` pixels a row),
    /// then the scrollbars.
    pub fn show(&self, viewport: &Viewport, pixels: &mut [u32], width: usize, height: usize) {
        let content_w = viewport.content.0.unsigned_abs() as usize;
        let (left, top) = (self.offset.0.unsigned_abs() as usize, self.offset.1.unsigned_abs() as usize);
        let view_w = (viewport.view.0.unsigned_abs() as usize).min(width);
        let rows = self.content.chunks_exact(content_w.max(1)).skip(top);
        for (row, source) in pixels.chunks_exact_mut(width.max(1)).zip(rows).take(viewport.view.1.max(0) as usize) {
            row[..view_w].copy_from_slice(&source[left..left + view_w]);
        }
        let mut canvas = Canvas::new(pixels, width, height);
        let t = viewport.thickness;
        let (view_w, view_h) = viewport.view;
        for axis in [Axis::Horizontal, Axis::Vertical] {
            if !viewport.shows(axis) {
                continue;
            }
            let (start, length) = viewport.thumb(axis, axis.of(self.offset));
            let held = self.drag.is_some_and(|d| d.axis == axis);
            let thumb = if held { theme::text() } else { theme::muted() };
            let inset = (t / 4).max(1);
            let track = viewport.track(axis);
            match axis {
                Axis::Horizontal => {
                    canvas.fill_rect(0, view_h, track, t, theme::panel());
                    canvas.fill_rect(start, view_h + inset, length, t - 2 * inset, thumb);
                }
                Axis::Vertical => {
                    canvas.fill_rect(view_w, 0, t, track, theme::panel());
                    canvas.fill_rect(view_w + inset, start, t - 2 * inset, length, thumb);
                }
            }
        }
        if viewport.horizontal && viewport.vertical {
            canvas.fill_rect(view_w, view_h, t, t, theme::panel());
        } else if viewport.grip && viewport.horizontal {
            canvas.fill_rect(view_w - t, view_h, t, t, theme::panel());
        } else if viewport.grip && viewport.vertical {
            canvas.fill_rect(view_w, view_h - t, t, t, theme::panel());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_show_only_where_the_layout_overflows() {
        let fits = Viewport::new((800, 600), (800, 600), Scale::ONE, false);
        assert!(!fits.scrolls());
        assert_eq!((fits.content, fits.view), ((800, 600), (800, 600)));
        let taller = Viewport::new((800, 600), (700, 900), Scale::ONE, false);
        assert_eq!((taller.horizontal, taller.vertical), (false, true));
        assert_eq!(taller.view, (800 - THICKNESS, 600));
        assert_eq!(taller.content, (800 - THICKNESS, 900), "the UI fills the view across");
        // The vertical bar takes the width the layout needed: a bar across too.
        let both = Viewport::new((800, 600), (795, 900), Scale::ONE, false);
        assert_eq!((both.horizontal, both.vertical), (true, true));
        assert_eq!(both.view, (800 - THICKNESS, 600 - THICKNESS));
        assert_eq!(Viewport::new((800, 600), (700, 900), Scale::whole(2), false).view.0, 800 - 2 * THICKNESS);
    }

    #[test]
    fn a_thumb_drags_the_view_from_end_to_end() {
        let viewport = Viewport::new((400, 300), (300, 1200), Scale::ONE, false);
        let mut scroll = Scroll::default();
        let bar_x = viewport.view.0 + 1;
        assert!(scroll.press(&viewport, (bar_x, 5)), "the thumb, at the top");
        assert!(scroll.dragging());
        assert!(scroll.drag_to(&viewport, (bar_x, 1000)));
        assert_eq!(scroll.to_content((0, 0)), (0, 900), "down to the bottom, no further");
        assert!(scroll.release());
        assert!(scroll.press(&viewport, (bar_x, 2)), "above the thumb: a page up");
        assert_eq!(scroll.to_content((0, 0)), (0, 600));
        assert!(scroll.wheel(&viewport, (0, 1), Scale::ONE));
        assert_eq!(scroll.to_content((0, 0)), (0, 600 - WHEEL_STEP));
        assert!(!scroll.press(&viewport, (10, 10)), "the view is the UI's");
    }

    #[test]
    fn a_bar_stops_short_of_the_grip() {
        let t = THICKNESS;
        // A vertical bar alone: down to the grip's corner, no further.
        let viewport = Viewport::new((400, 300), (300, 1200), Scale::ONE, true);
        assert_eq!(viewport.track(Axis::Vertical), 300 - t);
        assert_eq!(viewport.bar_at((400 - 1, 300 - t - 1)), Some((Axis::Vertical, 300 - t - 1)));
        assert_eq!(viewport.bar_at((400 - 1, 300 - t)), None, "the grip's");
        let mut scroll = Scroll::default();
        assert!(scroll.press(&viewport, (400 - 1, 2)));
        assert!(scroll.drag_to(&viewport, (400 - 1, 1000)));
        assert_eq!(scroll.to_content((0, 0)), (0, 900), "the whole range still");
        let mut pixels = vec![1; 400 * 300];
        scroll.content_buffer(&viewport);
        scroll.show(&viewport, &mut pixels, 400, 300);
        let at = |x: usize, y: usize| pixels[y * 400 + x];
        assert_eq!(at(399, 299), theme::panel(), "the corner left to the grip, painted as the bar");
        // A horizontal bar alone, and both: the corner is free either way.
        let across = Viewport::new((400, 300), (900, 200), Scale::ONE, true);
        assert_eq!(across.track(Axis::Horizontal), 400 - t);
        let both = Viewport::new((400, 300), (900, 900), Scale::ONE, true);
        assert_eq!((both.track(Axis::Horizontal), both.track(Axis::Vertical)), (400 - t, 300 - t));
        assert_eq!(grip_side(Scale::whole(2)), 2 * t);
    }

    #[test]
    fn the_view_shows_the_part_scrolled_to() {
        let viewport = Viewport::new((4, 3), (1, 6), Scale::from_quarters(1), false);
        assert_eq!(viewport.view, (1, 3), "a thickness of 3 at ×¼");
        let mut scroll = Scroll::default();
        for (i, pixel) in scroll.content_buffer(&viewport).iter_mut().enumerate() {
            *pixel = i as u32 + 100;
        }
        scroll.offset = (0, 2);
        let mut pixels = vec![0; 12];
        scroll.show(&viewport, &mut pixels, 4, 3);
        let shown: Vec<u32> = pixels.as_chunks::<4>().0.iter().map(|row| row[0]).collect();
        assert_eq!(shown, [102, 103, 104], "rows 2 to 4 of a one-pixel-wide UI");
    }
}
