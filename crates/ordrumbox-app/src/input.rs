//! winit's input in the UI's words: the physical keys, the modifiers, and
//! the cursor a pointer shape shows.
// audit: allow(function size): `key` is a table of winit's key codes, one line each.

use ordrumbox_ui::ui_input::{Key, Modifiers, PointerShape};
use winit::keyboard::{KeyCode, ModifiersState};
use winit::window::CursorIcon;

/// The UI's name for a physical key.
pub(crate) fn key(code: KeyCode) -> Key {
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

/// The UI's modifiers from winit's.
pub(crate) fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers { shift: state.shift_key(), control: state.control_key(), alt: state.alt_key(), logo: state.super_key() }
}

/// The system cursor that shows `shape`.
pub(crate) const fn cursor_icon(shape: PointerShape) -> CursorIcon {
    match shape {
        PointerShape::Arrow => CursorIcon::Default,
        PointerShape::Grab => CursorIcon::Grab,
        PointerShape::Grabbing => CursorIcon::Grabbing,
        PointerShape::ResizeHorizontal => CursorIcon::EwResize,
        PointerShape::ResizeVertical => CursorIcon::NsResize,
        PointerShape::Move => CursorIcon::Move,
    }
}
