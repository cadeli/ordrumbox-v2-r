//! The window's icon and its application id, so that Windows and X11 window
//! managers show the orDrumbox icon (macOS takes it from the `.app` bundle,
//! Wayland from the `.desktop` file the id names: `installer/`).

use winit::window::{Icon, WindowAttributes};

const SIZE: u32 = 256;
const PIXELS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/window_icon.rgba"));

/// The application id: the `.desktop` file's name, the X11 class, the Wayland `app_id`.
#[cfg(all(unix, not(target_os = "macos")))]
const APP_ID: &str = "ordrumbox";

/// `attributes` with the icon, and on Linux the application id.
#[expect(clippy::expect_used, reason = "the pixels are a build-time constant of the right size (`build.rs`, test)")]
pub fn with_icon(attributes: WindowAttributes) -> WindowAttributes {
    let icon = Icon::from_rgba(PIXELS.to_vec(), SIZE, SIZE).expect("the icon pixels are 256 x 256 RGBA");
    let attributes = attributes.with_window_icon(Some(icon));
    #[cfg(all(unix, not(target_os = "macos")))]
    let attributes = {
        use winit::platform::wayland::WindowAttributesExtWayland;
        use winit::platform::x11::WindowAttributesExtX11;
        let attributes = WindowAttributesExtWayland::with_name(attributes, APP_ID, APP_ID);
        WindowAttributesExtX11::with_name(attributes, APP_ID, APP_ID)
    };
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_pixels_make_an_icon() {
        assert!(Icon::from_rgba(PIXELS.to_vec(), SIZE, SIZE).is_ok());
    }
}
