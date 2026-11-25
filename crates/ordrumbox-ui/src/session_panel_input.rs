//! The mouse, the wheel and the keys on the editors' widgets: what a click,
//! a drag, a hover, a key or a typed number on them does, and the tooltip.

use crate::scale::Scale;
use ordrumbox_core::num;
use std::borrow::Cow;

use super::Session;
use super::session_chrome::Drag;
use super::session_panel::{DOUBLE_CLICK, control_name};
use super::session_panel_keys::moved_value;
use super::session_types::Workspace;
use crate::canvas::Canvas;
use crate::control::{Spec, Style};
use crate::panel::{self, Item, Kind};
use crate::panel::{Dropdown, Id};
use crate::ui_input::Button;
use crate::view;
use crate::view::Layout;
use crate::{control_help, editors};
use std::time::Instant;

impl Session {
    // ─── The mouse ───────────────────────────────────────────────────────────

    /// A press at `(x, y)`; whether the widgets took it.
    pub fn mouse_down(&mut self, size: (i32, i32, Scale), x: i32, y: i32, button: Button, shift: bool) -> bool {
        self.chrome.panel_size = size;
        self.chrome.widgets.pointer = (x, y);
        let (width, height, scale) = size;
        if let Some(dropdown) = self.chrome.widgets.dropdown.take() {
            self.press_dropdown(dropdown, size, (x, y), button == Button::Left);
            return true;
        }
        if let Some(entry) = &self.chrome.widgets.entry {
            if !entry.rect(width, height, scale).contains(x, y) {
                self.chrome.widgets.entry = None;
            }
            return true;
        }
        if button == Button::Left && self.press_wave_handle(size, x, y) {
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
            Kind::Value { spec, value, ref label, ref text, .. } => {
                self.chrome.widgets.focus = Some(id);
                self.press_value(
                    &item,
                    (id, spec, value),
                    (label.is_some(), !text.is_empty()),
                    (x, y),
                    (scale, shift, double),
                );
            }
            Kind::Select { values, labels, selected } | Kind::Screen { values, labels, selected, .. } => {
                self.chrome.widgets.focus = Some(id);
                self.open_select(id, item.rect, values, labels, selected);
            }
            _ => self.activate(id),
        }
        true
    }

