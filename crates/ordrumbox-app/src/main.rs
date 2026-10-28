//! orDrumbox native application.
//!
//! Loads the ordrumbox-v2 assets (patterns, kits, samples, synth presets) and
//! the user's library, plays the selected pattern through the default audio
//! device (per-track strips, synth tracks, master bus), and edits it with the
//! menu bar, the mouse and the keyboard (see `menu` and `view::HELP`), every
//! edit undoable and saved a moment later.

// No console window in a release build on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod audio;
mod canvas;
mod control;
mod editors;
mod font;
mod kits;
mod menu;
mod note_drag;
mod output;
mod panel;
mod piano_roll;
mod picker;
mod session;
mod song_view;
mod spectrum;
mod store;
mod synth_model;
mod synth_view;
mod theme;
mod view;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use canvas::Canvas;
use menu::Command;
use session::{Button, Move, OverlayKey, PanelKey, Session};
use view::{KitLayout, Layout};

const TITLE: &str = "orDrumbox";
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1100.0, 680.0);
/// Redraw interval while playing (the playhead moves).
const FRAME: Duration = Duration::from_millis(16);
/// Keys of the track rows, in order: 1–9 mute (shift: solo), Q–P audition.
const ROW_DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];
/// Rows Page Up / Page Down move in the file picker.
const PAGE: isize = 10;
const ROW_LETTERS: [KeyCode; 10] = [
    KeyCode::KeyQ,
    KeyCode::KeyW,
    KeyCode::KeyE,
    KeyCode::KeyR,
    KeyCode::KeyT,
    KeyCode::KeyY,
    KeyCode::KeyU,
    KeyCode::KeyI,
    KeyCode::KeyO,
    KeyCode::KeyP,
];

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct App {
    gfx: Option<Gfx>,
    session: Session,
    modifiers: ModifiersState,
    mouse: PhysicalPosition<f64>,
    error: Option<String>,
    /// Alt is down and no other key was pressed since.
    alt_alone: bool,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, context: &str, error: impl std::fmt::Display) {
        self.error = Some(format!("{context}: {error}"));
        event_loop.exit();
    }

    fn scale(&self) -> i32 {
        self.gfx.as_ref().map_or(1, |g| g.window.scale_factor().round().max(1.0) as i32)
    }

    fn request_redraw(&self) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    fn redraw(&mut self) -> Result<(), String> {
        let scale = self.scale();
        let Some(gfx) = &mut self.gfx else { return Ok(()) };
        let size = gfx.window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // minimised
        };
        gfx.surface.resize(width, height).map_err(|e| format!("resize surface: {e}"))?;
        let mut buffer = gfx.surface.buffer_mut().map_err(|e| format!("get frame buffer: {e}"))?;
        self.session.set_size((width.get() as i32, height.get() as i32, scale));
        self.session.draw(&mut Canvas::new(&mut buffer, width.get() as usize, height.get() as usize), scale);
        // `ORDRUMBOX_SCREENSHOT=path`: keep the first frame as a BMP there and
        // quit — to check the layout without a screen capture.
        if let Ok(path) = std::env::var("ORDRUMBOX_SCREENSHOT") {
            let _ = write_bmp(&path, &buffer, width.get() as usize, height.get() as usize);
            std::process::exit(0);
        }
        buffer.present().map_err(|e| format!("present frame: {e}"))
    }

    fn size(&self) -> (i32, i32) {
        self.gfx.as_ref().map_or((0, 0), |g| {
            let size = g.window.inner_size();
            (size.width as i32, size.height as i32)
        })
    }

    fn click(&mut self, button: Button) {
        let (width, height) = self.size();
        let (scale, x, y) = (self.scale(), self.mouse.x as i32, self.mouse.y as i32);
        let shift = self.modifiers.shift_key();
        let s = &mut self.session;
        // An open list or number entry takes the click; the right button is the widgets'.
        if s.popup_open() || button == Button::Right {
            let size = (width, height, scale);
            let menus_open = s.open_menu().is_some() || s.overlay_open();
            if s.mouse_down(size, x, y, button, shift)
                || (button == Button::Right && !menus_open && s.song_view_open() && s.song_click(size, x, y, button))
                || (button == Button::Right && !menus_open && s.piano_roll_open() && s.roll_click(size, x, y, button))
            {
                self.request_redraw();
            }
            return;
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
            None if s.mouse_down((width, height, scale), x, y, Button::Left, shift) => {}
            None => return self.click_content(width, height, scale, x, y),
        }
        self.request_redraw();
    }

    /// A click on the grid, the song panel or the kit editor.
    fn click_content(&mut self, width: i32, height: i32, scale: i32, x: i32, y: i32) {
        if self.session.song_view_open() {
            if self.session.song_click((width, height, scale), x, y, Button::Left) {
                self.request_redraw();
            }
            return;
        }
        if self.session.piano_roll_open() {
            if self.session.roll_click((width, height, scale), x, y, Button::Left) {
                self.request_redraw();
            }
            return;
        }
        let body = self.session.body((width, height, scale));
        if let Some((selected, count)) = self.session.kit_list() {
            let layout = KitLayout::new(body.main_w, body.editor_h, scale, self.session.status_lines());
            if let Some(hit) = layout.hit(selected, count, x, y) {
                self.session.kit_click(hit);
                self.request_redraw();
            }
            return;
        }
        let Some(pattern) = self.session.pattern() else { return };
        let layout = Layout::new(body.main_w, body.editor_h, scale, pattern, self.session.status_lines());
        if let Some(hit) = layout.hit(pattern, x, y) {
            self.session.click(hit);
            self.request_redraw();
        }
    }

    /// The mouse moved: an open menu follows it.
    fn hover(&mut self) {
        let scale = self.scale();
        let s = &mut self.session;
        let Some(open) = s.open_menu() else { return };
        let menus = s.menus();
        let next = match menu::hit(&menus, Some(open), scale, self.mouse.x as i32, self.mouse.y as i32) {
            Some(menu::Hit::Title(i)) if i != open.menu => menu::Open::new(&menus, i, false),
            Some(menu::Hit::Item(i)) => menu::Open { item: Some(i), ..open },
            Some(menu::Hit::Title(_)) => open,
            _ => menu::Open { item: None, ..open },
        };
        if next != open {
            s.set_menu(Some(next));
            self.request_redraw();
        }
    }

    /// Open the first menu from the keyboard, or close the open one.
    fn toggle_menu(&mut self) {
        let s = &mut self.session;
        let open = s.open_menu();
        s.close_overlay();
        s.set_menu(if open.is_some() { None } else { Some(menu::Open::new(&s.menus(), 0, true)) });
        self.request_redraw();
    }

    fn key(&mut self, event: &KeyEvent) {
        let PhysicalKey::Code(code) = event.physical_key else { return };
        let is_alt = matches!(code, KeyCode::AltLeft | KeyCode::AltRight);
        if event.state != ElementState::Pressed {
            // Alt pressed and released alone opens the menus, as on Windows.
            if is_alt && std::mem::take(&mut self.alt_alone) {
                self.toggle_menu();
            }
            return;
        }
        self.alt_alone = is_alt && !event.repeat;
        if is_alt {
            return;
        }
        if code == KeyCode::F10 {
            return self.toggle_menu();
        }
        if let Some(open) = self.session.open_menu() {
            return self.menu_key(open, code);
        }
        if self.session.overlay_open() {
            return self.overlay_key(event, code);
        }
        let shift = self.modifiers.shift_key();
        let control = self.modifiers.control_key() || self.modifiers.super_key();
        let alt = self.modifiers.alt_key();
        // Space plays or stops wherever the focus is (a button, a knob, an open
        // list…), except while writing text: an open entry keeps it (it types a
        // space). Menus and the file picker are modal and take it first.
        if code == KeyCode::Space && !event.repeat && !control && !self.session.entry_open() {
            if shift {
                self.session.toggle_play_song();
            } else {
                self.session.toggle_play();
            }
            return self.request_redraw();
        }
        if self.widget_key(event, code, shift || alt, control) {
            return self.request_redraw();
        }
        if let Some(command) = shortcut(code, shift, control) {
            self.session.run(command);
            return self.request_redraw();
        }
        let s = &mut self.session;
        if control {
            return;
        } else if s.kit_editor_open() {
            match code {
                KeyCode::Escape => s.toggle_kit_editor(),
                KeyCode::ArrowUp => s.kit_select(-1),
                KeyCode::ArrowDown => s.kit_select(1),
                KeyCode::Enter | KeyCode::NumpadEnter => s.kit_audition(),
                KeyCode::Delete | KeyCode::Backspace => s.kit_remove(),
                KeyCode::KeyN => s.kit_normalize(),
                KeyCode::KeyR => s.kit_reassign(),
                KeyCode::KeyE => s.kit_export(),
                _ => return,
            }
        } else if s.synth_open() {
            match code {
                KeyCode::Escape => s.toggle_synth(),
                _ => return,
            }
        } else if s.piano_roll_open() {
            match code {
                KeyCode::Escape => s.roll_escape(),
                KeyCode::ArrowLeft => s.roll_arrow(Move::Left, shift),
                KeyCode::ArrowRight => s.roll_arrow(Move::Right, shift),
                KeyCode::ArrowUp => s.roll_arrow(Move::Up, shift),
                KeyCode::ArrowDown => s.roll_arrow(Move::Down, shift),
                KeyCode::Enter | KeyCode::NumpadEnter => s.roll_enter(),
                KeyCode::Delete | KeyCode::Backspace => s.roll_delete(),
                _ => return,
            }
        } else if s.song_view_open() {
            match code {
                KeyCode::Escape => s.toggle_song_view(),
                KeyCode::ArrowLeft => s.move_song_cursor(-1),
                KeyCode::ArrowRight => s.move_song_cursor(1),
                KeyCode::ArrowUp => s.step_pattern(-1),
                KeyCode::ArrowDown => s.step_pattern(1),
                _ => return,
            }
        } else if let Some(row) = ROW_DIGITS.iter().position(|&k| k == code) {
            s.toggle_row(row, if shift { "solo" } else { "mute" });
        } else if let Some(row) = ROW_LETTERS.iter().position(|&k| k == code) {
            s.audition_row(row);
        } else {
            let tempo = if shift { 10.0 } else { 1.0 };
            match code {
                KeyCode::Minus | KeyCode::NumpadSubtract => s.step_tempo(-tempo),
                KeyCode::Equal | KeyCode::NumpadAdd => s.step_tempo(tempo),
                KeyCode::ArrowLeft if shift => s.nudge(Move::Left),
                KeyCode::ArrowRight if shift => s.nudge(Move::Right),
                KeyCode::ArrowUp if shift => s.nudge(Move::Up),
                KeyCode::ArrowDown if shift => s.nudge(Move::Down),
                KeyCode::ArrowLeft => s.move_cursor(Move::Left),
                KeyCode::ArrowRight => s.move_cursor(Move::Right),
                KeyCode::ArrowUp => s.move_cursor(Move::Up),
                KeyCode::ArrowDown => s.move_cursor(Move::Down),
                KeyCode::Enter | KeyCode::NumpadEnter => s.enter(),
                KeyCode::Delete | KeyCode::Backspace => s.delete_step(),
                KeyCode::Escape => s.deselect(),
                _ => return,
            }
        }
        self.request_redraw();
    }

    /// Keys for the editors' widgets: an open list or number entry takes
    /// them all; a focused knob, slider, select or button its own; Tab moves
    /// the focus. Whether the key was theirs.
    fn widget_key(&mut self, event: &KeyEvent, code: KeyCode, fine: bool, control: bool) -> bool {
        let size = (self.size().0, self.size().1, self.scale());
        let key = match code {
            KeyCode::ArrowUp => Some(PanelKey::Up),
            KeyCode::ArrowDown => Some(PanelKey::Down),
            KeyCode::ArrowLeft => Some(PanelKey::Left),
            KeyCode::ArrowRight => Some(PanelKey::Right),
            // Ctrl+- and Ctrl+= stay the pattern length.
            KeyCode::Minus | KeyCode::NumpadSubtract if !control => Some(PanelKey::Minus),
            KeyCode::Equal | KeyCode::NumpadAdd if !control => Some(PanelKey::Plus),
            KeyCode::Enter | KeyCode::NumpadEnter => Some(PanelKey::Enter),
            KeyCode::Space => Some(PanelKey::Space),
            KeyCode::Escape => Some(PanelKey::Escape),
            KeyCode::Tab => Some(PanelKey::Tab),
            KeyCode::PageUp => Some(PanelKey::PageUp),
            KeyCode::PageDown => Some(PanelKey::PageDown),
            KeyCode::Home => Some(PanelKey::Home),
            KeyCode::End => Some(PanelKey::End),
            KeyCode::Backspace => Some(PanelKey::Backspace),
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
    fn menu_key(&mut self, open: menu::Open, code: KeyCode) {
        let s = &mut self.session;
        let menus = s.menus();
        match code {
            KeyCode::Escape => s.set_menu(None),
            KeyCode::ArrowLeft => s.set_menu(Some(open.switch(&menus, -1))),
            KeyCode::ArrowRight => s.set_menu(Some(open.switch(&menus, 1))),
            KeyCode::ArrowUp => s.set_menu(Some(open.step(&menus, -1))),
            KeyCode::ArrowDown => s.set_menu(Some(open.step(&menus, 1))),
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => match open.command(&menus) {
                Some(command) => s.run(command),
                None => return,
            },
            _ => return,
        }
        self.request_redraw();
    }

    /// Keys while the file picker (or another panel) is open.
    fn overlay_key(&mut self, event: &KeyEvent, code: KeyCode) {
        let key = match code {
            KeyCode::Escape => OverlayKey::Escape,
            KeyCode::Enter | KeyCode::NumpadEnter => OverlayKey::Enter,
            KeyCode::Backspace => OverlayKey::Back,
            KeyCode::ArrowUp => OverlayKey::Move(-1),
            KeyCode::ArrowDown => OverlayKey::Move(1),
            KeyCode::PageUp => OverlayKey::Move(-PAGE),
            KeyCode::PageDown => OverlayKey::Move(PAGE),
            KeyCode::Home => OverlayKey::Edge(false),
            KeyCode::End => OverlayKey::Edge(true),
            _ => match event.text.as_ref().and_then(|t| t.chars().next()).filter(|c| c.is_alphanumeric()) {
                Some(c) => OverlayKey::Letter(c),
                None => return,
            },
        };
        self.session.overlay_key(key);
        self.request_redraw();
    }

    /// Finish: save, then leave.
    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        self.session.save_if_changed();
        event_loop.exit();
    }
}

