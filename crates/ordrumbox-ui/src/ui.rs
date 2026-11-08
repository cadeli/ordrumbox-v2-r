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
use crate::session::{Button, Move, OverlayKey, PanelKey, Session};
use crate::ui_input::{Key, KeyEvent, Modifiers, ScrollDelta};
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
        let needed = view::min_window_size(self.session.wanted_scale(), !self.session.hosted());
        Viewport::new(window, needed, self.display)
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
        // A new message's band goes when its time is over.
        redraw |= animating || self.session.end_flash(now);
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
            None => return self.click_content(x, y, shift),
        }
        true
    }

    /// A click on the grid, the song panel or the kit editor (Shift on a
    /// grid cell: select its note rather than switch the step).
    fn click_content(&mut self, x: i32, y: i32, shift: bool) -> bool {
        let size = self.size();
        let s = &mut self.session;
        if s.song_view_open() {
            return s.song_click(size, x, y, Button::Left);
        }
        if s.piano_roll_open() {
            return s.roll_click(size, x, y, Button::Left);
        }
        let body = s.body(size);
        if let Some((selected, count)) = s.kit_list() {
            let layout = KitLayout::new(body.main_w, body.editor_h, size.2, s.status_lines());
            let Some(hit) = layout.hit(selected, count, x, y) else { return false };
            s.kit_click(hit);
            return true;
        }
        let Some(hit) = s.grid_hit(size, x, y) else { return false };
        match hit {
            view::Hit::Cell { track, beat, step } if shift => s.select_step_note(track, beat, step),
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
        if self.widget_key(event, shift || alt, control) {
            return true;
        }
        // A plugin's Alt stands for the Ctrl/Cmd the host keeps (the menus say Alt+…).
        let command_key = control || (alt && self.session.hosted());
        if let Some(command) = shortcut(code, shift, command_key) {
            // A command a plugin leaves to the host leaves it its key too.
            if self.session.hosted() && command.hosted_refusal().is_some() {
                return false;
            }
            self.session.run(command);
            return true;
        }
        let s = &mut self.session;
        if command_key {
            return false;
        }
        if let Some(hint) = shift_hint(code).filter(|_| !shift && !s.kit_editor_open()) {
            s.say(hint);
            return true;
        }
        // Mute (1-9) and hear (Q-P) a track in every view, as in the JS; the
        // kit editor keeps its own letters (N, R, E).
        let kit_letter = s.kit_editor_open() && matches!(code, Key::Letter('N' | 'R' | 'E'));
        if let Some(row) = ROW_DIGITS.iter().position(|&k| k == code) {
            s.toggle_row(row, if shift { "solo" } else { "mute" });
        } else if let Some(row) = ROW_LETTERS.iter().position(|&k| k == code).filter(|_| !kit_letter) {
            s.audition_row(row);
        } else if s.kit_editor_open() {
            match code {
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
            match code {
                Key::Escape => s.toggle_synth(),
                _ => return false,
            }
        } else if s.piano_roll_open() {
            match code {
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
            match code {
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
            match code {
                Key::Minus | Key::NumpadSubtract if tempo_keys => s.step_tempo(-tempo),
                Key::Equal | Key::NumpadAdd if tempo_keys => s.step_tempo(tempo),
                Key::ArrowLeft if shift => s.nudge(Move::Left),
                Key::ArrowRight if shift => s.nudge(Move::Right),
                Key::ArrowUp if shift => s.nudge(Move::Up),
                Key::ArrowDown if shift => s.nudge(Move::Down),
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
    fn widget_key(&mut self, event: &KeyEvent, fine: bool, control: bool) -> bool {
        let size = self.size();
        let key = match event.key {
            Key::ArrowUp => Some(PanelKey::Up),
            Key::ArrowDown => Some(PanelKey::Down),
            Key::ArrowLeft => Some(PanelKey::Left),
            Key::ArrowRight => Some(PanelKey::Right),
            // Ctrl+- and Ctrl+= stay the pattern length.
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

/// What a letter that changes every track says when pressed without Shift.
fn shift_hint(code: Key) -> Option<&'static str> {
    Some(match code {
        Key::Letter('G') => "Shift+G: a random kit",
        Key::Letter('H') => "Shift+H: every track on a synth sound",
        Key::Letter('J') => "Shift+J: every track back on auto-assign",
        Key::Letter('K') => "Shift+K: a random sample on every track",
        _ => return None,
    })
}

/// The command a key runs, when the menus list it (`view::HELP`).
fn shortcut(code: Key, shift: bool, control: bool) -> Option<Command> {
    use Command as C;
    Some(if control {
        match code {
            Key::Letter('Z') if shift => C::Redo,
            Key::Letter('Z') => C::Undo,
            Key::Letter('Y') => C::Redo,
            Key::Letter('C') if shift => C::CopyTrack,
            Key::Letter('C') => C::CopyStep,
            Key::Letter('V') if shift => C::PasteTrack,
            Key::Letter('V') => C::Paste,
            Key::Letter('S') if shift => C::ExportPatternJson,
            Key::Letter('S') => C::Save,
            Key::Letter('O') => C::ImportMidi,
            Key::Letter('E') if shift => C::ExportSongWav,
            Key::Letter('E') => C::ExportWav,
            Key::Letter('M') => C::ExportMidi,
            Key::Letter('K') => C::KitEditor,
            Key::Letter('N') => C::NewPattern,
            Key::Letter('D') => C::DuplicatePattern,
            Key::Letter('T') => C::AddTrack,
            Key::Backspace | Key::Delete => C::RemoveTrack,
            Key::Letter('G') => C::NextGrid,
            Key::Letter('L') => C::ShortenLoop,
            // Ctrl++ is Ctrl+Shift+= on most layouts; the keypad needs no Shift.
            Key::Equal if shift => C::ZoomIn,
            Key::Minus if shift => C::ZoomOut,
            Key::NumpadAdd => C::ZoomIn,
            Key::NumpadSubtract => C::ZoomOut,
            Key::Digit(0) => C::ZoomReset,
            Key::Minus => C::LengthDown,
            Key::Equal => C::LengthUp,
            Key::Letter('Q') => C::Quit,
            _ => return None,
        }
    } else {
        match code {
            Key::F(1) => C::Shortcuts,
            Key::F(2) => C::TrackEditor,
            Key::F(3) => C::SongView,
            Key::F(4) => C::PianoRoll,
            Key::F(5) => C::Grid,
            Key::F(6) => C::SynthEditor,
            Key::BracketLeft if shift => C::PreviousSong,
            Key::BracketRight if shift => C::NextSong,
            Key::BracketLeft | Key::PageUp => C::PreviousPattern,
            Key::BracketRight | Key::PageDown => C::NextPattern,
            Key::Comma => C::PreviousKit,
            Key::Period => C::NextKit,
            Key::Letter('B') if shift => C::NewGeneratedPattern,
            Key::Letter('B') => C::GeneratePattern,
            Key::Letter('F') => C::SelectRandomPattern,
            // What changes every track wants Shift: a slip off the play keys (Q-P, 1-9) does nothing.
            Key::Letter('G') if shift => C::SelectRandomDrumkit,
            Key::Letter('V') => C::ToggleVus,
            Key::Letter('C') => C::NextColorScheme,
            Key::Letter('H') if shift => C::ConvertToSynth,
            Key::Letter('J') if shift => C::AutoAssignAll,
            Key::Letter('K') if shift => C::RandomSamples,
            Key::Letter('D') => C::GenerateDrums,
            Key::Letter('S') => C::GenerateBass,
            Key::Letter('A') => C::GenerateChords,
            Key::Letter('Z') => C::TrackAuto,
            Key::Letter('X') => C::RandomTrack,
            Key::Letter('L') => C::ToggleAccent,
            _ => return None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_follow_the_command_modifier() {
        assert_eq!(shortcut(Key::Letter('Z'), false, true), Some(Command::Undo));
        assert_eq!(shortcut(Key::Letter('Z'), true, true), Some(Command::Redo));
        assert_eq!(shortcut(Key::Letter('Z'), false, false), Some(Command::TrackAuto));
        // Only the main row's minus changes the length; the keypad's zooms.
        assert_eq!(shortcut(Key::Minus, false, true), Some(Command::LengthDown));
        assert_eq!(shortcut(Key::NumpadSubtract, false, true), Some(Command::ZoomOut));
        assert_eq!(shortcut(Key::Minus, true, true), Some(Command::ZoomOut));
        assert_eq!(shortcut(Key::Equal, true, true), Some(Command::ZoomIn));
        assert_eq!(shortcut(Key::NumpadAdd, false, true), Some(Command::ZoomIn));
        assert_eq!(shortcut(Key::Digit(0), false, true), Some(Command::ZoomReset));
        assert_eq!(shortcut(Key::F(3), false, false), Some(Command::SongView));
        assert_eq!(shortcut(Key::Letter('L'), false, false), Some(Command::ToggleAccent));
        assert_eq!(shortcut(Key::Letter('S'), false, true), Some(Command::Save));
        assert_eq!(shortcut(Key::Letter('S'), true, true), Some(Command::ExportPatternJson));
        for (letter, command) in
            [('G', Command::SelectRandomDrumkit), ('H', Command::ConvertToSynth), ('K', Command::RandomSamples)]
        {
            assert_eq!(shortcut(Key::Letter(letter), true, false), Some(command), "Shift+{letter}");
            assert_eq!(shortcut(Key::Letter(letter), false, false), None, "{letter} alone");
            assert!(shift_hint(Key::Letter(letter)).is_some(), "{letter} says to hold Shift");
        }
    }

    #[test]
    fn a_zoom_bigger_than_the_window_scrolls_it() {
        let library =
            ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
        let mut ui = Ui::new(Session::for_tests(library, false));
        let (w, h) = view::min_window_size(Scale::ONE, true);
        ui.set_size(w, h, Scale::ONE);
        ui.session.run(Command::ZoomIn);
        ui.session.run(Command::ZoomIn);
        let mut pixels = vec![0; (w * h) as usize];
        ui.draw(&mut pixels, w as usize, h as usize);
        let viewport = ui.viewport((w, h));
        assert!(viewport.scrolls(), "the UI at ×1.5 is drawn whole, the window shows a part");
        assert_eq!(ui.session.wanted_scale(), Scale::from_quarters(6), "the zoom asked for, not one that fits");
        // The vertical bar's thumb, dragged to the bottom: the window shows the UI's bottom.
        let bar_x = f64::from(viewport.view.0 + 2);
        ui.pointer_moved(bar_x, 4.0);
        assert!(ui.pointer_pressed(Button::Left));
        assert!(ui.pointer_moved(bar_x, f64::from(h)));
        assert!(ui.pointer_released(Button::Left));
        ui.pointer_moved(0.0, 0.0);
        assert_eq!(ui.at().1, viewport.content.1 - viewport.view.1);
    }

    #[test]
    fn a_plugin_leaves_the_host_its_keys() {
        let library =
            ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
        let session = |hosted| Session::for_tests(library.clone(), hosted);
        let press = |key| KeyEvent { key, pressed: true, repeat: false, text: None };
        let command = Modifiers { logo: true, ..Modifiers::default() };

        let mut app = Ui::new(session(false));
        assert!(app.key(&press(Key::Space)));
        app.set_modifiers(command);
        assert!(app.key(&press(Key::Letter('Z'))));

        let mut plugin = Ui::new(session(true));
        let bpm = plugin.session.tempo();
        // The host's: its transport, Ctrl/Cmd keys, what a plugin leaves out.
        for key in [Key::Space, Key::F(3), Key::Minus, Key::Equal, Key::BracketLeft] {
            let shift = key == Key::BracketLeft;
            plugin.set_modifiers(Modifiers { shift, ..Modifiers::default() });
            assert!(!plugin.key(&press(key)), "{key:?}");
        }
        plugin.set_modifiers(command);
        for key in [Key::Letter('S'), Key::Letter('Z'), Key::Letter('N'), Key::Minus] {
            assert!(!plugin.key(&press(key)), "Cmd+{key:?}");
        }
        assert!(!plugin.session.playing() && !plugin.session.song_view_open());
        assert_eq!(plugin.session.tempo(), bpm);
        assert_eq!(plugin.session.pattern().unwrap().beat_count, session(false).pattern().unwrap().beat_count);
        // The plugin's own keys stay its own.
        plugin.set_modifiers(Modifiers::default());
        assert!(plugin.key(&press(Key::Digit(1))));
        assert!(plugin.key(&press(Key::ArrowRight)));
        assert!(plugin.key(&press(Key::F(4))));
        // Alt stands for the Ctrl the host keeps, but not for what the host does.
        plugin.set_modifiers(Modifiers { alt: true, ..Modifiers::default() });
        let revision = plugin.session.library_revision();
        assert!(plugin.key(&press(Key::Letter('N'))));
        assert_ne!(plugin.session.library_revision(), revision, "a new pattern");
        assert!(plugin.key(&press(Key::Letter('K'))));
        assert!(plugin.session.kit_editor_open());
        assert!(!plugin.key(&press(Key::Letter('S'))), "the host saves");
        assert!(!plugin.key(&press(Key::Letter('Q'))), "the host closes");
    }

    #[test]
    fn alt_alone_opens_the_menus_but_not_after_a_click_or_a_wheel_turn() {
        let library =
            ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
        let mut ui = Ui::new(Session::for_tests(library, false));
        let alt = |pressed| KeyEvent { key: Key::Alt, pressed, repeat: false, text: None };

        ui.key(&alt(true));
        assert!(ui.key(&alt(false)));
        assert!(ui.session.open_menu().is_some(), "Alt alone");
        ui.key(&alt(true));
        ui.key(&alt(false));
        assert!(ui.session.open_menu().is_none(), "Alt alone again closes them");

        ui.key(&alt(true));
        ui.pointer_pressed(Button::Left);
        ui.pointer_released(Button::Left);
        assert!(!ui.key(&alt(false)));
        assert!(ui.session.open_menu().is_none(), "Alt+click");

        ui.key(&alt(true));
        ui.wheel(ScrollDelta::Lines(0.0, 1.0));
        assert!(!ui.key(&alt(false)));
        assert!(ui.session.open_menu().is_none(), "Alt+wheel");
    }

    #[test]
    fn a_new_message_stands_out_even_the_same_words_again_and_then_goes_back() {
        let library =
            ordrumbox_core::Library::parse(r#"{"patterns":[{"name":"demo","tracks":[{"name":"KICK"}]}]}"#).unwrap().0;
        let mut ui = Ui::new(Session::for_tests(library, false));
        let refuse = |ui: &mut Ui| {
            ui.session.say("select a track first");
            false
        };

        assert!(ui.input(refuse), "a message is a redraw");
        let first = ui.session.flash_until().expect("stands out");
        assert!(ui.session.end_flash(first));
        assert_eq!(ui.session.flash_until(), None, "its time is over");

        // The same refusal again stands out again.
        assert!(ui.input(refuse));
        assert!(ui.session.flash_until().is_some_and(|until| until >= first));

        // An input that says nothing keeps the message, its toast left as it is.
        let until = ui.session.flash_until();
        assert!(!ui.input(|_| false));
        assert_eq!(ui.session.message(), "select a track first");
        assert_eq!(ui.session.flash_until(), until);

        // A message given outside an input (an MCP call) stands out at the next idle pass.
        ui.session.end_flash(Instant::now() + Duration::from_secs(60));
        ui.session.say("stopped");
        let idle = ui.idle();
        assert!(ui.session.flash_until().is_some());
        assert!(idle.wake_at.is_some(), "woken when the toast goes");
    }
}
