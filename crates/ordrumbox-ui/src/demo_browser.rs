//! File > Demos: the demo styles as coloured tiles, then the songs and
//! patterns of the style opened, each with its description. The browser
//! only chooses: `session_demo_browser.rs` loads what is chosen.
//!
//! The panel keeps one size and one place whatever it shows: the tiles and
//! the list share the same area, the description its fixed lines.

use ordrumbox_logic::demo_catalog::{DemoItem, DemoStyle};

use crate::canvas::{Canvas, fit};
use crate::panel::Rect;
use crate::scale::Scale;
use crate::session::OverlayKey;
use crate::song_view::wrap;
use crate::theme::{self, ACCENT, FOCUS};

/// Tiles in a row of the grid.
const COLUMNS: usize = 4;
/// Rows of tiles the area shows (more scroll).
const TILE_ROWS: usize = 3;
/// Lines of a description under the area.
const DESCRIPTION_LINES: usize = 3;
/// The widest and tallest the panel gets, in scale units.
const MAX_SIZE: (i32, i32) = (720, 380);

/// What a key or a click asks of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Browsing,
    Close,
    /// Load `item` of style `style` into the library.
    Load {
        style: usize,
        item: DemoItem,
    },
}

/// Where the browser is: on the tiles (`style` is `None`), or in the list
/// of a style; the tile and the row chosen, and the first of each shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DemoBrowser {
    style: Option<usize>,
    tile: usize,
    row: usize,
    first_tile_row: usize,
    first_row: usize,
}

impl DemoBrowser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open style `index`, on its first item.
    fn open(&mut self, index: usize) {
        self.style = Some(index);
        self.tile = index;
        self.row = 0;
        self.first_row = 0;
    }

    /// Back to the tiles, on the style that was open.
    fn back(&mut self) {
        self.style = None;
    }

    /// A key: arrows move, Enter opens or loads, Backspace goes back, Esc closes.
    pub fn key(&mut self, key: &OverlayKey, styles: &[DemoStyle]) -> Outcome {
        match (self.style, key) {
            (_, OverlayKey::Escape) => return Outcome::Close,
            (None, OverlayKey::Enter) if self.tile < styles.len() => self.open(self.tile),
            (None, OverlayKey::Side(delta)) => self.tile = moved(self.tile, *delta, styles.len()),
            (None, OverlayKey::Move(delta)) => {
                let delta = delta.signum() * COLUMNS as isize;
                let to = self.tile.saturating_add_signed(delta);
                if to < styles.len() {
                    self.tile = to;
                }
            }
            (None, OverlayKey::Edge(end)) => self.tile = if *end { styles.len().saturating_sub(1) } else { 0 },
            (Some(s), OverlayKey::Enter) => {
                let item = styles.get(s).and_then(|style| style.items().get(self.row).copied());
                if let Some(item) = item {
                    return Outcome::Load { style: s, item };
                }
            }
            (Some(_), OverlayKey::Back | OverlayKey::Side(-1)) => self.back(),
            (Some(s), OverlayKey::Move(delta)) => {
                self.row = moved(self.row, *delta, item_count(styles, s));
            }
            (Some(s), OverlayKey::Edge(end)) => {
                self.row = if *end { item_count(styles, s).saturating_sub(1) } else { 0 };
            }
            _ => {}
        }
        Outcome::Browsing
    }

    /// A click at `(x, y)`: outside the panel closes, on a tile opens it, on
    /// the back line goes back, on an item loads it.
    pub fn click(&mut self, layout: &DemoLayout, styles: &[DemoStyle], x: i32, y: i32) -> Outcome {
        if !layout.panel.contains(x, y) {
            return Outcome::Close;
        }
        match self.style {
            None => {
                if let Some(tile) = layout.tile_at(self.first_tile_row, x, y).filter(|&t| t < styles.len()) {
                    self.open(tile);
                }
            }
            Some(s) => {
                if layout.back_line().contains(x, y) {
                    self.back();
                } else if let Some(row) = layout.row_at(self.first_row, x, y) {
                    let item = styles.get(s).and_then(|style| style.items().get(row).copied());
                    if let Some(item) = item {
                        self.row = row;
                        return Outcome::Load { style: s, item };
                    }
                }
            }
        }
        Outcome::Browsing
    }

    /// The first tile row and list row shown, so that the chosen ones are:
    /// after a key or a click, before the panel is drawn.
    pub fn follow(&mut self, layout: &DemoLayout) {
        let tile_row = self.tile / COLUMNS;
        self.first_tile_row = scrolled_to(self.first_tile_row, tile_row, TILE_ROWS);
        self.first_row = scrolled_to(self.first_row, self.row, layout.list_rows);
    }
}

