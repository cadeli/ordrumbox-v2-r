//! orDrumbox native application.
//!
//! Loads the ordrumbox-v2 assets (patterns, kits, samples, synth presets) and
//! the user's library, plays the selected pattern through the default audio
//! device (per-track strips, synth tracks, master bus), and edits it with the
//! menu bar, the mouse and the keyboard (`ordrumbox-ui`: `menu`, `view::HELP`), every
//! edit undoable and saved a moment later.

// No console window in a release build on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;

use std::num::NonZeroU32;
use std::rc::Rc;

use ordrumbox_ui::ui_input::{Button, Key, KeyEvent, Modifiers, ScrollDelta, single_char};
use ordrumbox_ui::{Command, Scale, Session, Ui};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopClosed};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Fullscreen, Window, WindowId};

const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1100.0, 680.0);

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct App {
    gfx: Option<Gfx>,
    ui: Ui,
    error: Option<String>,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, context: &str, error: impl std::fmt::Display) {
        self.error = Some(format!("{context}: {error}"));
        event_loop.exit();
    }

    fn request_redraw(&self) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    /// Tell the UI the window's current size and scale factor.
    fn sync_size(&mut self) {
        if let Some(gfx) = &self.gfx {
            let size = gfx.window.inner_size();
            let scale = Scale::from_factor(gfx.window.scale_factor());
            self.ui.set_size(size.width as i32, size.height as i32, scale);
        }
    }

    #[expect(clippy::print_stderr, reason = "the screenshot mode is run from a terminal, which reads it")]
    fn redraw(&mut self) -> Result<(), String> {
        self.sync_size();
        let Some(gfx) = &mut self.gfx else { return Ok(()) };
        let size = gfx.window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // minimised
        };
        gfx.surface.resize(width, height).map_err(|e| format!("resize surface: {e}"))?;
        let mut buffer = gfx.surface.buffer_mut().map_err(|e| format!("get frame buffer: {e}"))?;
        self.ui.draw(&mut buffer, width.get() as usize, height.get() as usize);
        // `ORDRUMBOX_SCREENSHOT=path`: keep the first frame as a BMP there and
        // quit — to check the layout without a screen capture.
        // A failed write exits with an error: whoever asked for the file reads it next.
        if let Ok(path) = std::env::var("ORDRUMBOX_SCREENSHOT") {
            if let Err(e) = write_bmp(&path, &buffer, width.get() as usize, height.get() as usize) {
                eprintln!("{path}: {e}");
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        buffer.present().map_err(|e| format!("present frame: {e}"))
    }

    /// Finish: save, then leave.
    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        self.ui.session.save_if_changed();
        event_loop.exit();
    }
}

