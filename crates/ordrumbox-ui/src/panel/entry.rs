//! The number or text typed for a control, over everything: one field, or a
//! form of labelled fields (a pattern's name, description and tags).

use super::*;

/// Where a text entry's first field is in it (unscaled): the mouse that
/// opened it is there.
const ENTRY_FIELD: (i32, i32) = (12, 22);
/// A field's height, and how far apart a form's fields are (unscaled).
const FIELD_H: i32 = 12;
const FIELD_STEP: i32 = 16;
/// The narrowest entry, and the narrowest form (unscaled): a form's
/// description wants room.
const ENTRY_MIN_W: i32 = 220;
const FORM_MIN_W: i32 = 380;
/// The space after a form's labels (unscaled).
const LABEL_GAP: i32 = 6;

/// One field of an entry: its label (empty for a lone field) and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub label: String,
    pub text: String,
}

/// The number typed for a control (right-click or Enter), or a text (a
/// name, a description: `spec` is `None`), or a form of texts.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: Id,
    pub spec: Option<Spec>,
    pub title: String,
    /// One field, or a form's, in order.
    fields: Vec<Field>,
    /// The field the keys type in.
    active: usize,
    /// The mouse when it opened: its first field starts there.
    pub at: (i32, i32),
}

impl Entry {
    /// An entry of one field holding `text`.
    #[must_use]
    pub fn new(id: Id, spec: Option<Spec>, title: String, text: String, at: (i32, i32)) -> Self {
        Self { id, spec, title, fields: vec![Field { label: String::new(), text }], active: 0, at }
    }

    /// A form of text `fields`, the first one typed in.
    #[must_use]
    pub fn form(id: Id, title: String, fields: Vec<Field>, at: (i32, i32)) -> Self {
        Self { id, spec: None, title, fields, active: 0, at }
    }

    /// The text of the field typed in.
    #[must_use]
    pub fn text(&self) -> &str {
        self.fields.get(self.active).map_or("", |f| f.text.as_str())
    }

    /// The text of the field typed in, to type.
    pub fn text_mut(&mut self) -> Option<&mut String> {
        self.fields.get_mut(self.active).map(|f| &mut f.text)
    }

    /// Every field's text, in order.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|f| f.text.as_str())
    }

    /// Whether it is a form of several fields.
    #[must_use]
    pub const fn is_form(&self) -> bool {
        self.fields.len() > 1
    }

    /// The field `delta` away typed in, round the form.
    pub fn step_field(&mut self, delta: isize) {
        let count = self.fields.len().max(1);
        self.active = self.active.saturating_add_signed(delta.rem_euclid(count as isize)) % count;
    }

    /// The field at `(x, y)` typed in, if one is there.
    pub fn click(&mut self, x: i32, y: i32, width: i32, height: i32, scale: Scale) {
        let r = self.rect(width, height, scale);
        if let Some(i) = (0..self.fields.len()).find(|&i| self.field_rect(r, i, scale).contains(x, y)) {
            self.active = i;
        }
    }

    /// The entry with its first field at the mouse, inside the `width` ×
    /// `height` window.
    #[must_use]
    pub fn rect(&self, width: i32, height: i32, scale: Scale) -> Rect {
        let min_w = if self.is_form() { FORM_MIN_W } else { ENTRY_MIN_W };
        let w = (text_width(&self.title, scale) + 32 * scale).max(min_w * scale).min(width);
        let h = (52 + self.extra_rows() * FIELD_STEP) * scale;
        let (x, y) = (self.at.0 - ENTRY_FIELD.0 * scale, self.at.1 - ENTRY_FIELD.1 * scale);
        Rect::new(x.clamp(0, (width - w).max(0)), y.clamp(0, (height - h).max(0)), w, h)
    }

    /// The rows under the first field.
    fn extra_rows(&self) -> i32 {
        i32::try_from(self.fields.len().saturating_sub(1)).unwrap_or(0)
    }

    /// The labels' column: as wide as the widest, none without labels.
    fn label_w(&self, scale: Scale) -> i32 {
        let widest = self.fields.iter().map(|f| text_width(&f.label, scale)).max().unwrap_or(0);
        if widest > 0 { widest + LABEL_GAP * scale } else { 0 }
    }

    /// Field `index`'s box in the entry `r`.
    fn field_rect(&self, r: Rect, index: usize, scale: Scale) -> Rect {
        let label_w = self.label_w(scale);
        let row = i32::try_from(index).unwrap_or(0);
        let y = r.y + (ENTRY_FIELD.1 + row * FIELD_STEP) * scale;
        Rect::new(r.x + ENTRY_FIELD.0 * scale + label_w, y, r.w - 24 * scale - label_w, FIELD_H * scale)
    }

    pub fn draw(&self, canvas: &mut Canvas, scale: Scale) {
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x - 4 * scale, r.y - 4 * scale, r.w + 8 * scale, r.h + 8 * scale, theme::shade());
        canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
        canvas.fill_rect(r.x + scale.px(), r.y + scale.px(), r.w - 2 * scale, r.h - 2 * scale, background_color());
        canvas.text(r.x + 12 * scale, r.y + 8 * scale, &self.title, scale, TITLE);
        for (i, f) in self.fields.iter().enumerate() {
            let field = self.field_rect(r, i, scale);
            canvas.text(r.x + ENTRY_FIELD.0 * scale, field.y + 2 * scale, &f.label, scale, dim());
            canvas.fill_rect(field.x, field.y, field.w, field.h, field_color());
            let shown = tail(&f.text, field.w - 8 * scale, scale);
            let x = canvas.text(field.x + 4 * scale, field.y + 2 * scale, shown, scale, theme::text());
            if i == self.active {
                canvas.fill_rect(x + scale.px(), field.y + 2 * scale, scale.px(), 8 * scale, CARET);
            }
        }
        let hint =
            if self.is_form() { "TAB: next field   ENTER: set   ESC: cancel" } else { "ENTER: set   ESC: cancel" };
        let hint_y = r.y + (40 + self.extra_rows() * FIELD_STEP) * scale;
        canvas.text(r.x + 12 * scale, hint_y, hint, scale, dim());
    }
}

/// The end of `text` that fits in `width`: the end is where the caret is.
fn tail(text: &str, width: i32, scale: Scale) -> &str {
    let mut shown = text;
    while text_width(shown, scale) > width {
        let mut chars = shown.chars();
        chars.next();
        shown = chars.as_str();
    }
    shown
}
