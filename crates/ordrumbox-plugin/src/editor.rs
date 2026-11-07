//! The plugin's window: a baseview child of the host's window showing the
//! app's UI, drawn in software and shown through softbuffer, its input
//! translated into the UI's. A pattern or kit chosen in it is given to the
//! host's parameters. A grip in its bottom-right corner resizes it: the host
//! is asked for the new size, which the project saves.

use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr::NonNull;
use std::sync::Arc;

use baseview::{
    DropData, DropEffect, Event, EventStatus, MouseButton, MouseEvent, ScrollDelta, WindowEvent, WindowHandle,
    WindowOpenOptions, WindowScalePolicy,
};
use keyboard_types::{KeyState, KeyboardEvent};
use nih_plug::nih_log;
use nih_plug::prelude::{Editor, GuiContext, IntParam, ParamSetter, ParentWindowHandle};
use ordrumbox_ui::Scale;
use ordrumbox_ui::ui_input::{self, Button, Key, KeyEvent, Modifiers};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle};
use raw_window_handle_06 as rwh06;

use crate::params::OrdrumboxParams;
use crate::player::{SharedPlayer, lock};

/// The window's first size in logical pixels (the app's first window size).
pub const SIZE: (u32, u32) = (1100, 680);

/// The largest side the grip makes, in logical pixels.
const MAX_SIDE: u32 = 8192;

/// The resize grip's side in logical pixels, in the window's bottom-right
/// corner, over the UI.
const GRIP: u32 = 14;

/// The grip's stripes.
const GRIP_COLOR: u32 = 0x0080_8090;

/// The window's colour where the UI cannot draw (no font, no session).
const BACKGROUND: u32 = 0x0018_1820;

/// Opens the window when the host asks.
#[derive(Debug)]
pub struct OrdrumboxEditor {
    pub player: SharedPlayer,
    pub params: Arc<OrdrumboxParams>,
}

impl Editor for OrdrumboxEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn std::any::Any + Send> {
        let host = Host { context, params: self.params.clone() };
        let player = self.player.clone();
        let window = baseview::Window::open_parented(
            &HostWindow(parent),
            WindowOpenOptions {
                title: "orDrumbox".into(),
                size: logical(self.size()),
                scale: WindowScalePolicy::SystemScaleFactor,
            },
            move |window: &mut baseview::Window<'_>| View::new(window, player, host),
        );
        Box::new(Open { window })
    }

    fn size(&self) -> (u32, u32) {
        fitted(self.params.editor_size.get())
    }

    fn set_scale_factor(&self, _factor: f32) -> bool {
        // baseview takes the system's scale factor.
        false
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    fn param_values_changed(&self) {}
}

/// `size` within [`ordrumbox_ui::MIN_WINDOW`] and [`MAX_SIDE`]: a project may
/// hold any size. The layout scrolls in a smaller window than it needs.
fn fitted((width, height): (u32, u32)) -> (u32, u32) {
    let (min_width, min_height) = ordrumbox_ui::MIN_WINDOW;
    (width.clamp(min_width, MAX_SIDE), height.clamp(min_height, MAX_SIDE))
}

fn logical((width, height): (u32, u32)) -> baseview::Size {
    baseview::Size::new(f64::from(width), f64::from(height))
}

/// The open window; the host drops it to close the window.
struct Open {
    window: WindowHandle,
}

// SAFETY: the host creates and drops the editor on its GUI thread, the
// thread the window was opened on; this handle is only moved, never used, in
// between. nih-plug requires `Send` for every editor handle.
#[expect(unsafe_code, reason = "a window handle the host keeps on its GUI thread")]
unsafe impl Send for Open {}

impl Drop for Open {
    fn drop(&mut self) {
        self.window.close();
    }
}

/// The host's parameters, set from the window.
struct Host {
    context: Arc<dyn GuiContext>,
    params: Arc<OrdrumboxParams>,
}

