//! Where the menu bar goes and how it is drawn, the library field included.

use super::{Item, Menu, Open};
use crate::canvas::{Canvas, Rgb, char_width, fit, text_width};
use crate::scale::Scale;
use crate::theme;

// ─── Geometry ────────────────────────────────────────────────────────────────

pub(super) fn bar_background_color() -> Rgb {
    theme::panel()
}
pub(super) fn panel_color() -> Rgb {
    theme::panel_2()
}
pub(super) fn border_color() -> Rgb {
    theme::line()
}
pub(super) fn highlight_color() -> Rgb {
    theme::highlight()
}
pub(super) fn dim() -> Rgb {
    theme::muted()
}
pub(super) fn disabled_color() -> Rgb {
    theme::disabled()
}
pub(super) const ACCENT: Rgb = theme::ACCENT;

/// Height of the menu bar.
pub fn bar_height(scale: Scale) -> i32 {
    14 * scale
}

pub(super) fn item_height(scale: Scale) -> i32 {
    13 * scale
}

pub(super) fn separator_height(scale: Scale) -> i32 {
    7 * scale
}

/// Left edge and width of each title in the bar.
pub(super) fn titles(menus: &[Menu], scale: Scale) -> Vec<(i32, i32)> {
    let mut x = 8 * scale;
    menus
        .iter()
        .map(|m| {
            let w = text_width(m.title, scale) + 16 * scale;
            let at = (x, w);
            x += w;
            at
        })
        .collect()
}

/// The open menu's panel: left, top, width, and each item's top.
pub(super) fn panel(menus: &[Menu], open: Open, scale: Scale) -> (i32, i32, i32, Vec<i32>) {
    let (x, _) = titles(menus, scale)[open.menu];
    let menu = &menus[open.menu];
    let mut label_w = 0;
    let mut shortcut_w = 0;
    for item in &menu.items {
        if let Item::Command { label, shortcut, .. } = item {
            label_w = label_w.max(text_width(label, scale));
            shortcut_w = shortcut_w.max(text_width(shortcut, scale));
        }
    }
    let width = 18 * scale + label_w + 24 * scale + shortcut_w + 8 * scale;
    let mut y = bar_height(scale) + 2 * scale;
    let tops = menu
        .items
        .iter()
        .map(|item| {
            let top = y;
            y += if matches!(item, Item::Separator) { separator_height(scale) } else { item_height(scale) };
            top
        })
        .collect();
    (x, bar_height(scale), width, tops)
}

/// What the mouse is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Title(usize),
    /// An item of the open menu.
    Item(usize),
    /// Inside the open panel, but on no item (a separator, the border).
    Panel,
    Bar,
}

pub fn hit(menus: &[Menu], open: Option<Open>, scale: Scale, x: i32, y: i32) -> Option<Hit> {
    if (0..bar_height(scale)).contains(&y) {
        let title = titles(menus, scale).iter().position(|&(tx, w)| (tx..tx + w).contains(&x));
        return Some(title.map_or(Hit::Bar, Hit::Title));
    }
    let open = open?;
    let (px, py, width, tops) = panel(menus, open, scale);
    let items = &menus[open.menu].items;
    let bottom = tops.last().map_or(py, |&t| t + item_height(scale)) + 2 * scale;
    if !(px..px + width).contains(&x) || !(py..bottom).contains(&y) {
        return None;
    }
    let found = tops.iter().zip(items).position(|(&top, item)| {
        let h = if matches!(item, Item::Separator) { separator_height(scale) } else { item_height(scale) };
        (top..top + h).contains(&y)
    });
    Some(found.filter(|&i| !matches!(items[i], Item::Separator)).map_or(Hit::Panel, Hit::Item))
}

// ─── The library ─────────────────────────────────────────────────────────────

/// Where the library's edits stand, shown after its name at the right of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryState {
    /// The file holds every edit.
    Saved,
    /// The library will be saved at the first edit: its file does not exist yet.
    New,
    /// Edits wait to be saved (a moment after the last one).
    Modified,
    /// The last save failed; the next edit, or File > Save, tries again.
    Failed,
    /// No file: edits are lost on quit (the footer says why).
    NotSaved,
    /// A plugin's: the host saves it with its project.
    Host,
}

impl LibraryState {
    /// Every state, the longest label setting the column's width.
    const ALL: [Self; 6] = [Self::Saved, Self::New, Self::Modified, Self::Failed, Self::NotSaved, Self::Host];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Saved => "saved",
            Self::New => "new",
            Self::Modified => "modified",
            Self::Failed => "save failed",
            Self::NotSaved => "not saved",
            Self::Host => "in host project",
        }
    }

    fn color(self) -> Rgb {
        match self {
            Self::Saved => theme::LIVE,
            Self::Modified => theme::VALUE,
            Self::Failed | Self::NotSaved => theme::ALERT,
            Self::New | Self::Host => dim(),
        }
    }
}

