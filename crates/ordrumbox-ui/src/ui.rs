//! The UI a window drives: the [`Session`] with the input state around it
//! (modifiers, pointer, size). The window translates its events into
//! [`crate::ui_input`] and calls these; each says whether the window must be
//! redrawn.

use crate::scale::Scale;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::canvas::Canvas;
use crate::frame;
use crate::menu::{self, Command};
use crate::scroll::{Scroll, Viewport};
use crate::session::{Move, OverlayKey, PanelKey, Session};
use crate::ui_input::{Button, Key, KeyEvent, MACOS, Modifiers, ScrollDelta};
use crate::view::{self, KitLayout};

/// Redraw interval while playing (the playhead moves).
const FRAME: Duration = Duration::from_millis(16);
/// Rows Page Up / Page Down move in the file picker.
const PAGE: isize = 10;
/// Lines one line of wheel scrolls in the file picker.
const WHEEL_ROWS: isize = 3;
/// Keys of the track rows, in order: 1–9 mute (shift: solo), Q–P audition.
const ROW_DIGITS: [Key; 9] = [
    Key::Digit(1),
    Key::Digit(2),
    Key::Digit(3),
    Key::Digit(4),
    Key::Digit(5),
    Key::Digit(6),
    Key::Digit(7),
    Key::Digit(8),
    Key::Digit(9),
];
const ROW_LETTERS: [Key; 10] = [
    Key::Letter('Q'),
    Key::Letter('W'),
    Key::Letter('E'),
    Key::Letter('R'),
    Key::Letter('T'),
    Key::Letter('Y'),
    Key::Letter('U'),
    Key::Letter('I'),
    Key::Letter('O'),
    Key::Letter('P'),
];

/// What the window does after an idle pass ([`Ui::idle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idle {
    /// Something changed: redraw.
    pub redraw: bool,
    /// When to call again at the latest; `None`: on the next event.
    pub wake_at: Option<Instant>,
}

/// The session and the input state of the window showing it.
#[derive(Debug)]
pub struct Ui {
    pub session: Session,
    modifiers: Modifiers,
    /// The pointer, in physical pixels.
    pointer: (f64, f64),
    /// The window's size in physical pixels.
    window: (i32, i32),
    /// The display's scale, which the scrollbars are drawn at.
    display: Scale,
    /// The part of the UI the window shows when it cannot show it all.
    scroll: Scroll,
    /// Alt is down and no other key, click or wheel turn came since.
    alt_alone: bool,
    /// The window has a resize grip in its bottom-right corner (a
    /// plugin's): the scrollbars leave it free.
    grip: bool,
}

