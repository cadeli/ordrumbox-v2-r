//! The keys and the typed text on the editors' widgets: the focused one,
//! the open list, the number entry.

use super::Session;
use super::session_panel::PanelKey;
use super::session_panel::control_name;
use crate::control::Spec;
use crate::panel::{self, Item, Kind};
use crate::panel::{Dropdown, Entry, Id};
use crate::scale::Scale;
use ordrumbox_core::json::Value;
use ordrumbox_core::num;

impl Session {
    /// A key for the widgets (the focused one, the open list or entry);
    /// whether it was theirs.
    pub fn panel_key(&mut self, size: (i32, i32, Scale), key: PanelKey, shift: bool, ctrl: bool) -> bool {
        self.chrome.panel_size = size;
        if self.chrome.widgets.entry.is_some() {
            self.entry_key(key);
            return true;
        }
        if self.chrome.widgets.dropdown.is_some() {
            self.dropdown_key(key);
            return true;
        }
        self.focused_key(size, key, shift, ctrl)
    }

    /// A key while the number or text entry is open: it takes them all.
    pub(super) fn entry_key(&mut self, key: PanelKey) {
        let Some(entry) = &mut self.chrome.widgets.entry else { return };
        match key {
            PanelKey::Enter => self.commit_entry(),
            PanelKey::Escape => self.chrome.widgets.entry = None,
            PanelKey::Backspace => {
                entry.text.pop();
            }
            PanelKey::Space if entry.spec.is_none() => entry.text.push(' '),
            _ => {}
        }
    }

    /// A key while a drop-down list is open: it takes them all.
    pub(super) fn dropdown_key(&mut self, key: PanelKey) {
        let Some(dropdown) = &mut self.chrome.widgets.dropdown else { return };
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
    }

    /// A key for the focused widget; whether it was its.
    pub(super) fn focused_key(&mut self, size: (i32, i32, Scale), key: PanelKey, shift: bool, ctrl: bool) -> bool {
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
            Kind::Select { values, labels, selected } | Kind::Screen { values, labels, selected, .. } => {
                let delta = match key {
                    PanelKey::Up | PanelKey::Left | PanelKey::Minus => -1,
                    PanelKey::Down | PanelKey::Right | PanelKey::Plus => 1,
                    PanelKey::Enter | PanelKey::Space => {
                        self.open_select(id, item.rect, values, labels, selected);
                        return true;
                    }
                    _ => return false,
                };
                if let Some(value) = moved_value(&values, selected, delta) {
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
    /// instrument opens the instrument menus instead, its categories first,
    /// and a synth preset's the preset menus, its instruments and styles first.
    pub(super) fn open_select(
        &mut self,
        id: Id,
        rect: panel::Rect,
        values: Vec<String>,
        labels: Vec<String>,
        selected: usize,
    ) {
        match id {
            Id::KitSampleInstrument(sample) => self.kit_instrument_menu(sample, rect),
            Id::Instrument => self.instrument_menu(values.get(selected).map_or("", String::as_str), rect),
            Id::PatternList => self.pattern_style_menu(rect),
            Id::Synth | Id::SynthPreset => {
                self.synth_preset_menu(id, values.get(selected).map_or("", String::as_str), rect);
            }
            _ => self.chrome.widgets.dropdown = Some(Dropdown::new(id, rect, values, labels, selected)),
        }
    }

    // ─── Number entry ────────────────────────────────────────────────────────

    /// Open the typed-number entry: `Enter value for LABEL (min–max unit):`.
    pub(super) fn open_entry(&mut self, id: Id, spec: Spec, value: f64) {
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

    pub(super) fn commit_entry(&mut self) {
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

/// The value of a select `delta` lines from `selected`, held at its ends;
/// `None` when that is the one selected, or the select is empty.
pub(super) fn moved_value(values: &[String], selected: usize, delta: isize) -> Option<&String> {
    let last = values.len().checked_sub(1)?;
    let at = selected.saturating_add_signed(delta).min(last);
    if at == selected { None } else { values.get(at) }
}

#[cfg(test)]
mod tests {
    use super::moved_value;

    #[test]
    fn a_select_moves_and_stops_at_its_ends() {
        let values: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        assert_eq!(moved_value(&values, 0, 1).map(String::as_str), Some("b"));
        assert_eq!(moved_value(&values, 1, 5).map(String::as_str), Some("c"));
        assert_eq!(moved_value(&values, 1, -5).map(String::as_str), Some("a"));
        assert_eq!(moved_value(&values, 2, 1), None, "held at its end");
        assert_eq!(moved_value(&[], 0, 1), None, "an empty select (no synth preset) does not panic");
    }
}