/// Characters of the library's place in the bar, its state included.
pub(super) const LIBRARY_CHARS: i32 = 44;

/// The library's place at the right of the bar: left edge and width, the
/// same whatever its name and state; empty when the titles leave no room.
pub(super) fn library_field(menus: &[Menu], width: i32, scale: Scale) -> (i32, i32) {
    let titles_end = titles(menus, scale).last().map_or(0, |&(x, w)| x + w);
    let right = width - 8 * scale;
    let field = (LIBRARY_CHARS * char_width(scale)).min(right - titles_end - 16 * scale).max(0);
    (right - field, field)
}

/// The library's name and state at the right of the bar: the state in a
/// column of its own, the name fitted to what is left.
pub fn draw_library(canvas: &mut Canvas, menus: &[Menu], name: &str, state: LibraryState, scale: Scale) {
    let (x, field) = library_field(menus, canvas.width(), scale);
    let longest = LibraryState::ALL.iter().map(|s| text_width(s.label(), scale)).max().unwrap_or(0);
    let dot = 4 * scale;
    let state_w = dot + 4 * scale + longest;
    let name_w = field - state_w - 12 * scale;
    if name_w <= 0 {
        return;
    }
    let text_y = (bar_height(scale) - 8 * scale) / 2;
    canvas.text(x, text_y, &fit(name, name_w, scale), scale, theme::text());
    let state_x = x + field - state_w;
    canvas.fill_rect(state_x, text_y + 2 * scale, dot, dot, state.color());
    canvas.text(state_x + dot + 4 * scale, text_y, state.label(), scale, state.color());
}

// ─── Drawing ─────────────────────────────────────────────────────────────────

/// The bar, and the open menu over everything.
pub fn draw(canvas: &mut Canvas, menus: &[Menu], open: Option<Open>, scale: Scale) {
    let bar = bar_height(scale);
    canvas.fill_rect(0, 0, canvas.width(), bar, bar_background_color());
    canvas.fill_rect(0, bar - scale.px(), canvas.width(), scale.px(), border_color());
    let text_y = (bar - 8 * scale) / 2;
    for (i, (menu, (x, w))) in menus.iter().zip(titles(menus, scale)).enumerate() {
        let is_open = open.is_some_and(|o| o.menu == i);
        if is_open {
            canvas.fill_rect(x, 0, w, bar - scale.px(), highlight_color());
        }
        canvas.text(x + 8 * scale, text_y, menu.title, scale, if is_open { ACCENT } else { theme::text() });
    }
    let Some(open) = open else { return };
    let (px, py, width, tops) = panel(menus, open, scale);
    let items = &menus[open.menu].items;
    let bottom = tops.last().map_or(py, |&t| t + item_height(scale)) + 2 * scale;
    canvas.fill_rect(px, py, width, bottom - py, border_color());
    canvas.fill_rect(px + scale.px(), py, width - 2 * scale, bottom - py - scale.px(), panel_color());
    for (i, (item, &top)) in items.iter().zip(&tops).enumerate() {
        match item {
            Item::Separator => {
                canvas.fill_rect(
                    px + 6 * scale,
                    top + separator_height(scale) / 2,
                    width - 12 * scale,
                    scale.px(),
                    border_color(),
                );
            }
            Item::Command { label, shortcut, enabled, checked, .. } => {
                let h = item_height(scale);
                if open.item == Some(i) && *enabled {
                    canvas.fill_rect(px + scale.px(), top, width - 2 * scale, h, highlight_color());
                }
                let ty = top + (h - 8 * scale) / 2;
                if let Some(on) = checked {
                    let (bx, by, b) = (px + 5 * scale, ty + scale.px(), 6 * scale);
                    if *on {
                        canvas.fill_rect(bx, by, b, b, if *enabled { ACCENT } else { disabled_color() });
                    } else {
                        let line = scale.px();
                        let c = if *enabled { dim() } else { disabled_color() };
                        canvas.fill_rect(bx, by, b, line, c);
                        canvas.fill_rect(bx, by + b - line, b, line, c);
                        canvas.fill_rect(bx, by, line, b, c);
                        canvas.fill_rect(bx + b - line, by, line, b, c);
                    }
                }
                let color = if !enabled {
                    disabled_color()
                } else if open.item == Some(i) {
                    ACCENT
                } else {
                    theme::text()
                };
                canvas.text(px + 18 * scale, ty, label, scale, color);
                let sw = text_width(shortcut, scale);
                canvas.text(
                    px + width - 8 * scale - sw,
                    ty,
                    shortcut,
                    scale,
                    if *enabled { dim() } else { disabled_color() },
                );
            }
        }
    }
}