impl Host {
    /// `param` set to `value` (from 1), as one gesture; nothing when `None`.
    fn set(&self, param: &IntParam, value: Option<i32>) {
        let Some(value) = value else { return };
        let setter = ParamSetter::new(self.context.as_ref());
        setter.begin_set_parameter(param);
        setter.set_parameter(param, value);
        setter.end_set_parameter(param);
    }
}

/// What the window shows: the player's UI, drawn into a softbuffer surface.
struct View {
    player: SharedPlayer,
    host: Host,
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
    /// The pointer, in logical pixels.
    pointer: (f64, f64),
    /// The grip is being dragged.
    drag: Option<Drag>,
}

/// A drag of the resize grip: where it started, and the size then.
#[derive(Clone, Copy)]
struct Drag {
    pointer: (f64, f64),
    size: (u32, u32),
}

/// A softbuffer surface and the context it needs alive.
struct Surface {
    _context: softbuffer::Context<Target>,
    surface: softbuffer::Surface<Target, Target>,
}

impl View {
    fn new(window: &mut baseview::Window<'_>, player: SharedPlayer, host: Host) -> Self {
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
        let mut view = Self {
            player,
            host,
            surface: None,
            font,
            width: 0,
            height: 0,
            scale: 1.0,
            dirty: true,
            pointer: (0.0, 0.0),
            drag: None,
        };
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

    /// One frame: the choices made in the window given to the host, the
    /// UI's idle pass, then a redraw if anything changed.
    fn frame(&mut self) -> Result<(), softbuffer::SoftBufferError> {
        // Taken under the lock, given with it released: the host is called.
        let choices = lock(&self.player).take_window_choices();
        self.host.set(&self.host.params.pattern, choices.pattern);
        self.host.set(&self.host.params.kit, choices.kit);
        let grip = self.grip_side();
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
                ui.set_size(width as i32, height as i32, Scale::from_factor(self.scale));
                ui.draw(&mut buffer, width, height);
            }
            None => buffer.fill(BACKGROUND),
        }
        draw_grip(&mut buffer, width, height, grip);
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

    /// The grip's side in physical pixels.
    fn grip_side(&self) -> usize {
        // The scale of a display, a few units at most.
        GRIP as usize * self.scale.round().max(1.0) as usize
    }

    /// The pointer is on the grip.
    fn on_grip(&self) -> bool {
        let side = self.grip_side() as f64;
        let (x, y) = (self.pointer.0 * self.scale, self.pointer.1 * self.scale);
        x >= f64::from(self.width) - side && y >= f64::from(self.height) - side
    }

    /// The grip dragged to the pointer: the window takes the size the drag
    /// gives, if the host agrees.
    fn drag_to(&mut self, window: &mut baseview::Window<'_>, drag: Drag) {
        let side = |from: u32, moved: f64| (f64::from(from) + moved).round().clamp(0.0, f64::from(MAX_SIDE)) as u32;
        let size = fitted((
            side(drag.size.0, self.pointer.0 - drag.pointer.0),
            side(drag.size.1, self.pointer.1 - drag.pointer.1),
        ));
        self.ask_size(window, size);
    }

    /// The window resized to `size` (logical pixels), if the host agrees.
    fn ask_size(&mut self, window: &mut baseview::Window<'_>, size: (u32, u32)) {
        let sizes = &self.host.params.editor_size;
        let before = sizes.get();
        if size == before {
            return;
        }
        // The host asks the editor for its size while it decides.
        sizes.set(size);
        if self.host.context.request_resize() {
            window.resize(logical(size));
        } else {
            sizes.set(before);
        }
    }

