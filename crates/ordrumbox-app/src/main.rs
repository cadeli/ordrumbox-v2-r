//! orDrumbox native application.
//!
//! Loads the ordrumbox-v2 assets (patterns, kits, samples, synth presets) and
//! the user's library, plays the selected pattern through the default audio
//! device (per-track strips, synth tracks, master bus), and edits it with the
//! mouse and the keyboard (see `view::HELP`), every edit undoable and saved a
//! moment later.

// No console window in a release build on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod audio;
mod canvas;
mod font;
mod session;
mod store;
mod view;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use canvas::Canvas;
use session::{Move, Session};
use view::Layout;

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
        self.session.draw(&mut Canvas::new(&mut buffer, width.get() as usize, height.get() as usize), scale);
        buffer.present().map_err(|e| format!("present frame: {e}"))
    }

    fn click(&mut self) {
        let Some(gfx) = &self.gfx else { return };
        let size = gfx.window.inner_size();
        let Some(pattern) = self.session.pattern() else { return };
        let layout =
            Layout::new(size.width as i32, size.height as i32, self.scale(), pattern, self.session.status_lines());
        if let Some(hit) = layout.hit(pattern, self.mouse.x as i32, self.mouse.y as i32) {
            self.session.click(hit);
            self.request_redraw();
        }
    }

    fn key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = event.physical_key else { return };
        let shift = self.modifiers.shift_key();
        let command = self.modifiers.control_key() || self.modifiers.super_key();
        let s = &mut self.session;
        if command {
            match code {
                KeyCode::KeyZ if shift => s.redo(),
                KeyCode::KeyZ => s.undo(),
                KeyCode::KeyY => s.redo(),
                KeyCode::KeyC => s.copy(shift),
                KeyCode::KeyV => s.paste(shift),
                KeyCode::KeyS => s.save(),
                KeyCode::KeyN => s.new_pattern(),
                KeyCode::KeyD => s.duplicate_pattern(),
                KeyCode::KeyT => s.add_track(),
                KeyCode::Backspace | KeyCode::Delete => s.remove_track(),
                KeyCode::KeyG => s.next_grid(),
                KeyCode::KeyL => s.loop_point_back(),
                KeyCode::Minus => s.change_length(-1),
                KeyCode::Equal => s.change_length(1),
                _ => return,
            }
        } else if let Some(row) = ROW_DIGITS.iter().position(|&k| k == code) {
            s.toggle_row(row, if shift { "solo" } else { "mute" });
        } else if let Some(row) = ROW_LETTERS.iter().position(|&k| k == code) {
            s.audition_row(row);
        } else {
            let tempo = if shift { 10.0 } else { 1.0 };
            match code {
                KeyCode::Space if !event.repeat => s.toggle_play(),
                KeyCode::BracketLeft | KeyCode::PageUp => s.step_pattern(-1),
                KeyCode::BracketRight | KeyCode::PageDown => s.step_pattern(1),
                KeyCode::Minus | KeyCode::NumpadSubtract => s.step_tempo(-tempo),
                KeyCode::Equal | KeyCode::NumpadAdd => s.step_tempo(tempo),
                KeyCode::Comma => s.step_kit(-1),
                KeyCode::Period => s.step_kit(1),
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
            WindowEvent::CloseRequested => {
                self.session.save_if_changed();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, "redraw", e);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.request_redraw(),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            WindowEvent::CursorMoved { position, .. } => self.mouse = position,
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => self.click(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let save_at = self.session.autosave();
        let playing = self.session.playing();
        if playing {
            self.session.update();
            self.request_redraw();
        }
        let wake = match (playing, save_at) {
            (true, _) => Some(Instant::now() + FRAME),
            (false, Some(at)) => Some(at),
            (false, None) => None,
        };
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        gfx: None,
        session: Session::load(),
        modifiers: ModifiersState::default(),
        mouse: PhysicalPosition::new(0.0, 0.0),
        error: None,
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}