/// The command a key runs, when the menus list it (`view::HELP`).
fn shortcut(code: KeyCode, shift: bool, control: bool) -> Option<Command> {
    use Command as C;
    Some(if control {
        match code {
            KeyCode::KeyZ if shift => C::Redo,
            KeyCode::KeyZ => C::Undo,
            KeyCode::KeyY => C::Redo,
            KeyCode::KeyC if shift => C::CopyTrack,
            KeyCode::KeyC => C::CopyStep,
            KeyCode::KeyV if shift => C::PasteTrack,
            KeyCode::KeyV => C::Paste,
            KeyCode::KeyS => C::Save,
            KeyCode::KeyO => C::ImportMidi,
            KeyCode::KeyE if shift => C::ExportSongWav,
            KeyCode::KeyE => C::ExportWav,
            KeyCode::KeyM => C::ExportMidi,
            KeyCode::KeyK => C::KitEditor,
            KeyCode::KeyN => C::NewPattern,
            KeyCode::KeyD => C::DuplicatePattern,
            KeyCode::KeyT => C::AddTrack,
            KeyCode::Backspace | KeyCode::Delete => C::RemoveTrack,
            KeyCode::KeyG => C::NextGrid,
            KeyCode::KeyL => C::LoopBack,
            KeyCode::Minus => C::LengthDown,
            KeyCode::Equal => C::LengthUp,
            KeyCode::KeyQ => C::Quit,
            _ => return None,
        }
    } else {
        match code {
            KeyCode::F1 => C::Shortcuts,
            KeyCode::F2 => C::TrackEditor,
            KeyCode::F3 => C::SongView,
            KeyCode::F4 => C::PianoRoll,
            KeyCode::F5 => C::Grid,
            KeyCode::F6 => C::SynthEditor,
            KeyCode::BracketLeft if shift => C::PreviousSong,
            KeyCode::BracketRight if shift => C::NextSong,
            KeyCode::BracketLeft | KeyCode::PageUp => C::PreviousPattern,
            KeyCode::BracketRight | KeyCode::PageDown => C::NextPattern,
            KeyCode::Comma => C::PreviousKit,
            KeyCode::Period => C::NextKit,
            KeyCode::KeyB => C::GeneratePattern,
            KeyCode::KeyD => C::GenerateDrums,
            KeyCode::KeyS => C::GenerateBass,
            KeyCode::KeyA => C::GenerateChords,
            KeyCode::KeyZ => C::TrackAuto,
            KeyCode::KeyX => C::RandomTrack,
            _ => return None,
        }
    })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title(TITLE).with_inner_size(INITIAL_SIZE);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(e) => return self.fail(event_loop, "create window", e),
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(context) => context,
            Err(e) => return self.fail(event_loop, "create graphics context", e),
        };
        match softbuffer::Surface::new(&context, window.clone()) {
            Ok(surface) => self.gfx = Some(Gfx { window, surface }),
            Err(e) => self.fail(event_loop, "create surface", e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.quit(event_loop),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, "redraw", e);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.request_redraw(),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = position;
                self.hover();
                let size = (self.size().0, self.size().1, self.scale());
                let shift = self.modifiers.shift_key();
                let (x, y) = (position.x as i32, position.y as i32);
                let moved = self.session.mouse_move(size, x, y, shift);
                let hovered = self.session.grid_hover(size, x, y)
                    | self.session.song_hover(size, x, y)
                    | self.session.roll_hover(size, x, y);
                if hovered || moved {
                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.session.overlay_open() => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.round() as isize,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 20.0).round() as isize,
                };
                if lines != 0 {
                    self.session.overlay_key(OverlayKey::Move(-lines * 3));
                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.round() as isize,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 20.0).round() as isize,
                };
                let size = (self.size().0, self.size().1, self.scale());
                let (x, y) = (self.mouse.x as i32, self.mouse.y as i32);
                let fine = self.modifiers.shift_key() || self.modifiers.alt_key();
                let coarse = self.modifiers.control_key() || self.modifiers.super_key();
                let shift = self.modifiers.shift_key();
                if self.session.wheel(size, x, y, lines, fine, coarse)
                    || self.session.song_wheel(size, x, y, lines, shift)
                    || self.session.roll_wheel(size, x, y, lines, shift)
                {
                    self.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                self.session.close_overlay();
                self.session.drop_path(&path, self.modifiers.shift_key());
                self.request_redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                self.click(Button::Left);
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                self.click(Button::Right);
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                self.session.mouse_up();
            }
            _ => {}
        }
        if self.session.quit_requested() {
            self.quit(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let save_at = self.session.autosave();
        let playing = self.session.playing();
        if playing {
            self.session.update();
        }
        // While playing (the playhead) or while an LFO moves the synth editor.
        let animating = playing || self.session.synth_animating();
        if animating {
            self.request_redraw();
        }
        let wake = match (animating, save_at) {
            (true, _) => Some(Instant::now() + FRAME),
            (false, Some(at)) => Some(at),
            (false, None) => None,
        };
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

/// `ORDRUMBOX_SCREENSHOT`: the first frame written as a 24-bit BMP (no image
/// crate to depend on), from the `0x00RRGGBB` pixels of the frame buffer.
fn write_bmp(path: &str, pixels: &[u32], width: usize, height: usize) -> std::io::Result<()> {
    let pad = (4 - width * 3 % 4) % 4;
    let data_size = (width * 3 + pad) * height;
    let mut out = Vec::with_capacity(54 + data_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data_size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // one plane
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&[0; 4]); // BI_RGB, no compression
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // 72 dpi
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    for y in (0..height).rev() {
        for x in 0..width {
            let c = pixels[y * width + x];
            out.extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]);
        }
        out.extend(std::iter::repeat_n(0u8, pad));
    }
    std::fs::write(path, out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut session = Session::load();
    // MIDI files given on the command line ("open with"): imported.
    for arg in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(arg);
        let is_midi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"));
        if is_midi {
            session.import_midi(&path);
        }
    }
    // `ORDRUMBOX_VIEW=roll|song|synth|kit|comp|flt`: which view the window
    // starts on (with `ORDRUMBOX_SCREENSHOT`, to capture that layout).
    match std::env::var("ORDRUMBOX_VIEW").as_deref() {
        Ok("roll") => session.toggle_piano_roll(),
        Ok("song") => session.toggle_song_view(),
        Ok("synth") => session.toggle_synth(),
        Ok("kit") => session.toggle_kit_editor(),
        Ok("comp") => session.output_activate(panel::Id::OutTab(1)),
        Ok("flt") => session.output_activate(panel::Id::OutTab(2)),
        Ok("reset") => session.run(Command::FactoryReset),
        _ => {}
    }
    let mut app = App {
        gfx: None,
        session,
        modifiers: ModifiersState::default(),
        mouse: PhysicalPosition::new(0.0, 0.0),
        error: None,
        alt_alone: false,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_bmp_writes_24_bit_bottom_up_rows() {
        let path = std::env::temp_dir().join(format!("ordrumbox_{}.bmp", std::process::id()));
        // One row, two pixels: red then green, the row padded to four bytes.
        write_bmp(path.to_str().unwrap(), &[0xff0000, 0x00ff00], 2, 1).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bytes[2..6].try_into().unwrap()), 54 + 8);
        assert_eq!(&bytes[54..], &[0, 0, 255, 0, 255, 0, 0, 0]); // BGR, BGR, pad
    }
}
