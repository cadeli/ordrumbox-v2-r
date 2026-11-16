//! The open drop-down list (and the right-click menu), drawn over everything.

use super::*;

/// Rows a drop-down list shows at once.
pub(super) const LIST_ROWS: usize = 14;

/// An open drop-down list (or a right-click menu, with a title).
#[derive(Debug, Clone, PartialEq)]
pub struct Dropdown {
    pub id: Id,
    /// A line over the options (`ContextMenu`'s title).
    pub title: Option<String>,
    /// The select's place: the list opens under it (or over it near the bottom).
    pub anchor: Rect,
    pub values: Vec<String>,
    pub labels: Vec<String>,
    /// Highlighted option.
    pub at: usize,
    /// First option shown.
    pub first: usize,
    /// The menu this one is a submenu of, still shown, its line that opened
    /// this one highlighted.
    pub parent: Option<Box<Dropdown>>,
    /// The control a `ControlMenu` is for.
    pub control: Option<Id>,
}

/// The label end of a line that opens a submenu.
pub const SUBMENU_MARK: &str = " >";

impl Dropdown {
    pub fn new(id: Id, anchor: Rect, values: Vec<String>, labels: Vec<String>, selected: usize) -> Self {
        let mut d =
            Self { id, title: None, anchor, values, labels, at: selected, first: 0, parent: None, control: None };
        d.show_at();
        d
    }

    /// A menu of actions at `(x, y)`, under `title`.
    pub fn menu(id: Id, x: i32, y: i32, title: String, values: Vec<String>, labels: Vec<String>) -> Self {
        Self { title: Some(title), ..Self::new(id, Rect::new(x, y, 0, 0), values, labels, 0) }
    }

    /// A menu opened by the highlighted line of `parent`, beside it (on its
    /// right, or on its left where the `width` × `height` window ends).
    pub fn submenu(
        parent: Dropdown,
        size: (i32, i32, Scale),
        id: Id,
        title: String,
        values: Vec<String>,
        labels: Vec<String>,
    ) -> Self {
        let (width, height, scale) = size;
        let mut menu = Self::menu(id, 0, 0, title, values, labels);
        let beside = parent.rect(width, height, scale);
        let w = menu.rect(width, height, scale).w;
        let row = parent.at.saturating_sub(parent.first) + parent.head();
        let y = beside.y + 2 * scale + row as i32 * row_height(scale) - 2 * scale;
        let right = beside.x + beside.w;
        let x = if right + w <= width { right } else { (beside.x - w).max(0) };
        menu.anchor = Rect::new(x, y, 0, 0);
        menu.parent = Some(Box::new(parent));
        menu
    }

    /// Whether the highlighted line opens a submenu.
    pub fn opens_submenu(&self) -> bool {
        self.labels.get(self.at).is_some_and(|l| l.ends_with(SUBMENU_MARK))
    }

    /// Back to the menu above, `None` when there is none.
    pub fn back(self) -> Option<Dropdown> {
        self.parent.map(|p| *p)
    }

    /// The menu above whose list is at `(x, y)` (the nearest first), and the
    /// option there: a click on it goes back to it.
    pub fn parent_at(self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> Option<(Dropdown, Option<usize>)> {
        let mut menu = self.back();
        while let Some(m) = menu {
            if m.rect(width, height, scale).contains(x, y) {
                let i = m.hit(width, height, scale, x, y);
                return Some((m, i));
            }
            menu = m.back();
        }
        None
    }

    /// Rows over the options (the title).
    fn head(&self) -> usize {
        usize::from(self.title.is_some())
    }

    fn shown(&self) -> usize {
        self.labels.len().min(LIST_ROWS)
    }

    fn show_at(&mut self) {
        if self.at < self.first {
            self.first = self.at;
        } else if self.at >= self.first + self.shown() {
            self.first = self.at + 1 - self.shown();
        }
    }

    pub fn step(&mut self, delta: isize) {
        if self.labels.is_empty() {
            return;
        }
        self.at = (self.at as isize + delta).clamp(0, self.labels.len() as isize - 1) as usize;
        self.show_at();
    }

    /// Scroll by `delta` rows (the wheel), the highlight kept in view.
    pub fn scroll(&mut self, delta: isize) {
        let max = self.labels.len().saturating_sub(self.shown());
        self.first = (self.first as isize + delta).clamp(0, max as isize) as usize;
        self.at = self.at.clamp(self.first, self.first + self.shown().saturating_sub(1));
    }

    /// The next option starting with `letter` (case-insensitive), after the highlighted one.
    pub fn jump(&mut self, letter: char) {
        let n = self.labels.len();
        let letter = letter.to_ascii_lowercase();
        if let Some(i) = (1..=n)
            .map(|k| (self.at + k) % n)
            .find(|&i| self.labels[i].chars().next().is_some_and(|c| c.to_ascii_lowercase() == letter))
        {
            self.at = i;
            self.show_at();
        }
    }

    /// The list's place in a `width` × `height` window.
    pub fn rect(&self, width: i32, height: i32, scale: Scale) -> Rect {
        let row = row_height(scale);
        let longest = self.labels.iter().chain(&self.title).map(|l| text_width(l, scale)).max().unwrap_or(0);
        let w = (longest + 20 * scale).max(self.anchor.w).min(width);
        let h = (self.shown() + self.head()) as i32 * row + 4 * scale;
        let x = self.anchor.x.min(width - w).max(0);
        let below = self.anchor.y + self.anchor.h;
        let y = if below + h <= height { below } else { (self.anchor.y - h).max(0) };
        Rect::new(x, y, w, h)
    }

    /// The option under `(x, y)`.
    pub fn hit(&self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> Option<usize> {
        let r = self.rect(width, height, scale);
        if !r.contains(x, y) {
            return None;
        }
        let row = row_at(y, r.y + 2 * scale, row_height(scale))?.checked_sub(self.head())?;
        let i = self.first + row;
        (i < self.labels.len()).then_some(i)
    }

    /// The menus above first, then this one over them.
    pub fn draw(&self, canvas: &mut Canvas, scale: Scale) {
        if let Some(parent) = &self.parent {
            parent.draw(canvas, scale);
        }
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
        canvas.fill_rect(r.x + scale.px(), r.y + scale.px(), r.w - 2 * scale, r.h - 2 * scale, background_color());
        let row = row_height(scale);
        if let Some(title) = &self.title {
            let y = r.y + 2 * scale + (row - 8 * scale) / 2;
            canvas.text(r.x + 6 * scale, y, &fit(title, r.w - 12 * scale, scale), scale, dim());
        }
        let top = r.y + 2 * scale + self.head() as i32 * row;
        for (k, label) in self.labels.iter().enumerate().skip(self.first).take(self.shown()) {
            let y = top + (k - self.first) as i32 * row;
            if k == self.at {
                canvas.fill_rect(r.x + scale.px(), y, r.w - 2 * scale, row, selected_row_color());
            }
            let color = if k == self.at { TITLE } else { theme::text() };
            canvas.text(r.x + 6 * scale, y + (row - 8 * scale) / 2, &fit(label, r.w - 12 * scale, scale), scale, color);
        }
        if self.labels.len() > self.shown() {
            // Scroll bar.
            let total = self.labels.len() as i32;
            let track = r.h - 4 * scale - self.head() as i32 * row;
            let top = top + track * self.first as i32 / total;
            let len = (track * self.shown() as i32 / total).max(4 * scale);
            canvas.fill_rect(r.x + r.w - 4 * scale, top, 2 * scale, len, dim());
        }
    }
}

fn row_height(scale: Scale) -> i32 {
    12 * scale
}
