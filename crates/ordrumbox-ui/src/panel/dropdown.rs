//! The open drop-down list (and the right-click menu), drawn over everything.

use super::*;
use crate::menu_action::MenuAction;
use crate::scroll_thumb;

/// Rows a drop-down list shows at once.
pub(super) const LIST_ROWS: usize = 14;

/// The scroll bar's width, in scale units: the kit table's.
const BAR: i32 = 8;

/// The shortest thumb, in scale units.
const MIN_THUMB: i32 = 12;

/// An open drop-down list (or a right-click menu, with a title).
#[derive(Debug, Clone, PartialEq)]
pub struct Dropdown {
    pub id: Id,
    /// A line over the options (`ContextMenu`'s title).
    pub title: Option<String>,
    /// The select's place: the list opens under it (or over it near the bottom).
    pub anchor: Rect,
    pub choices: Choices,
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
    /// The key of each line that has one, shown at its right end (a menu's
    /// lines only; empty: none).
    pub shortcuts: Vec<String>,
    /// The scroll bar's thumb is held by the mouse.
    pub bar_held: bool,
}

/// What the options of an open list stand for, one each.
#[derive(Debug, Clone, PartialEq)]
pub enum Choices {
    /// A select's values, set as the select sets them.
    Values(Vec<String>),
    /// A menu's actions.
    Actions(Vec<MenuAction>),
}

/// The label end of a line that opens a submenu.
pub const SUBMENU_MARK: &str = " >";

/// The mark of what opens a list or a menu: a select's, and a name's under
/// the pointer when a right-click opens its menu.
pub const MENU_MARK: &str = "v";

impl Dropdown {
    /// The list of select `id` under `anchor`: its `values`, each shown by
    /// its label, `selected` highlighted.
    pub fn new(id: Id, anchor: Rect, values: Vec<String>, labels: Vec<String>, selected: usize) -> Self {
        Self::with_choices(id, anchor, Choices::Values(values), labels, selected)
    }

    /// A menu of actions under `anchor`, opened as a list: its `lines`, each
    /// an action and the label shown, `selected` highlighted.
    pub fn from_actions(
        id: Id,
        anchor: Rect,
        lines: impl IntoIterator<Item = (impl Into<MenuAction>, String)>,
        selected: usize,
    ) -> Self {
        let (actions, labels) = lines.into_iter().map(|(action, label)| (action.into(), label)).unzip();
        Self::with_choices(id, anchor, Choices::Actions(actions), labels, selected)
    }

    fn with_choices(id: Id, anchor: Rect, choices: Choices, labels: Vec<String>, selected: usize) -> Self {
        let mut d = Self {
            id,
            title: None,
            anchor,
            choices,
            labels,
            at: selected,
            first: 0,
            parent: None,
            control: None,
            shortcuts: Vec::new(),
            bar_held: false,
        };
        d.show_at();
        d
    }

    /// A menu of actions at `(x, y)`, under `title`: its `lines`, each an
    /// action and the label shown.
    pub fn menu(
        id: Id,
        x: i32,
        y: i32,
        title: String,
        lines: impl IntoIterator<Item = (impl Into<MenuAction>, String)>,
    ) -> Self {
        Self { title: Some(title), ..Self::from_actions(id, Rect::new(x, y, 0, 0), lines, 0) }
    }

    /// The actions of a menu's lines; none for a select's list.
    #[cfg(test)]
    #[must_use]
    pub fn actions(&self) -> &[MenuAction] {
        match &self.choices {
            Choices::Actions(actions) => actions,
            Choices::Values(_) => &[],
        }
    }

    /// The menu with `shortcuts`, the key of each line (empty: none), shown
    /// at the right end of its line.
    #[must_use]
    pub fn with_shortcuts(self, shortcuts: Vec<String>) -> Self {
        Self { shortcuts, ..self }
    }

    /// The width of the widest shortcut, with the gap before it; 0 without one.
    fn shortcut_width(&self, scale: Scale) -> i32 {
        let widest = self.shortcuts.iter().map(|s| text_width(s, scale)).max().unwrap_or(0);
        if widest > 0 { widest + 16 * scale } else { 0 }
    }

