//! The mouse, the wheel and the keys on the editors' widgets: what a click,
//! a drag, a hover, a key or a typed number on them does, and the tooltip.

use crate::scale::Scale;
use ordrumbox_core::json::Value;
use ordrumbox_core::num;
use std::borrow::Cow;

use super::session_panel::{DOUBLE_CLICK, control_name};
use super::*;
use crate::control::{Spec, Style};
use crate::control_help;
use crate::editors;
use crate::panel::{self, Item, Kind};
use crate::view::Layout;

impl Session {
    // ─── The mouse ───────────────────────────────────────────────────────────

    /// A press at `(x, y)`; whether the widgets took it.
    pub fn mouse_down(&mut self, size: (i32, i32, Scale), x: i32, y: i32, button: Button, shift: bool) -> bool {
        self.chrome.panel_size = size;
        self.chrome.widgets.pointer = (x, y);
        let (width, height, scale) = size;
        if let Some(dropdown) = self.chrome.widgets.dropdown.take() {
            let left = button == Button::Left;
            if let Some(i) = dropdown.hit(width, height, scale, x, y) {
                self.chrome.widgets.dropdown = Some(dropdown);
                if left {
                    self.pick(i);
                }
            } else if let Some((menu, i)) = dropdown.parent_at(width, height, scale, x, y) {
                // Back to a menu above: its submenus close, a line of it is picked.
                self.chrome.widgets.dropdown = Some(menu);
                if let (true, Some(i)) = (left, i) {
                    self.pick(i);
                }
            }
            return true;
        }
        if let Some(entry) = &self.chrome.widgets.entry {
            if !entry.rect(width, height, scale).contains(x, y) {
                self.chrome.widgets.entry = None;
            }
            return true;
        }
        let layout = self.panel_layout(width, height, scale);
        let Some(item) = panel::hit(&layout.items, x, y).cloned() else {
            self.chrome.widgets.focus = None;
            // A click on the track editor's background is its own.
            return layout.right > 0 && x >= width - layout.right;
        };
        let Some(id) = item.id else { return false };
        if button == Button::Right {
            // A knob or a slider: its menu (type a value, its default, copy,
            // paste). The other widgets have no menu of their own.
            if matches!(item.kind, Kind::Value { .. }) {
                self.control_menu(id, x, y);
            }
            return true;
        }
        let now = Instant::now();
        let double = self.chrome.widgets.last_click.is_some_and(|(last, at)| last == id && now - at < DOUBLE_CLICK);
        self.chrome.widgets.last_click = Some((id, now));
        match item.kind {
            Kind::Value { spec, value, label, text, .. } => {
                self.chrome.widgets.focus = Some(id);
                if spec.style == Style::Stepper {
                    let [down, field, up] = panel::stepper_parts(item.rect, label.is_some(), scale);
                    if x < down.x - 2 * scale {
                        self.select_row(id);
                        return true;
                    }
                    // A button is one step a click, however fast the clicks.
                    if let Some(up) = [(down, false), (up, true)].iter().find(|(r, _)| r.contains(x, y)).map(|p| p.1) {
                        self.chrome.widgets.last_click = None;
                        self.set_value(id, spec.nudge(value, up, false, false));
                        return true;
                    }
                    if !field.contains(x, y) {
                        return true;
                    }
                }
                if double {
                    // Double-click: the default value.
                    self.chrome.widgets.last_click = None;
                    self.set_value(id, spec.default);
                    return true;
                }
                let mut drag = Drag { id, spec, start: value, x, y, bar: None, fine: false };
                if spec.style == Style::Slider {
                    let (left, w) = panel::slider_bar(item.rect, label.is_some(), !text.is_empty(), scale);
                    if x < left - 2 * scale {
                        // The label: selects the row.
                        self.select_row(id);
                        return true;
                    }
                    if shift {
                        drag.fine = true;
                    } else {
                        drag.bar = Some((left, w));
                        let v = spec.slider_at(f64::from(x - left) / f64::from(w));
                        drag.start = v;
                        self.set_value(id, v);
                    }
                }
                self.chrome.widgets.drag = Some(drag);
            }
            Kind::Select { values, labels, selected } => {
                self.chrome.widgets.focus = Some(id);
                self.open_select(id, item.rect, values, labels, selected);
            }
            _ => self.activate(id),
        }
        true
    }

