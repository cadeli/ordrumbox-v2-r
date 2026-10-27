//! orDrumbox native application.
//!
//! Loads the ordrumbox-v2 assets (patterns, kits, samples, synth presets),
//! plays the selected pattern through the default audio device (per-track
//! strips with filter, saturation, reverb, delay and LFOs, synth tracks, master
//! bus), and shows its tracks with a playhead. Keys: Space play/stop, ←/→ pattern, ↑/↓ tempo (Shift: ×10),
//! K next kit.

// No console window in a release build on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod audio;
mod canvas;
mod font;
mod view;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordrumbox_core::{Library, Pattern};
use ordrumbox_engine::{Controller, Kit};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use canvas::Canvas;
use view::View;

const TITLE: &str = "orDrumbox";
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1024.0, 640.0);
/// Redraw interval while playing (the playhead moves).
const FRAME: Duration = Duration::from_millis(16);

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

/// Everything loaded, and the transport.
struct Session {
    library: Library,
    kits: Vec<Kit>,
    kit_index: usize,
    pattern_index: usize,
    /// `None` when no audio device could be opened.
    controller: Option<Controller>,
    _output: Option<audio::Output>,
    status: Vec<String>,
}

impl Session {
    fn load() -> Self {
        let Some(dir) = assets::find() else {
            let searched: Vec<String> = assets::candidates().iter().map(|d| d.display().to_string()).collect();
            return Self::empty(vec![
                format!(
                    "assets folder not found: copy ordrumbox-v2/assets next to the program, or set {}",
                    assets::ENV_VAR
                ),
                format!("searched: {}", searched.join(", ")),
            ]);
        };
        let assets = match assets::load(&dir) {
            Ok(assets) => assets,
            Err(e) => return Self::empty(vec![e]),
        };
        let mut status = Vec::new();
        if !assets.report.issues.is_empty() {
            status.push(format!(
                "song.json: {} value(s) repaired, first: {}",
                assets.report.issues.len(),
                assets.report.issues[0].at
            ));
        }
        if let Some(first) = assets.sample_errors.first() {
            status.push(format!("{} sample(s) not loaded, first: {first}", assets.sample_errors.len()));
        }
        let (controller, output) = match audio::start(Arc::new(assets.bank), assets.scales, assets.synths) {
            Ok((output, controller)) => (Some(controller), Some(output)),
            Err(e) => {
                status.push(format!("no sound: {e}"));
                (None, None)
            }
        };
        Self {
            library: assets.library,
            kits: assets.kits,
            kit_index: 0,
            pattern_index: 0,
            controller,
            _output: output,
            status,
        }
    }

    fn empty(status: Vec<String>) -> Self {
        Self {
            library: Library::default(),
            kits: Vec::new(),
            kit_index: 0,
            pattern_index: 0,
            controller: None,
            _output: None,
            status,
        }
    }

    fn pattern(&self) -> Option<&Pattern> {
        self.library.patterns.get(self.pattern_index)
    }

    fn playing(&self) -> bool {
        self.controller.as_ref().is_some_and(Controller::is_playing)
    }

    fn toggle_play(&mut self) {
        let Some(pattern) = self.library.patterns.get(self.pattern_index) else { return };
        let Some(controller) = &mut self.controller else { return };
        if controller.is_playing() { controller.stop() } else { controller.play(pattern) }
    }

    fn step_pattern(&mut self, delta: isize) {
        let count = self.library.patterns.len();
        if count > 0 {
            self.pattern_index = (self.pattern_index as isize + delta).rem_euclid(count as isize) as usize;
        }
    }

    fn step_tempo(&mut self, delta: f64) {
        if let Some(pattern) = self.library.patterns.get_mut(self.pattern_index) {
            pattern.bpm = (pattern.bpm + delta).round().clamp(20.0, 300.0);
        }
    }

    fn next_kit(&mut self) {
        if self.kits.is_empty() {
            return;
        }
        self.kit_index = (self.kit_index + 1) % self.kits.len();
        if let Some(controller) = &mut self.controller {
            controller.set_kit(&self.kits[self.kit_index].name);
        }
    }

    /// Keep the engine fed; call every frame while playing.
    fn update(&mut self) {
        if let (Some(controller), Some(pattern)) = (&mut self.controller, self.library.patterns.get(self.pattern_index))
        {
            controller.update(pattern);
        }
    }

    fn draw(&self, canvas: &mut Canvas, scale: i32) {
        let position = self.controller.as_ref().and_then(Controller::position);
        let view = View {
            pattern: self.pattern(),
            pattern_index: self.pattern_index,
            pattern_count: self.library.patterns.len(),
            kit: self.kits.get(self.kit_index).map_or("-", |k| k.name.as_str()),
            playing: self.playing(),
            playhead: position.map(|p| p.frame as f64 / p.frames.max(1) as f64),
            status: &self.status,
        };
        view::draw(canvas, &view, scale);
    }
}

struct App {
    gfx: Option<Gfx>,
    session: Session,
    modifiers: ModifiersState,
    error: Option<String>,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, context: &str, error: impl std::fmt::Display) {
        self.error = Some(format!("{context}: {error}"));
        event_loop.exit();
    }

    fn redraw(&mut self) -> Result<(), String> {
        let Some(gfx) = &mut self.gfx else { return Ok(()) };
        let size = gfx.window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // minimised
        };
        let scale = gfx.window.scale_factor().round().max(1.0) as i32;
        gfx.surface.resize(width, height).map_err(|e| format!("resize surface: {e}"))?;
        let mut buffer = gfx.surface.buffer_mut().map_err(|e| format!("get frame buffer: {e}"))?;
        self.session.draw(&mut Canvas::new(&mut buffer, width.get() as usize, height.get() as usize), scale);
        buffer.present().map_err(|e| format!("present frame: {e}"))
    }

    fn key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let tempo_step = if self.modifiers.shift_key() { 10.0 } else { 1.0 };
        match &event.logical_key {
            Key::Named(NamedKey::Space) if !event.repeat => self.session.toggle_play(),
            Key::Named(NamedKey::ArrowLeft) => self.session.step_pattern(-1),
            Key::Named(NamedKey::ArrowRight) => self.session.step_pattern(1),
            Key::Named(NamedKey::ArrowUp) => self.session.step_tempo(tempo_step),
            Key::Named(NamedKey::ArrowDown) => self.session.step_tempo(-tempo_step),
            Key::Character(c) if c.eq_ignore_ascii_case("k") && !event.repeat => self.session.next_kit(),
            _ => return,
        }
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, "redraw", e);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(gfx) = &self.gfx {
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(&event),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.playing() {
            self.session.update();
            if let Some(gfx) = &self.gfx {
                gfx.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = App { gfx: None, session: Session::load(), modifiers: ModifiersState::default(), error: None };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}