    /// A menu opened by the highlighted line of `parent`, beside it (on its
    /// right, or on its left where the `width` × `height` window ends).
    pub fn submenu(
        parent: Dropdown,
        size: (i32, i32, Scale),
        id: Id,
        title: String,
        lines: impl IntoIterator<Item = (impl Into<MenuAction>, String)>,
    ) -> Self {
        let (width, height, scale) = size;
        let mut menu = Self::menu(id, 0, 0, title, lines);
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
                let i = m.option_at(width, height, scale, x, y);
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

    /// Whether the options are more than the rows: the list has a scroll bar.
    fn scrolls(&self) -> bool {
        self.labels.len() > self.shown()
    }

    /// The width the scroll bar takes at the right of the rows; 0 without one.
    fn bar_width(&self, scale: Scale) -> i32 {
        if self.scrolls() { BAR * scale } else { 0 }
    }

    /// The scroll bar, right of the options, in a `width` × `height` window;
    /// `None` when every option is shown.
    pub fn scroll_bar(&self, width: i32, height: i32, scale: Scale) -> Option<Rect> {
        if !self.scrolls() {
            return None;
        }
        let r = self.rect(width, height, scale);
        let row = row_height(scale);
        let w = BAR * scale;
        let top = r.y + 2 * scale + self.head() as i32 * row;
        Some(Rect::new(r.x + r.w - scale.px() - w, top, w, self.shown() as i32 * row))
    }

    /// Start and length of the thumb on the scroll `bar`.
    pub fn thumb(&self, bar: Rect, scale: Scale) -> (i32, i32) {
        scroll_thumb::thumb((bar.y, bar.h), self.first, self.shown(), self.labels.len(), MIN_THUMB * scale)
    }

    /// The thumb centred on height `y`, the highlight kept in view; whether
    /// the list moved.
    pub fn bar_to(&mut self, size: (i32, i32, Scale), y: i32) -> bool {
        let (width, height, scale) = size;
        let Some(bar) = self.scroll_bar(width, height, scale) else { return false };
        let first = scroll_thumb::first_at((bar.y, bar.h), y, self.shown(), self.labels.len(), MIN_THUMB * scale);
        let moved = first != self.first;
        self.scroll(first as isize - self.first as isize);
        moved
    }

    /// Option `at` highlighted, and shown.
    pub fn highlight(&mut self, at: usize) {
        self.at = at.min(self.labels.len().saturating_sub(1));
        self.show_at();
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
        let letter = letter.to_ascii_lowercase();
        let starts = |i: usize| self.labels[i].chars().next().is_some_and(|c| c.to_ascii_lowercase() == letter);
        if let Some(i) = next_matching(self.labels.len(), self.at, starts) {
            self.at = i;
            self.show_at();
        }
    }

    /// The list's place in a `width` × `height` window.
    pub fn rect(&self, width: i32, height: i32, scale: Scale) -> Rect {
        let row = row_height(scale);
        let label = self.labels.iter().map(|l| text_width(l, scale)).max().unwrap_or(0) + self.shortcut_width(scale);
        let longest = self.title.iter().map(|t| text_width(t, scale)).fold(label, i32::max);
        let w = (longest + 20 * scale + self.bar_width(scale)).max(self.anchor.w).min(width);
        let h = (self.shown() + self.head()) as i32 * row + 4 * scale;
        let x = self.anchor.x.min(width - w).max(0);
        let below = self.anchor.y + self.anchor.h;
        let y = if below + h <= height { below } else { (self.anchor.y - h).max(0) };
        Rect::new(x, y, w, h)
    }

    /// The option under `(x, y)`.
    pub fn option_at(&self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> Option<usize> {
        let r = self.rect(width, height, scale);
        if !r.contains(x, y) || self.scroll_bar(width, height, scale).is_some_and(|bar| x >= bar.x) {
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
        let rows_w = r.w - self.bar_width(scale);
        for (k, label) in self.labels.iter().enumerate().skip(self.first).take(self.shown()) {
            let y = top + (k - self.first) as i32 * row;
            if k == self.at {
                canvas.fill_rect(r.x + scale.px(), y, rows_w - 2 * scale, row, selected_row_color());
            }
            let color = if k == self.at { TITLE } else { theme::text() };
            let text_y = y + (row - 8 * scale) / 2;
            let shortcut = self.shortcuts.get(k).filter(|s| !s.is_empty());
            let room = rows_w - 12 * scale - shortcut.map_or(0, |_| self.shortcut_width(scale));
            canvas.text(r.x + 6 * scale, text_y, &fit(label, room, scale), scale, color);
            if let Some(shortcut) = shortcut {
                let x = r.x + rows_w - 6 * scale - text_width(shortcut, scale);
                canvas.text(x, text_y, shortcut, scale, dim());
            }
        }
        if let Some(bar) = self.scroll_bar(canvas.width(), canvas.height(), scale) {
            scroll_thumb::draw(canvas, bar, self.thumb(bar, scale), scale);
        }
    }
}

fn row_height(scale: Scale) -> i32 {
    12 * scale
}
