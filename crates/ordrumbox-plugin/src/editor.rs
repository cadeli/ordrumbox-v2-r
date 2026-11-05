//! The plugin's window: a baseview child of the host's window showing the
//! app's UI, drawn in software and shown through softbuffer, its input
//! translated into the UI's.

use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr::NonNull;
use std::sync::Arc;

use baseview::{
    DropData, DropEffect, Event, EventStatus, MouseButton, MouseEvent, ScrollDelta, WindowEvent, WindowHandle,
    WindowOpenOptions, WindowScalePolicy,
};
use keyboard_types::{KeyState, KeyboardEvent};
use nih_plug::nih_log;
use nih_plug::prelude::{Editor, GuiContext, ParentWindowHandle};
use ordrumbox_ui::ui_input::{self, Button, Key, KeyEvent, Modifiers};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle};
use raw_window_handle_06 as rwh06;

use crate::player::{SharedPlayer, lock};

/// The window's size in logical pixels (the app's first window size).
pub const SIZE: (u32, u32) = (1100, 680);

/// The window's colour where the UI cannot draw (no font, no session).
const BACKGROUND: u32 = 0x0018_1820;

/// Opens the window when the host asks.
#[derive(Debug)]
pub struct OrdrumboxEditor {
    pub player: SharedPlayer,
}

impl Editor for OrdrumboxEditor {
    fn spawn(&self, parent: ParentWindowHandle, _context: Arc<dyn GuiContext>) -> Box<dyn std::any::Any + Send> {
        let player = self.player.clone();
        let window = baseview::Window::open_parented(
            &HostWindow(parent),
            WindowOpenOptions {
                title: "orDrumbox".into(),
                size: baseview::Size::new(f64::from(SIZE.0), f64::from(SIZE.1)),
                scale: WindowScalePolicy::SystemScaleFactor,
            },
            move |window: &mut baseview::Window<'_>| View::new(window, player),
        );
        Box::new(Open { window })
    }

    fn size(&self) -> (u32, u32) {
        SIZE
    }

    fn set_scale_factor(&self, _factor: f32) -> bool {
        // baseview takes the system's scale factor.
        false
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    fn param_values_changed(&self) {}
}

/// The open window; the host drops it to close the window.
struct Open {
    window: WindowHandle,
}

// SAFETY: the host creates and drops the editor on its GUI thread, the
// thread the window was opened on; this handle is only moved, never used, in
// between. nih-plug requires `Send` for every editor handle.
#[allow(unsafe_code, reason = "a window handle the host keeps on its GUI thread")]
unsafe impl Send for Open {}

impl Drop for Open {
    fn drop(&mut self) {
        self.window.close();
    }
}

/// What the window shows: the player's UI, drawn into a softbuffer surface.
struct View {
    player: SharedPlayer,
    /// `None` when the surface could not be made or used: nothing is drawn.
    surface: Option<Surface>,
    /// The system's monospace font is loaded: the UI can draw text.
    font: bool,
    /// Physical pixels.
    width: u32,
    height: u32,
    /// Physical pixels per logical one.
    scale: f64,
    /// Something changed since the last frame drawn.
    dirty: bool,
}

/// A softbuffer surface and the context it needs alive.
struct Surface {
    _context: softbuffer::Context<Target>,
    surface: softbuffer::Surface<Target, Target>,
}

impl View {
    fn new(window: &mut baseview::Window<'_>, player: SharedPlayer) -> Self {
        let font = match ordrumbox_ui::load_font() {
            Ok(()) => true,
            Err(e) => {
                nih_log!("orDrumbox: no font, the window stays empty: {e}");
                false
            }
        };
        let target = Target::of(window);
        let surface = softbuffer::Context::new(target.clone()).and_then(|context| {
            softbuffer::Surface::new(&context, target).map(|surface| Surface { _context: context, surface })
        });
        let mut view = Self { player, surface: None, font, width: 0, height: 0, scale: 1.0, dirty: true };
        match surface {
            Ok(surface) => view.surface = Some(surface),
            Err(e) => view.fail(&e),
        }
        view
    }

    fn resize(&mut self, width: u32, height: u32, scale: f64) {
        let (Some(w), Some(h), Some(surface)) = (NonZeroU32::new(width), NonZeroU32::new(height), &mut self.surface)
        else {
            return;
        };
        match surface.surface.resize(w, h) {
            Ok(()) => (self.width, self.height, self.scale, self.dirty) = (width, height, scale, true),
            Err(e) => self.fail(&e),
        }
    }