    fn mouse(&mut self, window: &mut baseview::Window<'_>, event: MouseEvent) -> EventStatus {
        let scale = self.scale;
        match event {
            MouseEvent::CursorMoved { position, modifiers } => {
                self.pointer = (position.x, position.y);
                if let Some(drag) = self.drag {
                    self.drag_to(window, drag);
                } else {
                    self.with_ui(|ui| {
                        ui.set_modifiers(ui_modifiers(modifiers));
                        ui.pointer_moved(position.x * scale, position.y * scale)
                    });
                }
            }
            MouseEvent::ButtonPressed { button: MouseButton::Left, .. } if self.on_grip() => {
                self.drag = Some(Drag { pointer: self.pointer, size: self.host.params.editor_size.get() });
            }
            MouseEvent::ButtonReleased { button: MouseButton::Left, .. } if self.drag.is_some() => {
                self.drag = None;
            }
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

/// The resize grip: diagonal stripes in the bottom-right `side` × `side`
/// pixels of `buffer` (`width` × `height`).
fn draw_grip(buffer: &mut [u32], width: usize, height: usize, side: usize) {
    if side == 0 || width < side || height < side || buffer.len() < width * height {
        return;
    }
    let period = side / 4;
    let rows = buffer[..width * height].chunks_exact_mut(width).skip(height - side);
    for (y, row) in rows.enumerate() {
        for (x, pixel) in row[width - side..].iter_mut().enumerate() {
            // From the corner: a stripe every `period` along the diagonal.
            let from_corner = (side - 1 - x) + (side - 1 - y);
            if from_corner < side && from_corner % period < period / 2 {
                *pixel = GRIP_COLOR;
            }
        }
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

    fn on_event(&mut self, window: &mut baseview::Window<'_>, event: Event) -> EventStatus {
        match event {
            Event::Window(WindowEvent::Resized(info)) => {
                let size = info.physical_size();
                self.resize(size.width, size.height, info.scale());
                EventStatus::Captured
            }
            Event::Window(_) => EventStatus::Ignored,
            Event::Mouse(event) => self.mouse(window, event),
            Event::Keyboard(event) => self.key(&event),
        }
    }
}

/// The host's window, for baseview, which takes raw-window-handle 0.5.
struct HostWindow(ParentWindowHandle);

// SAFETY: the handle is the host's window, valid while the editor is open,
// and of the kind of this platform.
#[expect(unsafe_code, reason = "raw-window-handle 0.5 makes this trait unsafe")]
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
        #[expect(unsafe_code, reason = "a raw handle borrowed for the window's lifetime")]
        Ok(unsafe { rwh06::DisplayHandle::borrow_raw(self.display) })
    }
}

impl rwh06::HasWindowHandle for Target {
    fn window_handle(&self) -> Result<rwh06::WindowHandle<'_>, rwh06::HandleError> {
        let window = self.window.ok_or(rwh06::HandleError::Unavailable)?;
        // SAFETY: as for the display: baseview's window outlives the surface.
        #[expect(unsafe_code, reason = "a raw handle borrowed for the window's lifetime")]
        Ok(unsafe { rwh06::WindowHandle::borrow_raw(window) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_fitted_to_the_smallest_window() {
        assert_eq!(fitted(SIZE), SIZE);
        assert_eq!(fitted((0, 0)), ordrumbox_ui::MIN_WINDOW);
        assert_eq!(fitted((u32::MAX, u32::MAX)), (MAX_SIDE, MAX_SIDE));
    }

    #[test]
    fn the_grip_stays_in_its_corner() {
        let (width, height, side) = (40, 30, 14);
        let mut buffer = vec![0; width * height];
        draw_grip(&mut buffer, width, height, side);
        let drawn = |x: usize, y: usize| buffer[y * width + x] == GRIP_COLOR;
        assert!(drawn(width - 1, height - 1), "the corner itself");
        for (i, &pixel) in buffer.iter().enumerate() {
            let (x, y) = (i % width, i / width);
            if pixel == GRIP_COLOR {
                assert!(x >= width - side && y >= height - side, "({x}, {y}) out of the corner");
            }
        }
        // Too small a window: nothing drawn, no panic.
        let mut small = vec![0; 4];
        draw_grip(&mut small, 2, 2, side);
        assert!(small.iter().all(|&p| p == 0));
    }
}