/// `index` moved by `delta`, kept among `count`.
fn moved(index: usize, delta: isize, count: usize) -> usize {
    index.saturating_add_signed(delta).min(count.saturating_sub(1))
}

/// The first of `shown` rows from `first`, moved as little as can be so
/// that row `at` is among them.
fn scrolled_to(first: usize, at: usize, shown: usize) -> usize {
    first.min(at).max((at + 1).saturating_sub(shown.max(1)))
}

fn item_count(styles: &[DemoStyle], s: usize) -> usize {
    styles.get(s).map_or(0, |style| style.items().len())
}

/// Where the panel's parts are in a window.
#[derive(Debug, Clone, Copy)]
pub struct DemoLayout {
    panel: Rect,
    /// The tiles or the list.
    area: Rect,
    /// The description, under the area.
    description: Rect,
    list_rows: usize,
    line: i32,
    scale: Scale,
}

impl DemoLayout {
    #[must_use]
    pub fn new(width: i32, height: i32, scale: Scale) -> Self {
        let margin = 24 * scale;
        let w = (width - 2 * margin).clamp(120 * scale, MAX_SIZE.0 * scale);
        let h = (height - 2 * margin).clamp(120 * scale, MAX_SIZE.1 * scale);
        let panel = Rect::new((width - w) / 2, (height - h) / 2, w, h);
        let line = 12 * scale;
        let pad = 12 * scale;
        // Title, then the back line (its place kept on the tiles), then the area.
        let area_top = panel.y + pad + 2 * line;
        let bottom = panel.y + h - pad - line - 4 * scale;
        let description_h = DESCRIPTION_LINES as i32 * line;
        let area_h = (bottom - description_h - 6 * scale - area_top).max(line);
        let area = Rect::new(panel.x + pad, area_top, w - 2 * pad, area_h);
        let description = Rect::new(area.x, area.y + area.h + 6 * scale, area.w, description_h);
        let list_rows = usize::try_from(area_h / line).unwrap_or(1).max(1);
        Self { panel, area, description, list_rows, line, scale }
    }

    /// The rectangle of the tile at `slot` of the grid shown (from 0, row by row).
    fn tile_rect(&self, slot: usize) -> Rect {
        let gap = 6 * self.scale;
        let w = (self.area.w - gap * (COLUMNS as i32 - 1)) / COLUMNS as i32;
        let h = (self.area.h - gap * (TILE_ROWS as i32 - 1)) / TILE_ROWS as i32;
        let (column, row) = ((slot % COLUMNS) as i32, (slot / COLUMNS) as i32);
        Rect::new(self.area.x + column * (w + gap), self.area.y + row * (h + gap), w, h)
    }

    /// The tile under `(x, y)`, the grid scrolled to tile row `first_row`.
    fn tile_at(&self, first_row: usize, x: i32, y: i32) -> Option<usize> {
        (0..COLUMNS * TILE_ROWS)
            .find(|&slot| self.tile_rect(slot).contains(x, y))
            .map(|slot| first_row * COLUMNS + slot)
    }

    /// The line above the list that goes back to the tiles.
    fn back_line(&self) -> Rect {
        Rect::new(self.area.x, self.area.y - self.line - 2 * self.scale, self.area.w, self.line)
    }

    /// The item under `(x, y)`, the list scrolled to `first`.
    fn row_at(&self, first: usize, x: i32, y: i32) -> Option<usize> {
        if !self.area.contains(x, y) {
            return None;
        }
        let row = usize::try_from((y - self.area.y) / self.line).ok().filter(|&r| r < self.list_rows)?;
        Some(first + row)
    }
}

/// The panel drawn: the tiles or the list of the style open.
pub fn draw(canvas: &mut Canvas, browser: &DemoBrowser, styles: &[DemoStyle], layout: &DemoLayout) {
    let s = layout.scale;
    let p = layout.panel;
    canvas.fill_rect(p.x - 4 * s, p.y - 4 * s, p.w + 8 * s, p.h + 8 * s, theme::shade());
    canvas.fill_rect(p.x, p.y, p.w, p.h, theme::line());
    canvas.fill_rect(p.x + s.px(), p.y + s.px(), p.w - 2 * s, p.h - 2 * s, theme::panel());
    let x = layout.area.x;
    let title = browser.style.and_then(|i| styles.get(i)).map_or("Demos", DemoStyle::name);
    canvas.text(x, p.y + 10 * s, &fit(title, layout.area.w, s.title()), s.title(), ACCENT);
    let description = match browser.style.and_then(|i| styles.get(i)) {
        None => {
            draw_tiles(canvas, browser, styles, layout);
            styles.get(browser.tile).map_or("", DemoStyle::description)
        }
        Some(style) => {
            canvas.text(x, layout.back_line().y, "< all styles", s, theme::muted());
            draw_items(canvas, browser, style, layout);
            style.items().get(browser.row).map_or("", |&item| style.item_description(item))
        }
    };
    draw_description(canvas, description, layout);
    let hint = if browser.style.is_some() {
        "Arrows: choose   ENTER or click: load into your library   BACKSPACE: styles   ESC: close"
    } else {
        "Arrows: choose   ENTER or click: open the style   ESC: close"
    };
    canvas.text(x, p.y + p.h - 12 * s - layout.line / 2, &fit(hint, layout.area.w, s), s, theme::muted());
}