    /// One frame: the UI's idle pass, then a redraw if anything changed.
    fn frame(&mut self) -> Result<(), softbuffer::SoftBufferError> {
        let Some(surface) = &mut self.surface else { return Ok(()) };
        if self.width == 0 {
            return Ok(());
        }
        let mut player = lock(&self.player);
        let host_changed = player.take_redraw();
        let ui = player.ui().filter(|_| self.font);
        let redraw = match ui {
            Some(ui) => ui.idle().redraw || host_changed,
            None => false,
        };
        if !(redraw || self.dirty) {
            return Ok(());
        }
        let (width, height) = (self.width as usize, self.height as usize);
        let mut buffer = surface.surface.buffer_mut()?;
        match player.ui().filter(|_| self.font) {
            Some(ui) => {
                ui.set_size(width as i32, height as i32, self.scale.round().max(1.0) as i32);
                ui.draw(&mut buffer, width, height);
            }
            None => buffer.fill(BACKGROUND),
        }
        self.dirty = false;
        buffer.present()
    }

    /// The surface is given up; the window stays empty.
    fn fail(&mut self, error: &softbuffer::SoftBufferError) {
        nih_log!("orDrumbox: the window cannot be drawn: {error}");
        self.surface = None;
    }

    /// `f` on the UI, if there is one; its answer marks the window to redraw.
    fn with_ui(&mut self, f: impl FnOnce(&mut ordrumbox_ui::Ui) -> bool) {
        if let Some(ui) = lock(&self.player).ui() {
            self.dirty |= f(ui);
        }
    }

    fn mouse(&mut self, event: MouseEvent) -> EventStatus {
        let scale = self.scale;
        match event {
            MouseEvent::CursorMoved { position, modifiers } => self.with_ui(|ui| {
                ui.set_modifiers(ui_modifiers(modifiers));
                ui.pointer_moved(position.x * scale, position.y * scale)
            }),
            MouseEvent::ButtonPressed { button, modifiers } => {
                let Some(button) = ui_button(button) else { return EventStatus::Ignored };
                self.with_ui(|ui| {
                    ui.set_modifiers(ui_modifiers(modifiers));
                    ui.pointer_pressed(button)
                });
            }
            MouseEvent::ButtonReleased { button, modifiers } => {
                let Some(button) = ui_button(button) else { return EventStatus::Ignored };
                self.with_ui(|ui| {
                    ui.set_modifiers(ui_modifiers(modifiers));
                    ui.pointer_released(button)
                });
            }
            MouseEvent::WheelScrolled { delta, modifiers } => self.with_ui(|ui| {
                ui.set_modifiers(ui_modifiers(modifiers));
                ui.wheel(match delta {
                    ScrollDelta::Lines { x, y } => ui_input::ScrollDelta::Lines(x, y),
                    ScrollDelta::Pixels { x, y } => {
                        ui_input::ScrollDelta::Pixels(f64::from(x) * scale, f64::from(y) * scale)
                    }
                })
            }),
            MouseEvent::DragEntered { data: DropData::Files(_), .. }
            | MouseEvent::DragMoved { data: DropData::Files(_), .. } => {
                return EventStatus::AcceptDrop(DropEffect::Copy);
            }
            MouseEvent::DragDropped { data: DropData::Files(paths), .. } => {
                self.with_ui(|ui| paths.iter().fold(false, |redraw, path| ui.drop_path(path) | redraw));
            }
            _ => return EventStatus::Ignored,
        }
        EventStatus::Captured
    }

    /// A key for the UI. A key it does not take, and every release, go on to
    /// the host (its shortcuts, its transport, its computer keyboard): a
    /// release kept from it would leave its key held.
    fn key(&mut self, event: &KeyboardEvent) -> EventStatus {
        let key = KeyEvent {
            key: Key::from_code(&event.code.to_string()),
            pressed: event.state == KeyState::Down,
            repeat: event.repeat,
            text: match &event.key {
                keyboard_types::Key::Character(text) => Some(text.clone()),
                _ => None,
            },
        };
        let pressed = key.pressed;
        let mut taken = false;
        self.with_ui(|ui| {
            ui.set_modifiers(ui_modifiers(event.modifiers));
            taken = ui.key(&key);
            taken
        });
        if taken && pressed { EventStatus::Captured } else { EventStatus::Ignored }
    }
}

fn ui_modifiers(modifiers: keyboard_types::Modifiers) -> Modifiers {
    use keyboard_types::Modifiers as M;
    Modifiers {
        shift: modifiers.contains(M::SHIFT),
        control: modifiers.contains(M::CONTROL),
        alt: modifiers.contains(M::ALT),
        logo: modifiers.contains(M::META),
    }
}

fn ui_button(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Right => Some(Button::Right),
        _ => None,
    }
}

impl baseview::WindowHandler for View {
    fn on_frame(&mut self, _window: &mut baseview::Window<'_>) {
        if let Err(e) = self.frame() {
            self.fail(&e);
        }
    }