    /// The mouse moved; whether to redraw.
    pub fn mouse_move(&mut self, size: (i32, i32, Scale), x: i32, y: i32, shift: bool) -> bool {
        self.chrome.widgets.pointer = (x, y);
        if self.roll.dragging() {
            return self.roll_drag_move(x, y);
        }
        if self.roll.zone_drag.is_some() {
            return self.roll_zone_move(size, x, y);
        }
        if self.song_panel.dragging() {
            return self.song_drag_move(x, y);
        }
        if self.chrome.track_drag.is_some() {
            return self.track_drag_move(x, y);
        }
        if self.chrome.zone_drag.is_some() {
            return self.zone_drag_move(size, x, y);
        }
        if self.chrome.lane_drag.is_some() {
            return self.lane_drag_move(size, x, y);
        }
        if let Some(axis) = self.song_panel.held_bar() {
            return self.song_bar_to(size, axis, x, y);
        }
        let (width, height, scale) = size;
        if let Some(dropdown) = &mut self.chrome.widgets.dropdown {
            if let Some(i) = dropdown.hit(width, height, scale, x, y).filter(|&i| i != dropdown.at) {
                dropdown.at = i;
                return true;
            }
            return false;
        }
        let Some(drag) = self.chrome.widgets.drag else { return false };
        let value = match (drag.spec.style, drag.bar) {
            (Style::Knob, _) => drag.spec.drag(drag.start, f64::from(drag.y - y), shift),
            (Style::Stepper, _) => drag.spec.drag(drag.start, f64::from(drag.y - y), false),
            (Style::Slider, Some((left, w))) => drag.spec.slider_at(f64::from(x - left) / f64::from(w.max(1))),
            (Style::Slider, None) => {
                let w = self.slider_width(size, drag.id).max(1);
                drag.spec.slider_fine(drag.start, f64::from(x - drag.x) / f64::from(w))
            }
        };
        // `commitValue`: only a value that moved.
        if self.current_value(size, drag.id) != Some(value) {
            self.set_value(drag.id, value);
            return true;
        }
        false
    }

    /// The mouse over the grid: the step or the track name it is on, for its tooltip; whether it changed.
    pub fn grid_hover(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let body = self.body(size);
        let scale = size.2;
        let hover = match self.pattern() {
            Some(pattern)
                if self.kit_editor.is_none()
                    && self.workspace == Workspace::Grid
                    && self.chrome.widgets.drag.is_none()
                    && self.chrome.track_drag.is_none()
                    && self.chrome.zone_drag.is_none() =>
            {
                match Layout::new(
                    body.main_w,
                    body.editor_h,
                    scale,
                    pattern,
                    self.status.len(),
                    self.chrome.current_page,
                )
                .hit(pattern, x, y)
                {
                    hit @ Some(view::Hit::Cell { .. } | view::Hit::Name(_)) => hit,
                    _ => None,
                }
            }
            _ => None,
        };
        let changed = hover != self.chrome.hover;
        self.chrome.hover = hover;
        changed
    }

    /// The mouse over the panels: the control under it that has a tooltip
    /// (none while a control is dragged); whether it changed.
    pub fn control_hover(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let (width, height, scale) = size;
        let hover = if self.chrome.widgets.drag.is_some() || self.chrome.covered() {
            None
        } else {
            let layout = self.panel_layout(width, height, scale);
            panel::hit(&layout.items, x, y)
                .and_then(|item| Some((item.id?, item.rect)))
                .filter(|(id, _)| self.control_tip(*id).is_some())
        };
        let changed = hover != self.chrome.control_hover;
        self.chrome.control_hover = hover;
        changed
    }

    /// What a control is, said in its tooltip; a plugin's gives its own
    /// shortcuts (Alt for Ctrl).
    pub(super) fn control_tip(&self, id: Id) -> Option<[Cow<'static, str>; 2]> {
        let lines = control_help::help(id)?;
        Some(lines.map(|line| if self.hosted { menu::hosted_shortcuts(line) } else { line.into() }))
    }

