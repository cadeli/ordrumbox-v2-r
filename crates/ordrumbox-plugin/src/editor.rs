//! The plugin's window: a baseview child of the host's window, its pixels
//! drawn in software and shown through softbuffer.

use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr::NonNull;
use std::sync::Arc;

use baseview::{Event, EventStatus, WindowEvent, WindowHandle, WindowOpenOptions, WindowScalePolicy};
use nih_plug::prelude::{Editor, GuiContext, ParentWindowHandle};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle};
use raw_window_handle_06 as rwh06;

use crate::player::{SharedPlayer, lock};

/// The window's size in logical pixels (the app's first window size).
pub const SIZE: (u32, u32) = (1100, 680);

/// The window's background until the UI is drawn here.
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

/// What the window shows: the player's state, drawn into a softbuffer surface.
struct View {
    player: SharedPlayer,
    /// `None` when the surface could not be made or used: nothing is drawn
    /// (the reason is in the player's status).
    surface: Option<Surface>,
    width: u32,
    height: u32,
}

/// A softbuffer surface and the context it needs alive.
struct Surface {
    _context: softbuffer::Context<Target>,
    surface: softbuffer::Surface<Target, Target>,
}

impl View {
    fn new(window: &mut baseview::Window<'_>, player: SharedPlayer) -> Self {
        let target = Target::of(window);
        let surface = softbuffer::Context::new(target.clone()).and_then(|context| {
            softbuffer::Surface::new(&context, target).map(|surface| Surface { _context: context, surface })
        });
        let mut view = Self { player, surface: None, width: 0, height: 0 };
        match surface {
            Ok(surface) => view.surface = Some(surface),
            Err(e) => view.fail(format!("editor: {e}")),
        }
        view
    }

    fn resize(&mut self, width: u32, height: u32) {
        let (Some(w), Some(h), Some(surface)) = (NonZeroU32::new(width), NonZeroU32::new(height), &mut self.surface)
        else {
            return;
        };
        match surface.surface.resize(w, h) {
            Ok(()) => (self.width, self.height) = (width, height),
            Err(e) => self.fail(format!("editor: resize: {e}")),
        }
    }

    fn draw(&mut self) -> Result<(), softbuffer::SoftBufferError> {
        let Some(surface) = &mut self.surface else { return Ok(()) };
        if self.width == 0 {
            return Ok(());
        }
        let mut buffer = surface.surface.buffer_mut()?;
        buffer.fill(BACKGROUND);
        buffer.present()
    }

    /// The surface is given up; the window stays empty.
    fn fail(&mut self, message: String) {
        lock(&self.player).status.push(message);
        self.surface = None;
    }
}

impl baseview::WindowHandler for View {
    fn on_frame(&mut self, _window: &mut baseview::Window<'_>) {
        if let Err(e) = self.draw() {
            self.fail(format!("editor: {e}"));
        }
    }

    fn on_event(&mut self, _window: &mut baseview::Window<'_>, event: Event) -> EventStatus {
        if let Event::Window(WindowEvent::Resized(info)) = event {
            let size = info.physical_size();
            self.resize(size.width, size.height);
        }
        EventStatus::Captured
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
