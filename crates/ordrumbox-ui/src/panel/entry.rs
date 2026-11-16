//! The number or text typed for a control, over everything.

use super::*;

/// Where a text entry's field is in it (unscaled): the mouse that opened it is there.
const ENTRY_FIELD: (i32, i32) = (12, 22);

/// The number typed for a control (`promptNumericInput`, right-click or
/// Enter), or a text (a name, a description: `spec` is `None`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: Id,
    pub spec: Option<Spec>,
    pub title: String,
    pub text: String,
    /// The mouse when it opened: its field starts there.
    pub at: (i32, i32),
}

impl Entry {
    /// The entry with its field at the mouse, inside the `width` × `height` window.
    pub fn rect(&self, width: i32, height: i32, scale: Scale) -> Rect {
        let w = (text_width(&self.title, scale) + 32 * scale).max(220 * scale).min(width);
        let h = 52 * scale;
        let (x, y) = (self.at.0 - ENTRY_FIELD.0 * scale, self.at.1 - ENTRY_FIELD.1 * scale);
        Rect::new(x.clamp(0, (width - w).max(0)), y.clamp(0, (height - h).max(0)), w, h)
    }

    pub fn draw(&self, canvas: &mut Canvas, scale: Scale) {
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x - 4 * scale, r.y - 4 * scale, r.w + 8 * scale, r.h + 8 * scale, theme::shade());
        canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
        canvas.fill_rect(r.x + scale.px(), r.y + scale.px(), r.w - 2 * scale, r.h - 2 * scale, background_color());
        canvas.text(r.x + 12 * scale, r.y + 8 * scale, &self.title, scale, TITLE);
        let field = Rect::new(r.x + ENTRY_FIELD.0 * scale, r.y + ENTRY_FIELD.1 * scale, r.w - 24 * scale, 12 * scale);
        canvas.fill_rect(field.x, field.y, field.w, field.h, field_color());
        let x = canvas.text(field.x + 4 * scale, field.y + 2 * scale, &self.text, scale, theme::text());
        canvas.fill_rect(x + scale.px(), field.y + 2 * scale, scale.px(), 8 * scale, CARET);
        canvas.text(r.x + 12 * scale, r.y + 40 * scale, "ENTER: set   ESC: cancel", scale, dim());
    }
}