fn draw_tiles(canvas: &mut Canvas, browser: &DemoBrowser, styles: &[DemoStyle], layout: &DemoLayout) {
    let s = layout.scale;
    let first = browser.first_tile_row * COLUMNS;
    for (slot, (index, style)) in styles.iter().enumerate().skip(first).take(COLUMNS * TILE_ROWS).enumerate() {
        let r = layout.tile_rect(slot);
        let color = theme::track(index);
        let chosen = index == browser.tile;
        if chosen {
            canvas.fill_rect(r.x - 2 * s, r.y - 2 * s, r.w + 4 * s, r.h + 4 * s, FOCUS);
        }
        let fill = theme::mix(theme::panel(), color, if chosen { 0.55 } else { 0.3 });
        canvas.fill_rounded_gradient(r.x, r.y, r.w, r.h, (fill, theme::mix(fill, theme::panel(), 0.4)), 4 * s);
        canvas.fill_rect(r.x, r.y, 4 * s, r.h, color);
        let room = r.w - 14 * s;
        canvas.text(r.x + 10 * s, r.y + 6 * s, &fit(style.name(), room, s), s, theme::text());
        let library = style.library();
        let counts = match library.songs.len() {
            0 => format!("{} patterns", library.patterns.len()),
            n => format!("{n} song{}, {} patterns", if n == 1 { "" } else { "s" }, library.patterns.len()),
        };
        if r.h >= 2 * layout.line + 4 * s {
            canvas.text(r.x + 10 * s, r.y + 6 * s + layout.line, &fit(&counts, room, s), s, theme::muted());
        }
    }
}

/// An item's line: what it is, its name, and its length.
fn item_line(style: &DemoStyle, item: DemoItem, chars: usize) -> String {
    let library = style.library();
    let (kind, detail) = match item {
        DemoItem::Song(i) => {
            let measures = library.songs.get(i).map_or(0.0, |song| song.length_measures());
            ("song", format!("{measures} measures"))
        }
        DemoItem::Pattern(i) => {
            let beats = library.patterns.get(i).map_or(0, |pattern| pattern.beat_count);
            ("", format!("{beats} beats"))
        }
    };
    let detail = format!("{detail:>12}");
    let name_chars = chars.saturating_sub(6 + detail.chars().count() + 1);
    let name: String = style.item_name(item).chars().take(name_chars).collect();
    format!("{kind:<6}{name:<name_chars$} {detail}")
}

fn draw_items(canvas: &mut Canvas, browser: &DemoBrowser, style: &DemoStyle, layout: &DemoLayout) {
    let s = layout.scale;
    let chars = crate::canvas::chars_in(layout.area.w - 8 * s, s);
    for (row, (index, item)) in
        style.items().into_iter().enumerate().skip(browser.first_row).take(layout.list_rows).enumerate()
    {
        let y = layout.area.y + row as i32 * layout.line;
        let chosen = index == browser.row;
        if chosen {
            canvas.fill_rect(layout.area.x - 4 * s, y - 2 * s, layout.area.w + 8 * s, layout.line, theme::highlight());
        }
        let color = if chosen { ACCENT } else { theme::text() };
        canvas.text(layout.area.x, y, &item_line(style, item, chars), s, color);
    }
}