    /// The tooltip of the note under the mouse (the first of its step), as
    /// `PointerSection.onMouseOver` shows it; over a control, what it is.
    pub(super) fn draw_tooltip(&self, canvas: &mut Canvas, scale: Scale) {
        if let Some((id, rect)) = self.chrome.control_hover.filter(|_| !self.chrome.covered())
            && let Some(lines) = self.control_tip(id)
        {
            return view::draw_tooltip_lines(canvas, &lines, rect, scale);
        }
        if self.song_view_open() {
            return self.draw_song_tooltip(canvas, scale);
        }
        if self.piano_roll_open() {
            return self.draw_roll_tooltip(canvas, scale);
        }
        let Some(hover) = self.chrome.hover else { return };
        if self.chrome.covered() || self.kit_editor.is_some() {
            return;
        }
        let Some(pattern) = self.pattern() else { return };
        let body = self.body((canvas.width(), canvas.height(), scale));
        let layout =
            Layout::new(body.main_w, body.editor_h, scale, pattern, self.status.len(), self.chrome.current_page);
        match hover {
            view::Hit::Cell { track: t, beat, step } => {
                let Some(track) = pattern.tracks.get(t) else { return };
                let Some(note) = track.notes.iter().find(|n| n.beat == beat && n.step == step) else { return };
                let cell = layout.cell_rect(t, beat, step, track);
                view::draw_tooltip(canvas, &editors::note_tooltip(note, track.transpose), cell, scale);
            }
            view::Hit::Name(t) => {
                let Some(track) = pattern.tracks.get(t) else { return };
                let lines = editors::track_tooltip(track, &self.track_sound_label(t));
                view::draw_tooltip_lines(canvas, &lines, layout.name_rect(t), scale);
            }
            view::Hit::Mute(_) | view::Hit::Solo(_) | view::Hit::AddTrack => {}
        }
    }

    pub fn mouse_up(&mut self) {
        self.chrome.widgets.drag = None;
        self.song_panel.release_bar();
        self.chrome.track_drag = None;
        self.chrome.zone_drag = None;
        self.chrome.lane_drag = None;
        self.roll_release();
    }

    /// The right-click menu of a knob or a slider: type a value, its
    /// default, copy it, paste one copied.
    fn control_menu(&mut self, id: Id, x: i32, y: i32) {
        let (values, labels) = (
            vec!["type".into(), "default".into(), "copy".into(), "paste".into()],
            vec![
                "Type a value...".to_owned(),
                "Default value".to_owned(),
                "Copy value".to_owned(),
                "Paste value".to_owned(),
            ],
        );
        let (label, _) = control_name(id);
        let mut menu = Dropdown::menu(Id::ControlMenu, x, y, label.to_owned(), values, labels);
        menu.control = Some(id);
        self.chrome.widgets.dropdown = Some(menu);
    }

    /// A line of a control's right-click menu picked (`control`: whose).
    pub(super) fn control_menu_pick(&mut self, value: &str, control: Option<Id>) {
        let Some(id) = control else { return };
        let size = self.chrome.panel_size;
        let layout = self.panel_layout(size.0, size.1, size.2);
        let Some(item) = panel::find(&layout.items, id) else { return };
        let Kind::Value { spec, value: current, .. } = item.kind else { return };
        match value {
            "type" => {
                self.chrome.widgets.focus = Some(id);
                self.open_entry(id, spec, current);
            }
            "default" => self.set_value(id, spec.default),
            "copy" => {
                self.chrome.widgets.copied_value = Some(current);
                self.message = format!("value {} copied", num::text(spec.shown(current)));
            }
            "paste" => match self.chrome.widgets.copied_value {
                Some(copied) if (spec.min..=spec.max).contains(&copied) => self.set_value(id, copied),
                Some(copied) => {
                    self.message = format!(
                        "{} is out of this value's {}–{}",
                        num::text(copied),
                        num::text(spec.min),
                        num::text(spec.max)
                    )
                }
                None => self.message = "no value copied".into(),
            },
            _ => {}
        }
    }