impl Ui {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            modifiers: Modifiers::default(),
            pointer: (0.0, 0.0),
            window: (0, 0),
            display: Scale::ONE,
            scroll: Scroll::default(),
            alt_alone: false,
            grip: false,
        }
    }

    /// The window has a resize grip of [`crate::grip_side`] in its
    /// bottom-right corner, which the scrollbars leave free.
    pub fn set_grip(&mut self, grip: bool) {
        self.grip = grip;
    }

    /// The smallest window, in physical pixels, that shows the whole UI at
    /// its zoom with what it shows now; a smaller one scrolls.
    #[must_use]
    pub fn needed_size(&self) -> (i32, i32) {
        view::min_window_size(self.session.wanted_scale(), self.session.shown())
    }

    /// The window's size in physical pixels and the display's scale factor
    /// (≥ 1), which the zoom starts from (`session_zoom.rs`).
    pub fn set_size(&mut self, width: i32, height: i32, scale: Scale) {
        self.window = (width, height);
        self.display = scale;
        self.session.set_display_scale(scale);
        self.session.set_window_size(width, height);
    }

    /// How a window of `window` pixels shows the UI at its zoom: whole, or
    /// a part with scrollbars.
    fn viewport(&self, window: (i32, i32)) -> Viewport {
        Viewport::new(window, self.needed_size(), self.display, self.grip)
    }

    /// The UI's size in physical pixels (the window's, or the layout's when
    /// it scrolls) and the scale it shows at: the zoomed one, whatever the
    /// window's size.
    fn size(&self) -> (i32, i32, Scale) {
        let (width, height) = self.viewport(self.window).content;
        (width, height, self.session.wanted_scale())
    }

    /// The pointer in the UI's pixels.
    fn at(&self) -> (i32, i32) {
        self.scroll.to_content((self.pointer.0 as i32, self.pointer.1 as i32))
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// Draw the window, `pixels` holding `width × height` `0x00RRGGBB` pixels.
    pub fn draw(&mut self, pixels: &mut [u32], width: usize, height: usize) {
        let scale = self.session.wanted_scale();
        // A window's size: well within `i32`.
        let viewport = self.viewport((width as i32, height as i32));
        self.scroll.fit(&viewport);
        let (content_w, content_h) = viewport.content;
        self.session.set_size((content_w, content_h, scale));
        if !viewport.scrolls() {
            self.session.draw(&mut Canvas::new(pixels, width, height), scale);
            return;
        }
        let content = self.scroll.content_buffer(&viewport);
        let (w, h) = (content_w.unsigned_abs() as usize, content_h.unsigned_abs() as usize);
        self.session.draw(&mut Canvas::new(content, w, h), scale);
        self.scroll.show(&viewport, pixels, width, height);
    }

    /// Between events: MCP calls, autosave, audio errors, update checks,
    /// the playhead and the end of a new message's band.
    pub fn idle(&mut self) -> Idle {
        // MCP calls first: the autosave below counts their edits.
        let called = self.session.poll_mcp();
        let save_at = self.session.autosave();
        let mut redraw = called || self.session.check_audio() || self.session.poll_updates();
        let playing = self.session.playing();
        if playing {
            self.session.update();
        }
        // While playing (the playhead) or while an LFO moves the synth editor.
        let animating = playing || self.session.synth_animating();
        self.session.note_message();
        let now = Instant::now();
        // A toast goes when its time is over, playing or not: checked first,
        // so `animating` cannot skip it.
        redraw |= self.session.end_flash(now) || animating;
        let wake_at = if animating {
            Some(now + FRAME)
        } else {
            [save_at, self.session.flash_until()].into_iter().flatten().min()
        };
        Idle { redraw, wake_at }
    }

    /// A button pressed at the pointer.
    pub fn pointer_pressed(&mut self, button: Button) -> bool {
        self.input(|ui| ui.pressed(button))
    }

    fn pressed(&mut self, button: Button) -> bool {
        // Alt+click or Alt+drag: releasing Alt then leaves the menus closed.
        self.alt_alone = false;
        let window = (self.pointer.0 as i32, self.pointer.1 as i32);
        if button == Button::Left && self.scroll.press(&self.viewport(self.window), window) {
            return true;
        }
        let size = self.size();
        let (width, height, scale) = size;
        let (x, y) = self.at();
        let shift = self.modifiers.shift;
        let s = &mut self.session;
        // The toast is over everything but the overlays: a click on it hides it.
        if !s.overlay_open() && s.toast_click(size, x, y) {
            return true;
        }
        // An open list or number entry takes the click; the right button is the widgets'.
        if s.popup_open() || button == Button::Right {
            let menus_open = s.open_menu().is_some() || s.overlay_open();
            return s.mouse_down(size, x, y, button, shift)
                || (button == Button::Right && !menus_open && s.song_view_open() && s.song_click(size, x, y, button))
                || (button == Button::Right && !menus_open && s.piano_roll_open() && s.roll_click(size, x, y, button))
                || (button == Button::Right && !menus_open && s.grid_menu(size, x, y));
        }
        let menus = s.menus();
        // The menu bar first: it is over everything.
        match menu::hit(&menus, s.open_menu(), scale, x, y) {
            Some(menu::Hit::Title(i)) => {
                let same = s.open_menu().is_some_and(|o| o.menu == i);
                s.close_overlay();
                s.set_menu((!same).then(|| menu::Open::new(&menus, i, false)));
            }
            Some(menu::Hit::Item(i)) => {
                if let Some(command) = s.open_menu().and_then(|o| menu::Open { item: Some(i), ..o }.command(&menus)) {
                    s.run(command);
                }
            }
            Some(menu::Hit::Panel) => {}
            Some(menu::Hit::Bar) => s.set_menu(None),
            None if s.open_menu().is_some() => s.set_menu(None),
            None if s.overlay_open() => s.overlay_click(width, height, scale, x, y),
            // Before the panel's: with the track editor open, its side claims the whole column.
            None if frame::on_status_line(y, scale) => s.open_history(),
            None if s.mouse_down(size, x, y, Button::Left, shift) => {}
            None => return self.click_content(x, y, shift, self.modifiers.command()),
        }
        true
    }

    /// A click on the grid, the song panel or the kit editor (Shift on a
    /// grid cell: select its note rather than switch the step, and a drag
    /// from it draws a zone).
    fn click_content(&mut self, x: i32, y: i32, shift: bool, control: bool) -> bool {
        let size = self.size();
        let s = &mut self.session;
        if s.song_view_open() {
            return if shift { s.song_shift_click(size, x, y) } else { s.song_click(size, x, y, Button::Left) };
        }
        if s.piano_roll_open() {
            return if shift { s.roll_shift_press(size, x, y) } else { s.roll_click(size, x, y, Button::Left) };
        }
        let body = s.body(size);
        if let Some((selected, count)) = s.kit_list() {
            let layout = KitLayout::new(body.main_w, body.editor_h, size.2, s.status_lines());
            let Some(hit) = layout.hit(selected, count, x, y) else { return false };
            s.kit_click(hit);
            return true;
        }
        if s.lane_press(size, x, y) {
            return true;
        }
        let Some(hit) = s.grid_hit(size, x, y) else { return false };
        match hit {
            view::Hit::Cell { track, beat, step } if shift => {
                s.select_step_note(track, beat, step);
                s.press_zone_drag(track, beat, step, x, y);
            }
            // Ctrl+click: the zone grows to take the step in.
            view::Hit::Cell { track, beat, step } if control => s.extend_zone_to(track, beat, step),
            _ => s.click(hit),
        }
        if let view::Hit::Name(t) = hit {
            s.press_track_drag(t, x, y);
        }
        true
    }

    /// A button released.
    pub fn pointer_released(&mut self, button: Button) -> bool {
        self.input(|ui| ui.released(button))
    }

    fn released(&mut self, button: Button) -> bool {
        if button != Button::Left {
            return false;
        }
        if self.scroll.release() {
            return true;
        }
        let redraw = self.session.song_release(self.size()) | self.session.track_release(self.size());
        self.session.mouse_up();
        redraw
    }

    /// The pointer moved to `(x, y)`, in physical pixels.
    pub fn pointer_moved(&mut self, x: f64, y: f64) -> bool {
        self.pointer = (x, y);
        if self.scroll.dragging() {
            return self.scroll.drag_to(&self.viewport(self.window), (x as i32, y as i32));
        }
        let hovered_menu = self.hover();
        let size = self.size();
        let (x, y) = self.at();
        let moved = self.session.mouse_move(size, x, y, self.modifiers.shift);
        let hovered = self.session.grid_hover(size, x, y)
            | self.session.control_hover(size, x, y)
            | self.session.song_hover(size, x, y)
            | self.session.roll_hover(size, x, y);
        hovered_menu || hovered || moved
    }

    /// The pointer moved: an open menu follows it.
    fn hover(&mut self) -> bool {
        let scale = self.size().2;
        let (x, y) = self.at();
        let s = &mut self.session;
        let Some(open) = s.open_menu() else { return false };
        let menus = s.menus();
        let next = match menu::hit(&menus, Some(open), scale, x, y) {
            Some(menu::Hit::Title(i)) if i != open.menu => menu::Open::new(&menus, i, false),
            Some(menu::Hit::Item(i)) => menu::Open { item: Some(i), ..open },
            Some(menu::Hit::Title(_)) => open,
            _ => menu::Open { item: None, ..open },
        };
        if next == open {
            return false;
        }
        s.set_menu(Some(next));
        true
    }

    /// The wheel turned over the pointer.
    pub fn wheel(&mut self, delta: ScrollDelta) -> bool {
        self.alt_alone = false;
        let (columns, lines) = delta.lines();
        if self.session.overlay_open() {
            if lines == 0 {
                return false;
            }
            self.session.overlay_key(OverlayKey::Move(-lines * WHEEL_ROWS));
            return true;
        }
        let size = self.size();
        let (x, y) = self.at();
        let m = self.modifiers;
        let (fine, coarse) = (m.shift || m.alt, m.command());
        // What the pointer is over first; the view scrolls with what is left.
        self.session.wheel(size, x, y, lines, fine, coarse)
            || self.session.song_wheel(size, x, y, (columns, lines), m.shift)
            || self.session.roll_wheel(size, x, y, lines, m.shift)
            || self.scroll.wheel(&self.viewport(self.window), (columns, lines), self.display)
    }

    /// A file or folder dropped on the window.
    pub fn drop_path(&mut self, path: &Path) -> bool {
        self.input(|ui| {
            ui.session.close_overlay();
            ui.session.drop_path(path, ui.modifiers.shift);
            true
        })
    }

    /// Run `handle`, an input: a message it gives stands out, even the one
    /// already shown said again (a refusal repeated).
    fn input(&mut self, handle: impl FnOnce(&mut Self) -> bool) -> bool {
        let before = self.session.input_starts();
        let redraw = handle(self);
        let said = !self.session.message().is_empty();
        self.session.input_done(before);
        redraw || said
    }

    /// Open the first menu from the keyboard, or close the open one.
    fn toggle_menu(&mut self) -> bool {
        let s = &mut self.session;
        let open = s.open_menu();
        s.close_overlay();
        s.set_menu(if open.is_some() { None } else { Some(menu::Open::new(&s.menus(), 0, true)) });
        true
    }

    /// A key pressed or released.
    pub fn key(&mut self, event: &KeyEvent) -> bool {
        self.input(|ui| ui.key_event(event))
    }

    fn key_event(&mut self, event: &KeyEvent) -> bool {
        let code = event.key;
        let is_alt = code == Key::Alt;
        if !event.pressed {
            // Alt pressed and released alone opens the menus, as on Windows.
            return is_alt && std::mem::take(&mut self.alt_alone) && self.toggle_menu();
        }
        self.alt_alone = is_alt && !event.repeat;
        if is_alt {
            return false;
        }
        if code == Key::F(10) {
            return self.toggle_menu();
        }
        let shift = self.modifiers.shift;
        let control = self.modifiers.command();
        let alt = self.modifiers.alt;
        // In a plugin, Ctrl/Cmd keys are the host's (save, undo, zoom…),
        // whatever is open: the key is left to the host.
        if self.session.hosted() && control {
            return false;
        }
        if let Some(open) = self.session.open_menu() {
            return self.menu_key(open, code);
        }
        // Alt plus the first letter of a menu's title opens it (Y: Play, the
        // Pattern's P taken). The app only: a plugin's Alt is the Ctrl the
        // host keeps.
        if alt
            && !self.session.hosted()
            && let Some(i) = code.letter()
        {
            let s = &mut self.session;
            let menus = s.menus();
            if let Some(menu) = menus.iter().position(|m| menu_letter(m.title, i)) {
                s.close_overlay();
                s.set_menu(Some(menu::Open::new(&menus, menu, true)));
                return true;
            }
        }
        if self.session.overlay_open() {
            return self.overlay_key(event);
        }
        // Space too, its transport, unless an entry types it.
        if self.session.hosted() && code == Key::Space && !self.session.entry_open() {
            return false;
        }
        // Space plays or stops wherever the focus is (a button, a knob, an open
        // list…), except while writing text: an open entry keeps it (it types a
        // space). Menus and the file picker are modal and take it first.
        if code == Key::Space && !event.repeat && !control && !self.session.entry_open() {
            if shift {
                self.session.toggle_play_song();
            } else {
                self.session.toggle_play();
            }
            return true;
        }
        // A plugin's Alt stands for the Ctrl/Cmd the host keeps (the menus say Alt+…).
        let command_key = control || (alt && self.session.hosted());
        // The keys of the rows (1-9 mute, Q-P hear) are read by their place, a
        // keyboard of its own on every layout; every other key is the one the
        // layout marks. The kit editor keeps its own letters (N, R, E).
        let layout = event.layout_key();
        let kit_letter = self.session.kit_editor_open() && matches!(layout, Key::Letter('N' | 'R' | 'E'));
        let row_key = !command_key && !kit_letter && (ROW_DIGITS.contains(&code) || ROW_LETTERS.contains(&code));
        let key = if row_key { code } else { layout };
        if self.widget_key(event, key, shift || alt, control) {
            return true;
        }
        let s = &mut self.session;
        if let Some(row) = ROW_DIGITS.iter().position(|&k| k == code).filter(|_| row_key) {
            s.toggle_row(row, if shift { "solo" } else { "mute" });
            return true;
        }
        if let Some(row) = ROW_LETTERS.iter().position(|&k| k == code).filter(|_| row_key) {
            s.audition_row(row);
            if !event.repeat {
                s.record_note(row);
            }
            return true;
        }
        if let Some(command) = shortcut(key, shift, command_key) {
            // A command a plugin leaves to the host leaves it its key too.
            if s.hosted() && command.hosted_refusal().is_some() {
                return false;
            }
            s.run(command);
            return true;
        }
        // Alt+arrows nudge the selected note, in a plugin too, where Alt is the host's Ctrl.
        let nudge_key =
            alt && !control && matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown);
        if command_key && !nudge_key {
            return false;
        }
        if let Some(hint) = shift_hint(key).filter(|_| !shift && !s.kit_editor_open()) {
            s.say(hint);
            return true;
        }
        // Home and the keypad's Enter rewind the transport (the kit editor's Enter hears a sample).
        if matches!(key, Key::Home | Key::NumpadEnter) && !s.hosted() && !s.kit_editor_open() && !s.synth_open() {
            s.rewind();
            return true;
        }
        if s.kit_editor_open() {
            match key {
                Key::Escape => s.toggle_kit_editor(),
                Key::ArrowUp => s.kit_select(-1),
                Key::ArrowDown => s.kit_select(1),
                Key::Enter | Key::NumpadEnter => s.kit_audition(),
                Key::Delete | Key::Backspace => s.kit_remove(),
                Key::Letter('N') => s.kit_normalize(),
                Key::Letter('E') => s.run(Command::ExportMapping),
                _ => return false,
            }
        } else if s.synth_open() {
            match key {
                Key::Escape => s.toggle_synth(),
                _ => return false,
            }
        } else if s.piano_roll_open() {
            match key {
                Key::Escape => s.roll_escape(),
                Key::ArrowLeft => s.roll_arrow(Move::Left, shift),
                Key::ArrowRight => s.roll_arrow(Move::Right, shift),
                Key::ArrowUp => s.roll_arrow(Move::Up, shift),
                Key::ArrowDown => s.roll_arrow(Move::Down, shift),
                Key::Enter | Key::NumpadEnter => s.roll_enter(),
                Key::Delete | Key::Backspace => s.roll_delete(),
                _ => return false,
            }
        } else if s.song_view_open() {
            match key {
                Key::Escape => s.toggle_song_view(),
                Key::ArrowLeft => s.move_song_cursor(-1),
                Key::ArrowRight => s.move_song_cursor(1),
                Key::ArrowUp => s.step_pattern(-1),
                Key::ArrowDown => s.step_pattern(1),
                _ => return false,
            }
        } else {
            let tempo = if shift { 10.0 } else { 1.0 };
            let tempo_keys = !s.hosted();
            match key {
                Key::Minus | Key::NumpadSubtract if tempo_keys => s.step_tempo(-tempo),
                Key::Equal | Key::NumpadAdd if tempo_keys => s.step_tempo(tempo),
                Key::ArrowLeft if alt => s.nudge(Move::Left),
                Key::ArrowRight if alt => s.nudge(Move::Right),
                Key::ArrowUp if alt => s.nudge(Move::Up),
                Key::ArrowDown if alt => s.nudge(Move::Down),
                Key::ArrowLeft if shift => s.extend_zone(Move::Left),
                Key::ArrowRight if shift => s.extend_zone(Move::Right),
                Key::ArrowUp if shift => s.extend_zone(Move::Up),
                Key::ArrowDown if shift => s.extend_zone(Move::Down),
                Key::ArrowLeft => s.move_cursor(Move::Left),
                Key::ArrowRight => s.move_cursor(Move::Right),
                Key::ArrowUp => s.move_cursor(Move::Up),
                Key::ArrowDown => s.move_cursor(Move::Down),
                Key::Enter | Key::NumpadEnter => s.enter(),
                Key::Delete | Key::Backspace => s.delete_step(),
                Key::Escape => s.deselect(),
                _ => return false,
            }
        }
        true
    }

    /// Keys for the editors' widgets: an open list or number entry takes
    /// them all; a focused knob, slider, select or button its own; Tab moves
    /// the focus. Whether the key was theirs.
    fn widget_key(&mut self, event: &KeyEvent, key: Key, fine: bool, control: bool) -> bool {
        let size = self.size();
        let key = match key {
            Key::ArrowUp => Some(PanelKey::Up),
            Key::ArrowDown => Some(PanelKey::Down),
            Key::ArrowLeft => Some(PanelKey::Left),
            Key::ArrowRight => Some(PanelKey::Right),
            // Ctrl+- and Ctrl+= stay the zoom.
            Key::Minus | Key::NumpadSubtract if !control => Some(PanelKey::Minus),
            Key::Equal | Key::NumpadAdd if !control => Some(PanelKey::Plus),
            Key::Enter | Key::NumpadEnter => Some(PanelKey::Enter),
            Key::Space => Some(PanelKey::Space),
            Key::Escape => Some(PanelKey::Escape),
            Key::Tab => Some(PanelKey::Tab),
            Key::PageUp => Some(PanelKey::PageUp),
            Key::PageDown => Some(PanelKey::PageDown),
            Key::Home => Some(PanelKey::Home),
            Key::End => Some(PanelKey::End),
            Key::Backspace => Some(PanelKey::Backspace),
            _ => None,
        };
        let s = &mut self.session;
        if s.popup_open() {
            match key {
                Some(key) if !(key == PanelKey::Minus || key == PanelKey::Plus) => {
                    s.panel_key(size, key, fine, control);
                }
                _ => {
                    for c in event.text.as_deref().unwrap_or("").chars() {
                        s.panel_text(c);
                    }
                }
            }
            return true;
        }
        key.is_some_and(|key| s.panel_key(size, key, fine, control))
    }

    /// Keys while a menu is open.
    fn menu_key(&mut self, open: menu::Open, code: Key) -> bool {
        let s = &mut self.session;
        let menus = s.menus();
        match code {
            Key::Escape => s.set_menu(None),
            Key::ArrowLeft => s.set_menu(Some(open.switch(&menus, -1))),
            Key::ArrowRight => s.set_menu(Some(open.switch(&menus, 1))),
            Key::ArrowUp => s.set_menu(Some(open.step(&menus, -1))),
            Key::ArrowDown => s.set_menu(Some(open.step(&menus, 1))),
            Key::Home => s.set_menu(Some(open.step(&menus, isize::MIN / 2))),
            Key::End => s.set_menu(Some(open.step(&menus, isize::MAX / 2))),
            Key::Letter(c) => s.set_menu(Some(open.jump(&menus, c))),
            Key::Enter | Key::NumpadEnter | Key::Space => match open.command(&menus) {
                Some(command) => s.run(command),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// Keys while the file picker (or another panel) is open.
    fn overlay_key(&mut self, event: &KeyEvent) -> bool {
        let key = match event.key {
            Key::Escape => OverlayKey::Escape,
            Key::Enter | Key::NumpadEnter => OverlayKey::Enter,
            Key::Backspace => OverlayKey::Back,
            Key::ArrowUp => OverlayKey::Move(-1),
            Key::ArrowDown => OverlayKey::Move(1),
            Key::ArrowLeft => OverlayKey::Side(-1),
            Key::ArrowRight => OverlayKey::Side(1),
            Key::PageUp => OverlayKey::Move(-PAGE),
            Key::PageDown => OverlayKey::Move(PAGE),
            Key::Home => OverlayKey::Edge(false),
            Key::End => OverlayKey::Edge(true),
            _ => match event.text.as_ref().and_then(|t| t.chars().next()).filter(|c| c.is_alphanumeric()) {
                Some(c) => OverlayKey::Letter(c),
                None => return false,
            },
        };
        self.session.overlay_key(key);
        true
    }
}

#[path = "ui/keys.rs"]
mod keys;
use keys::*;

#[cfg(test)]
#[path = "ui/tests.rs"]
mod tests;