    /// A press at `at` while `dropdown` is open: on its scroll bar (its thumb
    /// follows the mouse), on one of its lines, on a menu above it, or
    /// elsewhere (it closes). A line is picked by a left click.
    fn press_dropdown(&mut self, mut dropdown: Dropdown, size: (i32, i32, Scale), at: (i32, i32), left: bool) {
        let (width, height, scale) = size;
        let (x, y) = at;
        if dropdown.scroll_bar(width, height, scale).is_some_and(|bar| bar.contains(x, y)) {
            if left {
                dropdown.bar_held = true;
                dropdown.bar_to(size, y);
            }
            self.chrome.widgets.dropdown = Some(dropdown);
        } else if let Some(i) = dropdown.hit(width, height, scale, x, y) {
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
    }

    /// A press on a knob, slider or stepper `item` (`id`, its `spec` and its
    /// `value`; `labelled`, `valued`: it has a label and a value text) at
    /// `(x, y)`: a step, a reset on a double click, or the start of a drag.
    fn press_value(
        &mut self,
        item: &Item,
        (id, spec, value): (Id, Spec, f64),
        (labelled, valued): (bool, bool),
        (x, y): (i32, i32),
        (scale, shift, double): (Scale, bool, bool),
    ) {
        if spec.style == Style::Stepper {
            let [down, field, up] = panel::stepper_parts(item.rect, labelled, scale);
            if x < down.x - 2 * scale {
                self.select_row(id);
                return;
            }
            // A button is one step a click, however fast the clicks.
            if let Some(up) = [(down, false), (up, true)].iter().find(|(r, _)| r.contains(x, y)).map(|p| p.1) {
                self.chrome.widgets.last_click = None;
                self.set_value(id, spec.nudge(value, up, false, false));
                return;
            }
            if !field.contains(x, y) {
                return;
            }
        }
        if double {
            // Double-click: the default value.
            self.chrome.widgets.last_click = None;
            self.set_value(id, spec.default);
            return;
        }
        let mut drag = Drag { id, spec, start: value, x, y, bar: None, fine: false };
        if spec.style == Style::Slider {
            let (left, w) = panel::slider_bar(item.rect, labelled, valued, scale);
            if x < left - 2 * scale {
                // The label: selects the row.
                self.select_row(id);
                return;
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
        if spec.style == Style::Fader {
            let (top, h) = panel::fader_bar(item.rect, scale);
            if shift {
                drag.fine = true;
            } else {
                drag.bar = Some((top, h));
                let v = spec.slider_at(fader_fraction(y, top, h));
                drag.start = v;
                self.set_value(id, v);
            }
        }
        self.chrome.widgets.drag = Some(drag);
    }

    /// The mouse moved; whether to redraw.
    pub fn mouse_move(&mut self, size: (i32, i32, Scale), x: i32, y: i32, shift: bool) -> bool {
        self.chrome.widgets.pointer = (x, y);
        if self.transport.loop_drag.is_some() {
            return self.pattern_loop_move(size, x);
        }
        if self.roll.dragging() {
            return self.roll_drag_move(x, y);
        }
        if self.is_wave_dragging() {
            return self.wave_drag_move(x);
        }
        if self.roll.bar_drag.is_some() {
            return self.roll_bar_to(size, x);
        }
        if self.roll.zone_move.is_some() {
            return self.roll_zone_move_to(size, x, y);
        }
        if self.roll.zone_drag.is_some() {
            return self.roll_zone_move(size, x, y);
        }
        if self.song_panel.dragging() {
            return self.song_drag_move(size, (x, y), shift);
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
        if self.chrome.lane_resize.is_some() {
            return self.grid_lane_resize_move(y);
        }
        if self.chrome.grid_bar_drag.is_some() {
            return self.grid_bar_held(size, x);
        }
        if self.kit_editor.list.is_bar_held() {
            return self.kit_bar_to(size, y);
        }
        if self.is_mixer_bar_held() {
            return self.mixer_bar_to(size, x);
        }
        if let Some(drag) = self.song_panel.held_bar() {
            return self.song_bar_held(size, drag, x, y);
        }
        let (width, height, scale) = size;
        if let Some(dropdown) = &mut self.chrome.widgets.dropdown {
            if dropdown.bar_held {
                return dropdown.bar_to(size, y);
            }
            if let Some(i) = dropdown.hit(width, height, scale, x, y).filter(|&i| i != dropdown.at) {
                dropdown.at = i;
                return true;
            }
            return false;
        }
        self.control_drag_move(size, x, y, shift)
    }

    /// The mouse moved with a knob, slider, stepper or fader held: its value
    /// follows (finer with `shift` on a knob); whether to redraw.
    fn control_drag_move(&mut self, size: (i32, i32, Scale), x: i32, y: i32, shift: bool) -> bool {
        let Some(drag) = self.chrome.widgets.drag else { return false };
        let value = match (drag.spec.style, drag.bar) {
            (Style::Knob, _) => drag.spec.drag(drag.start, f64::from(drag.y - y), shift),
            (Style::Stepper, _) => drag.spec.drag(drag.start, f64::from(drag.y - y), false),
            (Style::Slider, Some((left, w))) => drag.spec.slider_at(f64::from(x - left) / f64::from(w.max(1))),
            (Style::Slider, None) => {
                let w = self.bar_length(size, drag.id).max(1);
                drag.spec.slider_fine(drag.start, f64::from(x - drag.x) / f64::from(w))
            }
            (Style::Fader, Some((top, h))) => drag.spec.slider_at(fader_fraction(y, top, h)),
            (Style::Fader, None) => {
                let h = self.bar_length(size, drag.id).max(1);
                drag.spec.slider_fine(drag.start, f64::from(drag.y - y) / f64::from(h))
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
                if !self.kit_editor.is_open()
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
                    self.chrome.beat_window,
                    self.chrome.lane_height,
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

    /// What a control is, said in its tooltip.
    pub(super) fn control_tip(&self, id: Id) -> Option<[Cow<'static, str>; 2]> {
        control_help::help(id).map(|lines| lines.map(Into::into))
    }

    /// The tooltip of the note under the mouse (the first of its step), as
    /// `PointerSection.onMouseOver` shows it; over a control, what it is.
    pub(super) fn draw_tooltip(&self, canvas: &mut Canvas, scale: Scale) {
        if let Some((id, rect)) = self.chrome.control_hover.filter(|_| !self.chrome.covered())
            && let Some(lines) = self.control_tip(id)
        {
            return view::draw_tooltip_lines(canvas, &lines, rect, scale);
        }
        if self.draw_lane_tooltip(canvas, scale) {
            return;
        }
        if self.song_view_open() {
            return self.draw_song_tooltip(canvas, scale);
        }
        if self.piano_roll_open() {
            return self.draw_roll_tooltip(canvas, scale);
        }
        let Some(hover) = self.chrome.hover else { return };
        if self.chrome.covered() || self.kit_editor.is_open() {
            return;
        }
        let Some(pattern) = self.pattern() else { return };
        let body = self.body((canvas.width(), canvas.height(), scale));
        let layout = Layout::new(
            body.main_w,
            body.editor_h,
            scale,
            pattern,
            self.status.len(),
            self.chrome.beat_window,
            self.chrome.lane_height,
        );
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
        if let Some(dropdown) = &mut self.chrome.widgets.dropdown {
            dropdown.bar_held = false;
        }
        self.song_panel.release_bar();
        self.kit_editor.list.release_bar();
        self.release_mixer_bar();
        self.chrome.track_drag = None;
        self.chrome.zone_drag = None;
        self.chrome.lane_drag = None;
        self.chrome.lane_resize = None;
        self.release_grid_bar();
        self.roll_release();
        self.release_wave_handle();
    }

    /// The right-click menu of a knob or a slider: type a value, its
    /// default, copy it, paste one copied.
    fn control_menu(&mut self, id: Id, x: i32, y: i32) {
        let lines = [
            ("type", "Type a value..."),
            ("default", "Default value"),
            ("copy", "Copy value"),
            ("paste", "Paste value"),
        ]
        .map(|(value, label)| (value.to_owned(), label.to_owned()));
        let (label, _) = control_name(id);
        let mut menu = Dropdown::menu(Id::ControlMenu, x, y, label.to_owned(), lines);
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
            return self.mixer_wheel(size, x, y, lines);
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
            Kind::Select { values, selected, .. } | Kind::Screen { values, selected, .. } => {
                if let Some(value) = moved_value(&values, selected, lines) {
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

    /// The length of a slider's bar, or of a fader's groove.
    fn bar_length(&self, size: (i32, i32, Scale), id: Id) -> i32 {
        let layout = self.panel_layout(size.0, size.1, size.2);
        match panel::find(&layout.items, id) {
            Some(Item { rect, kind: Kind::Value { spec, .. }, .. }) if spec.style == Style::Fader => {
                panel::fader_bar(*rect, size.2).1
            }
            Some(Item { rect, kind: Kind::Value { label, text, .. }, .. }) => {
                panel::slider_bar(*rect, label.is_some(), !text.is_empty(), size.2).1
            }
            _ => 1,
        }
    }
}

/// Where `y` is up a fader's groove `height` high from `top`, 0 at its foot to 1 at its top.
fn fader_fraction(y: i32, top: i32, height: i32) -> f64 {
    1.0 - f64::from(y - top) / f64::from(height.max(1))
}

#[cfg(test)]
mod tests {
    use super::fader_fraction;

    #[test]
    fn a_fader_is_full_at_its_top_and_empty_at_its_foot() {
        assert_eq!(fader_fraction(10, 10, 100), 1.0);
        assert_eq!(fader_fraction(110, 10, 100), 0.0);
        assert_eq!(fader_fraction(60, 10, 100), 0.5);
    }
}