    /// The wheel over the widgets (one notch: one step); whether it was
    /// theirs. Over a control it is only theirs: the window never scrolls
    /// under a knob a wheel turn would turn, a fine delta included.
    pub fn wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, lines: isize, shift: bool, ctrl: bool) -> bool {
        if let Some(dropdown) = &mut self.chrome.widgets.dropdown {
            dropdown.scroll(-lines);
            return true;
        }
        if self.chrome.widgets.entry.is_some() {
            return false;
        }
        let (width, height, scale) = size;
        let layout = self.panel_layout(width, height, scale);
        let Some(item) = panel::hit(&layout.items, x, y).cloned() else {
            return false;
        };
        let Some(id) = item.id else { return false };
        match item.kind {
            Kind::Value { spec, value, .. } => {
                let mut v = value;
                for _ in 0..lines.unsigned_abs() {
                    v = spec.nudge(v, lines > 0, shift, ctrl);
                }
                if v != value {
                    self.set_value(id, v);
                }
            }
            Kind::Select { values, selected, .. } => {
                let at = (selected as isize + lines).clamp(0, values.len() as isize - 1) as usize;
                if at != selected
                    && let Some(value) = values.get(at)
                {
                    self.select(id, value);
                }
            }
            _ => {}
        }
        true
    }

    fn current_value(&self, size: (i32, i32, Scale), id: Id) -> Option<f64> {
        let layout = self.panel_layout(size.0, size.1, size.2);
        match panel::find(&layout.items, id)?.kind {
            Kind::Value { value, .. } => Some(value),
            _ => None,
        }
    }

    fn slider_width(&self, size: (i32, i32, Scale), id: Id) -> i32 {
        let layout = self.panel_layout(size.0, size.1, size.2);
        match panel::find(&layout.items, id) {
            Some(Item { rect, kind: Kind::Value { label, text, .. }, .. }) => {
                panel::slider_bar(*rect, label.is_some(), !text.is_empty(), size.2).1
            }
            _ => 1,
        }
    }

    // ─── The keyboard ────────────────────────────────────────────────────────

    /// A key for the widgets (the focused one, the open list or entry);
    /// whether it was theirs.
    pub fn panel_key(&mut self, size: (i32, i32, Scale), key: PanelKey, shift: bool, ctrl: bool) -> bool {
        self.chrome.panel_size = size;
        if let Some(entry) = &mut self.chrome.widgets.entry {
            match key {
                PanelKey::Enter => self.commit_entry(),
                PanelKey::Escape => self.chrome.widgets.entry = None,
                PanelKey::Backspace => {
                    entry.text.pop();
                }
                PanelKey::Space if entry.spec.is_none() => entry.text.push(' '),
                _ => {}
            }
            return true;
        }
        if let Some(dropdown) = &mut self.chrome.widgets.dropdown {
            match key {
                PanelKey::Up => dropdown.step(-1),
                PanelKey::Down => dropdown.step(1),
                PanelKey::PageUp => dropdown.step(-10),
                PanelKey::PageDown => dropdown.step(10),
                PanelKey::Home => dropdown.step(isize::MIN / 2),
                PanelKey::End => dropdown.step(isize::MAX / 2),
                PanelKey::Enter | PanelKey::Space => {
                    let at = dropdown.at;
                    self.pick(at);
                }
                PanelKey::Right if dropdown.opens_submenu() => {
                    let at = dropdown.at;
                    self.pick(at);
                }
                // Back to the menu above (Escape closes the last one).
                PanelKey::Left | PanelKey::Backspace if dropdown.parent.is_some() => {
                    self.chrome.widgets.dropdown = self.chrome.widgets.dropdown.take().and_then(Dropdown::back);
                }
                PanelKey::Escape => {
                    self.chrome.widgets.dropdown = self.chrome.widgets.dropdown.take().and_then(Dropdown::back);
                }
                _ => {}
            }
            return true;
        }
        let layout = self.panel_layout(size.0, size.1, size.2);
        let focusable: Vec<Item> =
            layout.items.into_iter().filter(|i| !matches!(i.id, Some(Id::TrackVolume(_)))).collect();
        if key == PanelKey::Tab {
            let next = panel::next_focus(&focusable, self.chrome.widgets.focus, if shift { -1 } else { 1 });
            if next.is_none() {
                return false;
            }
            self.chrome.widgets.focus = next;
            return true;
        }
        let Some(id) = self.chrome.widgets.focus else { return false };
        let Some(item) = panel::find(&focusable, id).cloned() else {
            self.chrome.widgets.focus = None;
            return false;
        };
        if key == PanelKey::Escape {
            self.chrome.widgets.focus = None;
            return true;
        }
        match item.kind {
            Kind::Value { spec, value, .. } => {
                let up = match key {
                    PanelKey::Up | PanelKey::Right | PanelKey::Plus => true,
                    PanelKey::Down | PanelKey::Left | PanelKey::Minus => false,
                    PanelKey::Enter => {
                        self.open_entry(id, spec, value);
                        return true;
                    }
                    _ => return false,
                };
                let v = spec.nudge(value, up, shift, ctrl);
                if v != value {
                    self.set_value(id, v);
                }
                true
            }
            Kind::Select { values, labels, selected } => {
                let delta = match key {
                    PanelKey::Up | PanelKey::Left | PanelKey::Minus => -1,
                    PanelKey::Down | PanelKey::Right | PanelKey::Plus => 1,
                    PanelKey::Enter | PanelKey::Space => {
                        self.open_select(id, item.rect, values, labels, selected);
                        return true;
                    }
                    _ => return false,
                };
                let at = (selected as isize + delta).clamp(0, values.len() as isize - 1) as usize;
                if at != selected
                    && let Some(value) = values.get(at)
                {
                    self.select(id, value);
                }
                true
            }
            _ => {
                if matches!(key, PanelKey::Enter | PanelKey::Space) {
                    self.activate(id);
                    return true;
                }
                false
            }
        }
    }

    /// A typed character (the number entry, the open list's letter jump).
    pub fn panel_text(&mut self, c: char) -> bool {
        if let Some(entry) = &mut self.chrome.widgets.entry {
            let accepted = match entry.spec {
                Some(_) => c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'),
                None => !c.is_control(),
            };
            if accepted {
                entry.text.push(c);
            }
            return true;
        }
        if let Some(dropdown) = &mut self.chrome.widgets.dropdown {
            if c.is_alphanumeric() {
                dropdown.jump(c);
            }
            return true;
        }
        false
    }

    /// The list of select `id`, under `rect`; a kit sample's or a track's
    /// instrument opens the instrument menus instead, its categories first.
    fn open_select(&mut self, id: Id, rect: panel::Rect, values: Vec<String>, labels: Vec<String>, selected: usize) {
        match id {
            Id::KitSampleInstrument(sample) => self.kit_instrument_menu(sample, rect),
            Id::Instrument => self.instrument_menu(values.get(selected).map_or("", String::as_str), rect),
            _ => self.chrome.widgets.dropdown = Some(Dropdown::new(id, rect, values, labels, selected)),
        }
    }

    // ─── Number entry ────────────────────────────────────────────────────────

    /// `promptNumericInput`: `Enter value for LABEL (min–max unit):`.
    fn open_entry(&mut self, id: Id, spec: Spec, value: f64) {
        let (label, unit) = control_name(id);
        let unit = spec.shown_unit().unwrap_or(unit);
        let unit = if unit.is_empty() { String::new() } else { format!(" {unit}") };
        let (min, max) = (num::text(spec.shown(spec.min)), num::text(spec.shown(spec.max)));
        let title = format!("Enter value for {label} ({min}–{max}{unit}):");
        let text = num::text(spec.shown(value));
        self.show_entry(id, Some(spec), title, text);
    }

    /// The entry of `id` (a number by `spec`, else a text), titled `title`,
    /// holding `text`, opened at the mouse.
    pub(super) fn show_entry(&mut self, id: Id, spec: Option<Spec>, title: String, text: String) {
        let at = self.chrome.widgets.pointer;
        self.chrome.widgets.entry = Some(Entry { id, spec, title, text, at });
    }

    fn commit_entry(&mut self) {
        let Some(entry) = self.chrome.widgets.entry.take() else { return };
        let Some(spec) = entry.spec else { return self.commit_text(entry.id, &entry.text) };
        if entry.text.trim().is_empty() {
            return;
        }
        match Value::from(entry.text.as_str()).to_number() {
            Some(number) => self.set_value(entry.id, spec.typed(spec.typed_shown(number))),
            None => self.tell(format!("\"{}\" is not a number", entry.text.trim())),
        }
    }
}
