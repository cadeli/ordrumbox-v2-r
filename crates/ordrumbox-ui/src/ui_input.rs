//! Input as the UI reads it, whatever window delivers it: the physical keys
//! it knows, the modifiers held, the pointer and its wheel. The program
//! translates its window events into these (winit in the app, the plugin
//! window later).

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// The program runs on macOS: its Ctrl key is the Command one (the menus say
/// Cmd+, Export MIDI takes Shift to leave Cmd+M to the window).
pub const MACOS: bool = cfg!(target_os = "macos");

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
    /// A keypad key other than Enter, + and -: what it types is read where it is.
    Numpad,
    /// Any other key: it still ends an Alt pressed alone, and its text is typed.
    Other,
}

impl Key {
    /// The key's letter, when it is one.
    pub fn letter(self) -> Option<char> {
        match self {
            Self::Letter(c) => Some(c),
            _ => None,
        }
    }

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
            _ if code.starts_with("Numpad") => Self::Numpad,
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
    /// The character the key types with the current layout and no modifier
    /// held, if it types one: what its cap shows.
    pub unmodified: Option<char>,
}

impl KeyEvent {
    /// The key a shortcut reads: the one the layout marks. A letter, a digit
    /// or `- = [ ] , .` is the one the key types (Ctrl+Z is the key marked Z on
    /// an AZERTY keyboard too, where it sits in the place of W; `-` there is
    /// on the 6); a key of the main block that types other ASCII (AZERTY's `;`
    /// in the place of `,`) is none of them. The other keys (arrows, keypad…),
    /// a layout outside ASCII (Cyrillic, AZERTY's `à` on the 0) and a key whose
    /// character is unknown are read by their place.
    #[must_use]
    pub fn layout_key(&self) -> Key {
        let main_block = matches!(
            self.key,
            Key::Letter(_)
                | Key::Digit(_)
                | Key::Minus
                | Key::Equal
                | Key::BracketLeft
                | Key::BracketRight
                | Key::Comma
                | Key::Period
                | Key::Other
        );
        let Some(c) = self.unmodified.filter(|c| main_block && c.is_ascii_graphic()) else { return self.key };
        match c {
            'a'..='z' | 'A'..='Z' => Key::Letter(c.to_ascii_uppercase()),
            '0'..='9' => c.to_digit(10).and_then(|d| u8::try_from(d).ok()).map_or(Key::Other, Key::Digit),
            '-' => Key::Minus,
            '=' => Key::Equal,
            '[' => Key::BracketLeft,
            ']' => Key::BracketRight,
            ',' => Key::Comma,
            '.' => Key::Period,
            _ => Key::Other,
        }
    }
}

/// The one character of a key's text, if it is one character.
#[must_use]
pub fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    chars.next().filter(|_| chars.next().is_none())
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
        assert_eq!(Key::from_code("Numpad1"), Key::Numpad);
        for other in ["F13", "Digit10", "Keya", "KeyAB", "Backquote", ""] {
            assert_eq!(Key::from_code(other), Key::Other, "{other}");
        }
    }

    #[test]
    fn shortcut_keys_follow_the_layout() {
        let event = |key, unmodified| KeyEvent { key, pressed: true, repeat: false, text: None, unmodified };
        // AZERTY: Z in the place of W, A in the place of Q, M in the place of `;`,
        // `,` in the place of M, `;` in the place of `,`, `-` on the 6, `^` in the place of `[`.
        assert_eq!(event(Key::Letter('W'), Some('z')).layout_key(), Key::Letter('Z'));
        assert_eq!(event(Key::Letter('Q'), Some('a')).layout_key(), Key::Letter('A'));
        assert_eq!(event(Key::Other, Some('m')).layout_key(), Key::Letter('M'));
        assert_eq!(event(Key::Letter('M'), Some(',')).layout_key(), Key::Comma);
        assert_eq!(event(Key::Comma, Some(';')).layout_key(), Key::Other);
        assert_eq!(event(Key::Digit(6), Some('-')).layout_key(), Key::Minus);
        assert_eq!(event(Key::Minus, Some(')')).layout_key(), Key::Other);
        assert_eq!(event(Key::BracketLeft, Some('^')).layout_key(), Key::Other);
        // QWERTZ: Y and Z swapped, `-` in the place of `/`.
        assert_eq!(event(Key::Letter('Y'), Some('z')).layout_key(), Key::Letter('Z'));
        assert_eq!(event(Key::Other, Some('-')).layout_key(), Key::Minus);
        // Cyrillic, AZERTY's `à` on the 0, a dead key: by place.
        assert_eq!(event(Key::Letter('Z'), Some('я')).layout_key(), Key::Letter('Z'));
        assert_eq!(event(Key::Digit(0), Some('à')).layout_key(), Key::Digit(0));
        assert_eq!(event(Key::BracketLeft, None).layout_key(), Key::BracketLeft);
        // Keys outside the main block stay what they are.
        assert_eq!(event(Key::NumpadSubtract, Some('-')).layout_key(), Key::NumpadSubtract);
        assert_eq!(event(Key::Numpad, Some('.')).layout_key(), Key::Numpad);
        assert_eq!(event(Key::Space, Some(' ')).layout_key(), Key::Space);
        assert_eq!(event(Key::Enter, Some('\r')).layout_key(), Key::Enter);
        assert_eq!(single_char("z"), Some('z'));
        assert_eq!(single_char("ab"), None);
        assert_eq!(single_char(""), None);
    }

    #[test]
    fn scroll_delta_rounds_pixels_to_lines() {
        assert_eq!(ScrollDelta::Lines(0.4, -1.6).lines(), (0, -2));
        assert_eq!(ScrollDelta::Pixels(-30.0, 41.0).lines(), (-2, 2));
    }
}
