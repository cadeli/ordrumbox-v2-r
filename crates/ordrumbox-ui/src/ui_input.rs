//! Input as the UI reads it, whatever window delivers it: the physical keys
//! it knows, the modifiers held, the pointer and its wheel. The program
//! translates its window events into these (winit in the app, the plugin
//! window later).

pub use crate::session::Button;

/// A physical key, by its place on a US keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter key, `'A'..='Z'`.
    Letter(char),
    /// A digit key of the main row, `0..=9`.
    Digit(u8),
    /// A function key, `1..=12`.
    F(u8),
    /// Left or right Alt (Option).
    Alt,
    Escape,
    Enter,
    NumpadEnter,
    Space,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    Minus,
    Equal,
    NumpadSubtract,
    NumpadAdd,
    BracketLeft,
    BracketRight,
    Comma,
    Period,
    /// Any other key: it still ends an Alt pressed alone, and its text is typed.
    Other,
}

impl Key {
    /// The key a W3C `KeyboardEvent.code` value names (`"KeyA"`, `"Digit1"`,
    /// `"ArrowUp"`…), the names winit's `KeyCode` and `keyboard-types`'
    /// `Code` both use.
    pub fn from_code(code: &str) -> Self {
        let letter = code.strip_prefix("Key").filter(|l| l.len() == 1).and_then(|l| l.chars().next());
        if let Some(letter) = letter.filter(char::is_ascii_uppercase) {
            return Self::Letter(letter);
        }
        let digit = code.strip_prefix("Digit").and_then(|d| d.parse::<u8>().ok()).filter(|d| *d <= 9);
        if let Some(digit) = digit {
            return Self::Digit(digit);
        }
        let function = code.strip_prefix('F').and_then(|f| f.parse::<u8>().ok()).filter(|f| (1..=12).contains(f));
        if let Some(function) = function {
            return Self::F(function);
        }
        match code {
            "AltLeft" | "AltRight" => Self::Alt,
            "Escape" => Self::Escape,
            "Enter" => Self::Enter,
            "NumpadEnter" => Self::NumpadEnter,
            "Space" => Self::Space,
            "Tab" => Self::Tab,
            "Backspace" => Self::Backspace,
            "Delete" => Self::Delete,
            "ArrowUp" => Self::ArrowUp,
            "ArrowDown" => Self::ArrowDown,
            "ArrowLeft" => Self::ArrowLeft,
            "ArrowRight" => Self::ArrowRight,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            "Home" => Self::Home,
            "End" => Self::End,
            "Minus" => Self::Minus,
            "Equal" => Self::Equal,
            "NumpadSubtract" => Self::NumpadSubtract,
            "NumpadAdd" => Self::NumpadAdd,
            "BracketLeft" => Self::BracketLeft,
            "BracketRight" => Self::BracketRight,
            "Comma" => Self::Comma,
            "Period" => Self::Period,
            _ => Self::Other,
        }
    }
}

/// A key pressed or released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    /// Pressed again by the keyboard's auto-repeat.
    pub repeat: bool,
    /// The text the key types with the current layout, if any.
    pub text: Option<String>,
}

/// The modifier keys held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// Command on macOS, Windows key on Windows.
    pub logo: bool,
}

impl Modifiers {
    /// Control or Command: what the menus' shortcuts are held with.
    pub fn command(self) -> bool {
        self.control || self.logo
    }
}

/// A turn of the mouse wheel or a swipe on a touchpad (positive: up, left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// In lines (and columns).
    Lines(f32, f32),
    /// In physical pixels.
    Pixels(f64, f64),
}

/// Pixels a scrolled line counts for.
const PIXELS_PER_LINE: f64 = 20.0;

impl ScrollDelta {
    /// Columns and lines scrolled, rounded.
    pub fn lines(self) -> (isize, isize) {
        match self {
            Self::Lines(x, y) => (x.round() as isize, y.round() as isize),
            Self::Pixels(x, y) => ((x / PIXELS_PER_LINE).round() as isize, (y / PIXELS_PER_LINE).round() as isize),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_read_from_their_w3c_code() {
        assert_eq!(Key::from_code("KeyQ"), Key::Letter('Q'));
        assert_eq!(Key::from_code("Digit0"), Key::Digit(0));
        assert_eq!(Key::from_code("F12"), Key::F(12));
        assert_eq!(Key::from_code("AltRight"), Key::Alt);
        assert_eq!(Key::from_code("NumpadEnter"), Key::NumpadEnter);
        for other in ["F13", "Digit10", "Keya", "KeyAB", "Numpad1", ""] {
            assert_eq!(Key::from_code(other), Key::Other, "{other}");
        }
    }

    #[test]
    fn scroll_delta_rounds_pixels_to_lines() {
        assert_eq!(ScrollDelta::Lines(0.4, -1.6).lines(), (0, -2));
        assert_eq!(ScrollDelta::Pixels(-30.0, 41.0).lines(), (-2, 2));
    }
}