    fn on_event(&mut self, _window: &mut baseview::Window<'_>, event: Event) -> EventStatus {
        match event {
            Event::Window(WindowEvent::Resized(info)) => {
                let size = info.physical_size();
                self.resize(size.width, size.height, info.scale());
                EventStatus::Captured
            }
            Event::Window(_) => EventStatus::Ignored,
            Event::Mouse(event) => self.mouse(event),
            Event::Keyboard(event) => self.key(&event),
        }
    }
}

/// The host's window, for baseview, which takes raw-window-handle 0.5.
struct HostWindow(ParentWindowHandle);

// SAFETY: the handle is the host's window, valid while the editor is open,
// and of the kind of this platform.
#[allow(unsafe_code, reason = "raw-window-handle 0.5 makes this trait unsafe")]
unsafe impl HasRawWindowHandle for HostWindow {
    fn raw_window_handle(&self) -> RawWindowHandle {
        match self.0 {
            ParentWindowHandle::X11Window(window) => {
                let mut handle = raw_window_handle::XcbWindowHandle::empty();
                handle.window = window;
                RawWindowHandle::Xcb(handle)
            }
            ParentWindowHandle::AppKitNsView(ns_view) => {
                let mut handle = raw_window_handle::AppKitWindowHandle::empty();
                handle.ns_view = ns_view;
                RawWindowHandle::AppKit(handle)
            }
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                let mut handle = raw_window_handle::Win32WindowHandle::empty();
                handle.hwnd = hwnd;
                RawWindowHandle::Win32(handle)
            }
        }
    }
}

/// baseview's window, for softbuffer, which takes raw-window-handle 0.6.
#[derive(Clone)]
struct Target {
    display: rwh06::RawDisplayHandle,
    /// `None`: baseview gave a handle of no kind softbuffer draws in.
    window: Option<rwh06::RawWindowHandle>,
}

impl Target {
    fn of(window: &baseview::Window<'_>) -> Self {
        let display = match window.raw_display_handle() {
            RawDisplayHandle::Xlib(h) => {
                rwh06::RawDisplayHandle::Xlib(rwh06::XlibDisplayHandle::new(NonNull::new(h.display), h.screen))
            }
            RawDisplayHandle::Xcb(h) => {
                rwh06::RawDisplayHandle::Xcb(rwh06::XcbDisplayHandle::new(NonNull::new(h.connection), h.screen))
            }
            RawDisplayHandle::Windows(_) => rwh06::RawDisplayHandle::Windows(rwh06::WindowsDisplayHandle::new()),
            // AppKit, and anything else baseview does not open.
            _ => rwh06::RawDisplayHandle::AppKit(rwh06::AppKitDisplayHandle::new()),
        };
        let window = match window.raw_window_handle() {
            RawWindowHandle::AppKit(h) => {
                NonNull::new(h.ns_view).map(|view| rwh06::RawWindowHandle::AppKit(rwh06::AppKitWindowHandle::new(view)))
            }
            RawWindowHandle::Xlib(h) => Some(rwh06::RawWindowHandle::Xlib(rwh06::XlibWindowHandle::new(h.window))),
            RawWindowHandle::Xcb(h) => {
                NonZeroU32::new(h.window).map(|w| rwh06::RawWindowHandle::Xcb(rwh06::XcbWindowHandle::new(w)))
            }
            RawWindowHandle::Win32(h) => NonZeroIsize::new(h.hwnd as isize).map(|hwnd| {
                let mut handle = rwh06::Win32WindowHandle::new(hwnd);
                handle.hinstance = NonZeroIsize::new(h.hinstance as isize);
                rwh06::RawWindowHandle::Win32(handle)
            }),
            _ => None,
        };
        Self { display, window }
    }
}

impl rwh06::HasDisplayHandle for Target {
    fn display_handle(&self) -> Result<rwh06::DisplayHandle<'_>, rwh06::HandleError> {
        // SAFETY: the display of baseview's window, which outlives the
        // surface: the view that owns both is dropped with the window.
        #[allow(unsafe_code, reason = "a raw handle borrowed for the window's lifetime")]
        Ok(unsafe { rwh06::DisplayHandle::borrow_raw(self.display) })
    }
}

impl rwh06::HasWindowHandle for Target {
    fn window_handle(&self) -> Result<rwh06::WindowHandle<'_>, rwh06::HandleError> {
        let window = self.window.ok_or(rwh06::HandleError::Unavailable)?;
        // SAFETY: as for the display: baseview's window outlives the surface.
        #[allow(unsafe_code, reason = "a raw handle borrowed for the window's lifetime")]
        Ok(unsafe { rwh06::WindowHandle::borrow_raw(window) })
    }
}
