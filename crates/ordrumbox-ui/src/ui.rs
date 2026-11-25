//! The UI a window drives: the [`Session`] with the input state around it
//! (modifiers, pointer, size). The window translates its events into
//! [`crate::ui_input`] and calls these; each says whether the window must be
//! redrawn.

use crate::scale::Scale;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::canvas::Canvas;
use crate::frame;
use crate::menu;
use crate::scroll::{Scroll, Viewport};
use crate::session::{OverlayKey, Session, WheelMode};
use crate::ui_input::{Button, Key, Modifiers, PointerShape, ScrollDelta};
use crate::view;

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
        }
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
        Viewport::new(window, self.needed_size(), self.display)
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
        // Following a MIDI clock, the engine is fed even while stopped: the
        // pass it starts in is posted before the other machine starts.
        if playing || self.session.follows_clock() {
            self.session.update();
        }
        // While playing (the playhead), while the other machine plays, or
        // while an LFO moves the synth editor.
        let animating = playing || self.session.clock_running() || self.session.synth_animating();
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
            // A second click on a clip: its pattern, not what its menu would do.
            return (button == Button::Left && s.song_double_click(size, x, y))
                || s.mouse_down(size, x, y, button, shift)
                || (button == Button::Right && !menus_open && s.song_view_open() && s.song_click(size, x, y, button))
                || (button == Button::Right && !menus_open && s.piano_roll_open() && s.roll_click(size, x, y, button))
                || (button == Button::Right && !menus_open && s.pattern_ruler_menu(size, x, y))
                || (button == Button::Right && !menus_open && s.grid_lane_menu(size, x, y))
                || (button == Button::Right && !menus_open && s.grid_menu(size, x, y));
        }
        let menus = s.menus();
        // The menu bar first: it is over everything.
        match menu::spot_at(&menus, s.open_menu(), scale, x, y) {
            Some(menu::Spot::Title(i)) => {
                let same = s.open_menu().is_some_and(|o| o.menu == i);
                s.close_overlay();
                s.set_menu((!same).then(|| menu::Open::new(&menus, i, false)));
            }
            Some(menu::Spot::Item(i)) => {
                if let Some(command) = s.open_menu().and_then(|o| menu::Open { item: Some(i), ..o }.command(&menus)) {
                    s.run(command);
                }
            }
            Some(menu::Spot::Panel) => {}
            Some(menu::Spot::Bar) => s.set_menu(None),
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
        if s.mixer_bar_press(size, x, y) {
            return true;
        }
        if let Some((_, count)) = s.kit_list() {
            if s.kit_bar_press(size, x, y) {
                return true;
            }
            let layout = s.kit_layout(size);
            let Some(sample) = layout.sample_at(s.kit_first(&layout), count, x, y) else { return false };
            s.kit_click(sample);
            return true;
        }
        if s.grid_lane_resize_press(size, x, y)
            || s.lane_press(size, x, y)
            || s.grid_bar_press(size, x, y)
            || s.press_pattern_ruler(size, x, y)
        {
            return true;
        }
        let Some(spot) = s.grid_spot_at(size, x, y) else { return false };
        match spot {
            view::Spot::Cell { track, beat, step } if shift => {
                s.select_step_note(track, beat, step);
                s.press_zone_drag(track, beat, step, x, y);
            }
            // Ctrl+click: the zone grows to take the step in.
            view::Spot::Cell { track, beat, step } if control => s.extend_zone_to(track, beat, step),
            _ => s.click(spot),
        }
        if let view::Spot::Name(t) = spot {
            s.press_track_name(t, x, y);
        }
        true
    }

    /// A button released.
    pub fn pointer_released(&mut self, button: Button) -> bool {
        self.input(|ui| ui.released(button))
    }

    fn released(&mut self, button: Button) -> bool {
        if button != Button::Left {
            // A lane erased with the right button.
            return button == Button::Right && self.session.lane_paint_release();
        }
        if self.scroll.release() {
            return true;
        }
        let redraw = self.session.song_release(self.size())
            | self.session.track_release(self.size())
            | self.session.pattern_loop_release(self.size());
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
            | self.session.roll_hover(size, x, y)
            | self.session.hint_hover(size, x, y);
        hovered_menu || hovered || moved
    }

    /// The pointer's shape where it is, for the window's cursor: the
    /// window's scrollbars first, then the views under them.
    #[must_use]
    pub fn pointer_shape(&self) -> PointerShape {
        let window = (self.pointer.0 as i32, self.pointer.1 as i32);
        if let Some(shape) = self.scroll.pointer_shape(&self.viewport(self.window), window) {
            return shape;
        }
        let (x, y) = self.at();
        self.session.pointer_shape(self.size(), x, y)
    }

    /// The pointer moved: an open menu follows it.
    fn hover(&mut self) -> bool {
        let scale = self.size().2;
        let (x, y) = self.at();
        let s = &mut self.session;
        let Some(open) = s.open_menu() else { return false };
        let menus = s.menus();
        let next = match menu::spot_at(&menus, Some(open), scale, x, y) {
            Some(menu::Spot::Title(i)) if i != open.menu => menu::Open::new(&menus, i, false),
            Some(menu::Spot::Item(i)) => menu::Open { item: Some(i), ..open },
            Some(menu::Spot::Title(_)) => open,
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
        let wheel_mode = if m.command() {
            WheelMode::Zoom
        } else if m.shift {
            WheelMode::Scroll
        } else {
            WheelMode::Plain
        };
        // What the pointer is over first; the view scrolls with what is left.
        self.session.kit_wheel(size, x, y, lines)
            || self.session.wheel(size, x, y, lines, fine, coarse)
            || (m.command() && self.session.song_zoom_wheel(size, x, y, lines))
            || self.session.grid_wheel(size, (x, y), (columns, lines), wheel_mode)
            || self.session.song_wheel(size, x, y, (columns, lines), m.shift)
            || self.session.roll_wheel(size, (x, y), lines, wheel_mode)
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
}

#[path = "ui/key_input.rs"]
mod key_input;
#[path = "ui/keys.rs"]
mod keys;

#[cfg(test)]
#[path = "ui/tests.rs"]
mod tests;