fn draw_description(canvas: &mut Canvas, text: &str, layout: &DemoLayout) {
    let s = layout.scale;
    let chars = crate::canvas::chars_in(layout.description.w, s);
    let mut lines = wrap(text, chars.max(1));
    if lines.len() > DESCRIPTION_LINES {
        lines.truncate(DESCRIPTION_LINES);
        if let Some(last) = lines.last_mut() {
            let kept: String = last.chars().take(chars.saturating_sub(3)).collect();
            *last = format!("{kept}...");
        }
    }
    for (i, line) in lines.iter().enumerate() {
        canvas.text(layout.description.x, layout.description.y + i as i32 * layout.line, line, s, theme::muted());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;

    fn style(name: &str, patterns: usize, songs: bool) -> DemoStyle {
        let pattern_list: Vec<String> =
            (0..patterns).map(|i| format!(r#"{{"id":"{name}{i}","name":"P{i}"}}"#)).collect();
        let song = if songs {
            format!(
                r#","songs":[{{"id":"{name}s","name":"Song","clips":[{{"patternId":"{name}0","startMeasure":0,"measureCount":4}}]}}]"#
            )
        } else {
            String::new()
        };
        let text = format!(
            r#"{{"info":{{"name":"{name}","description":"About {name}."}},"patterns":[{}]{song}}}"#,
            pattern_list.join(",")
        );
        DemoStyle::new(Library::parse(&text).unwrap().0, name)
    }

    fn styles() -> Vec<DemoStyle> {
        (0..14).map(|i| style(&format!("s{i}"), 3, i == 1)).collect()
    }

    #[test]
    fn the_keys_open_a_style_and_load_an_item() {
        let styles = styles();
        let mut b = DemoBrowser::new();
        assert_eq!(b.key(&OverlayKey::Side(1), &styles), Outcome::Browsing);
        assert_eq!(b.key(&OverlayKey::Enter, &styles), Outcome::Browsing);
        assert_eq!(b.style, Some(1));
        assert_eq!(b.key(&OverlayKey::Enter, &styles), Outcome::Load { style: 1, item: DemoItem::Song(0) });
        b.key(&OverlayKey::Move(1), &styles);
        assert_eq!(b.key(&OverlayKey::Enter, &styles), Outcome::Load { style: 1, item: DemoItem::Pattern(0) });
        b.key(&OverlayKey::Edge(true), &styles);
        assert_eq!(b.key(&OverlayKey::Enter, &styles), Outcome::Load { style: 1, item: DemoItem::Pattern(2) });
        b.key(&OverlayKey::Back, &styles);
        assert_eq!((b.style, b.tile), (None, 1), "back on the style that was open");
        assert_eq!(b.key(&OverlayKey::Escape, &styles), Outcome::Close);
    }

    #[test]
    fn up_and_down_move_a_row_of_tiles_and_stay_in_the_grid() {
        let styles = styles();
        let mut b = DemoBrowser::new();
        b.key(&OverlayKey::Move(1), &styles);
        assert_eq!(b.tile, COLUMNS);
        b.key(&OverlayKey::Move(-1), &styles);
        b.key(&OverlayKey::Move(-1), &styles);
        assert_eq!(b.tile, 0);
        b.key(&OverlayKey::Edge(true), &styles);
        b.key(&OverlayKey::Move(1), &styles);
        assert_eq!(b.tile, 13, "no row under the last");
    }

    #[test]
    fn a_click_opens_a_tile_goes_back_and_loads_a_row() {
        let styles = styles();
        let layout = DemoLayout::new(1000, 700, Scale::ONE);
        let mut b = DemoBrowser::new();
        let tile = layout.tile_rect(5);
        assert_eq!(b.click(&layout, &styles, tile.x + 2, tile.y + 2), Outcome::Browsing);
        assert_eq!(b.style, Some(5));
        let row = (layout.area.x + 2, layout.area.y + layout.line + 2);
        assert_eq!(b.click(&layout, &styles, row.0, row.1), Outcome::Load { style: 5, item: DemoItem::Pattern(1) });
        let back = layout.back_line();
        b.click(&layout, &styles, back.x + 2, back.y + 2);
        assert_eq!(b.style, None);
        assert_eq!(b.click(&layout, &styles, 1, 1), Outcome::Close);
    }

    #[test]
    fn the_panel_keeps_its_place_and_scrolls_to_the_choice() {
        let styles = styles();
        let layout = DemoLayout::new(1000, 700, Scale::ONE);
        let mut pixels = vec![0; 1000 * 700];
        let mut b = DemoBrowser::new();
        b.key(&OverlayKey::Edge(true), &styles);
        b.follow(&layout);
        draw(&mut Canvas::new(&mut pixels, 1000, 700), &b, &styles, &layout);
        assert_eq!(b.first_tile_row, 1, "the last tile shown");
        b.key(&OverlayKey::Enter, &styles);
        b.follow(&layout);
        draw(&mut Canvas::new(&mut pixels, 1000, 700), &b, &styles, &layout);
        assert_eq!(DemoLayout::new(1000, 700, Scale::ONE).panel, layout.panel, "one place for both");
        assert!(pixels.iter().any(|&p| p != pixels[0]));
    }

    #[test]
    fn an_item_line_keeps_its_width() {
        let style = style("x", 2, true);
        let chars = 60;
        for item in style.items() {
            assert_eq!(item_line(&style, item, chars).chars().count(), chars, "{item:?}");
        }
    }

    #[test]
    fn scrolling_moves_as_little_as_it_can() {
        assert_eq!(scrolled_to(0, 2, 3), 0);
        assert_eq!(scrolled_to(0, 5, 3), 3);
        assert_eq!(scrolled_to(4, 1, 3), 1);
    }
}
