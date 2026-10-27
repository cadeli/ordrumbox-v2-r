//! orDrumbox native application.
//!
//! Phase 0: a window drawn by the software canvas (a 16-step row grouped by
//! beat), proving the winit + softbuffer bridge builds and runs on Windows.

// No console window in a release build on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod canvas;

use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use canvas::{Canvas, Rgb};

const TITLE: &str = "orDrumbox";
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1024.0, 640.0);

const BACKGROUND: Rgb = 0x0016_1619;
const STEP_ON_BEAT: Rgb = 0x00F2_A93B;
const STEP_OFF_BEAT: Rgb = 0x0035_3540;
const STEP_COUNT: i32 = 16;
const STEPS_PER_BEAT: i32 = 4;

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

#[derive(Default)]
struct App {
    gfx: Option<Gfx>,
    error: Option<String>,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, context: &str, error: impl std::fmt::Display) {
        self.error = Some(format!("{context}: {error}"));
        event_loop.exit();
    }
}

impl Gfx {
    /// Draw one frame at the window's current size.
    fn redraw(&mut self) -> Result<(), String> {
        let size = self.window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // minimised
        };
        self.surface.resize(width, height).map_err(|e| format!("resize surface: {e}"))?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| format!("get frame buffer: {e}"))?;
        draw(&mut Canvas::new(&mut buffer, width.get() as usize, height.get() as usize));
        buffer.present().map_err(|e| format!("present frame: {e}"))
    }
}

/// Placeholder frame: one row of steps, beats set apart by a wider gap.
fn draw(canvas: &mut Canvas) {
    canvas.clear(BACKGROUND);
    let cell = (canvas.width() as i32 / (STEP_COUNT + 4)).clamp(8, 64);
    let gap = (cell / 8).max(1);
    let beat_gap = gap * 4;
    let row_width = STEP_COUNT * cell + (STEP_COUNT - 1) * gap + (STEP_COUNT / STEPS_PER_BEAT - 1) * (beat_gap - gap);
    let mut x = (canvas.width() as i32 - row_width) / 2;
    let y = (canvas.height() as i32 - cell) / 2;
    for step in 0..STEP_COUNT {
        let color = if step % STEPS_PER_BEAT == 0 { STEP_ON_BEAT } else { STEP_OFF_BEAT };
        canvas.fill_rect(x, y, cell, cell, color);
        x += cell + if (step + 1) % STEPS_PER_BEAT == 0 { beat_gap } else { gap };
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
                if let Some(Err(e)) = self.gfx.as_mut().map(Gfx::redraw) {
                    self.fail(event_loop, "redraw", e);
                }
            }
            WindowEvent::Resized(_) => {
                if let Some(gfx) = &self.gfx {
                    gfx.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = App::default();
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}