/// The UI's name for a physical key.
fn key(code: KeyCode) -> Key {
    use KeyCode as K;
    match code {
        K::KeyA => Key::Letter('A'),
        K::KeyB => Key::Letter('B'),
        K::KeyC => Key::Letter('C'),
        K::KeyD => Key::Letter('D'),
        K::KeyE => Key::Letter('E'),
        K::KeyF => Key::Letter('F'),
        K::KeyG => Key::Letter('G'),
        K::KeyH => Key::Letter('H'),
        K::KeyI => Key::Letter('I'),
        K::KeyJ => Key::Letter('J'),
        K::KeyK => Key::Letter('K'),
        K::KeyL => Key::Letter('L'),
        K::KeyM => Key::Letter('M'),
        K::KeyN => Key::Letter('N'),
        K::KeyO => Key::Letter('O'),
        K::KeyP => Key::Letter('P'),
        K::KeyQ => Key::Letter('Q'),
        K::KeyR => Key::Letter('R'),
        K::KeyS => Key::Letter('S'),
        K::KeyT => Key::Letter('T'),
        K::KeyU => Key::Letter('U'),
        K::KeyV => Key::Letter('V'),
        K::KeyW => Key::Letter('W'),
        K::KeyX => Key::Letter('X'),
        K::KeyY => Key::Letter('Y'),
        K::KeyZ => Key::Letter('Z'),
        K::Digit0 => Key::Digit(0),
        K::Digit1 => Key::Digit(1),
        K::Digit2 => Key::Digit(2),
        K::Digit3 => Key::Digit(3),
        K::Digit4 => Key::Digit(4),
        K::Digit5 => Key::Digit(5),
        K::Digit6 => Key::Digit(6),
        K::Digit7 => Key::Digit(7),
        K::Digit8 => Key::Digit(8),
        K::Digit9 => Key::Digit(9),
        K::F1 => Key::F(1),
        K::F2 => Key::F(2),
        K::F3 => Key::F(3),
        K::F4 => Key::F(4),
        K::F5 => Key::F(5),
        K::F6 => Key::F(6),
        K::F7 => Key::F(7),
        K::F8 => Key::F(8),
        K::F9 => Key::F(9),
        K::F10 => Key::F(10),
        K::F11 => Key::F(11),
        K::F12 => Key::F(12),
        K::AltLeft | K::AltRight => Key::Alt,
        K::Escape => Key::Escape,
        K::Enter => Key::Enter,
        K::NumpadEnter => Key::NumpadEnter,
        K::Space => Key::Space,
        K::Tab => Key::Tab,
        K::Backspace => Key::Backspace,
        K::Delete => Key::Delete,
        K::ArrowUp => Key::ArrowUp,
        K::ArrowDown => Key::ArrowDown,
        K::ArrowLeft => Key::ArrowLeft,
        K::ArrowRight => Key::ArrowRight,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
        K::Home => Key::Home,
        K::End => Key::End,
        K::Minus => Key::Minus,
        K::Equal => Key::Equal,
        K::NumpadSubtract => Key::NumpadSubtract,
        K::NumpadAdd => Key::NumpadAdd,
        K::BracketLeft => Key::BracketLeft,
        K::BracketRight => Key::BracketRight,
        K::Comma => Key::Comma,
        K::Period => Key::Period,
        K::Numpad0
        | K::Numpad1
        | K::Numpad2
        | K::Numpad3
        | K::Numpad4
        | K::Numpad5
        | K::Numpad6
        | K::Numpad7
        | K::Numpad8
        | K::Numpad9
        | K::NumpadDecimal
        | K::NumpadComma
        | K::NumpadDivide
        | K::NumpadMultiply
        | K::NumpadEqual => Key::Numpad,
        _ => Key::Other,
    }
}

fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers { shift: state.shift_key(), control: state.control_key(), alt: state.alt_key(), logo: state.super_key() }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        // The first window holds the layout at scale 1, in logical pixels: on
        // a HiDPI display the UI's scale and the window grow together. A
        // smaller window scrolls.
        let (layout_width, layout_height) = ordrumbox_ui::min_window_size(Scale::ONE, ordrumbox_ui::Shown::all(true));
        let initial = LogicalSize::new(
            INITIAL_SIZE.width.max(f64::from(layout_width)),
            INITIAL_SIZE.height.max(f64::from(layout_height)),
        );
        let (min_width, min_height) = ordrumbox_ui::MIN_WINDOW;
        let min_size = LogicalSize::new(f64::from(min_width), f64::from(min_height));
        let attributes = Window::default_attributes()
            .with_title(format!("orDrumbox {}", ordrumbox_ui::BUILD))
            .with_min_inner_size(min_size)
            .with_inner_size(initial);
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
        self.sync_size();
        let redraw = match event {
            WindowEvent::CloseRequested => {
                self.quit(event_loop);
                false
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, "redraw", e);
                }
                false
            }
            WindowEvent::Resized(_) => {
                // Full screen left by the system's own means: the menu's check follows.
                if let Some(gfx) = &self.gfx {
                    self.ui.session.set_fullscreen(gfx.window.fullscreen().is_some());
                }
                true
            }
            WindowEvent::ScaleFactorChanged { .. } => true,
            WindowEvent::ModifiersChanged(state) => {
                self.ui.set_modifiers(modifiers(state.state()));
                false
            }
            WindowEvent::KeyboardInput { event, .. } => match event.physical_key {
                PhysicalKey::Code(code) => self.ui.key(&KeyEvent {
                    key: key(code),
                    pressed: event.state == ElementState::Pressed,
                    repeat: event.repeat,
                    unmodified: event.key_without_modifiers().to_text().and_then(single_char),
                    text: event.text.map(|t| t.to_string()),
                }),
                PhysicalKey::Unidentified(_) => false,
            },
            WindowEvent::CursorMoved { position, .. } => self.ui.pointer_moved(position.x, position.y),
            WindowEvent::MouseWheel { delta, .. } => self.ui.wheel(match delta {
                MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines(x, y),
                MouseScrollDelta::PixelDelta(p) => ScrollDelta::Pixels(p.x, p.y),
            }),
            WindowEvent::DroppedFile(path) => self.ui.drop_path(&path),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Button::Left,
                    MouseButton::Right => Button::Right,
                    _ => return self.after_event(event_loop),
                };
                match state {
                    ElementState::Pressed => self.ui.pointer_pressed(button),
                    ElementState::Released => self.ui.pointer_released(button),
                }
            }
            _ => false,
        };
        if redraw {
            self.request_redraw();
        }
        self.after_event(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let idle = self.ui.idle();
        if idle.redraw {
            self.request_redraw();
        }
        event_loop.set_control_flow(idle.wake_at.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

impl App {
    /// After each window event: a quit asked from the menus or the keyboard.
    fn after_event(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(on) = self.ui.session.take_fullscreen_request()
            && let Some(gfx) = &self.gfx
        {
            gfx.window.set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
            self.request_redraw();
        }
        if self.ui.session.quit_requested() {
            self.quit(event_loop);
        }
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
    // `ordrumbox --mcp`, started by an MCP client: the bridge to the window.
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "--mcp") {
        return Ok(ordrumbox_ui::run_mcp_bridge()?);
    }
    // `ordrumbox --version`: which build this is.
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "--version") {
        println!("orDrumbox {}", ordrumbox_ui::BUILD);
        return Ok(());
    }
    // A failure from here on keeps the window from opening: told where the
    // user sees it, not on stderr only.
    open_window().map_err(|e| ordrumbox_ui::report_startup_error(&e.to_string()).into())
}

/// The window, until the user quits it.
fn open_window() -> Result<(), Box<dyn std::error::Error>> {
    // No text without it: stop with where it was looked for.
    ordrumbox_ui::load_font()?;
    let event_loop = EventLoop::new()?;
    let mut session = Session::load(Box::new(audio::start));
    let proxy = event_loop.create_proxy();
    let waker = ordrumbox_ui::Waker::new(move || match proxy.send_event(()) {
        // Closed: the app is quitting, there is nothing to wake.
        Ok(()) | Err(EventLoopClosed(())) => {}
    });
    session.offer_devices(audio::devices);
    session.start_updates(waker.clone());
    session.start_mcp(waker);
    // MIDI files given on the command line ("open with"): imported.
    for arg in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(arg);
        let is_midi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"));
        if is_midi {
            session.import_midi(&path);
        }
    }
    // `ORDRUMBOX_VIEW=roll|song|synth|kit|gen`: which view the window
    // starts on (with `ORDRUMBOX_SCREENSHOT`, to capture that layout).
    match std::env::var("ORDRUMBOX_VIEW").as_deref() {
        Ok("roll") => session.toggle_piano_roll(),
        Ok("song") => session.toggle_song_view(),
        Ok("synth") => session.toggle_synth(),
        Ok("kit") => session.toggle_kit_editor(),
        Ok("gen") => session.show_generation_tab(),
        Ok("reset") => session.run(Command::FactoryReset),
        _ => {}
    }
    let mut app = App { gfx: None, ui: Ui::new(session), error: None };
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
        std::fs::remove_file(&path).unwrap();
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bytes[2..6].try_into().unwrap()), 54 + 8);
        assert_eq!(&bytes[54..], &[0, 0, 255, 0, 255, 0, 0, 0]); // BGR, BGR, pad
    }
}
