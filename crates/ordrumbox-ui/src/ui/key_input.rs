//! The keyboard: where a key goes, in order the menus, the overlay, the play
//! key, the focused widget, the track rows, the shortcuts and the open view.

use super::keys::*;
use super::{PAGE, ROW_DIGITS, ROW_LETTERS, Ui};
use crate::menu::{self, Command};
use crate::panel::Step;
use crate::session::{Move, OverlayKey, PanelKey, Session, Switch};
use crate::ui_input::{Key, KeyEvent, Modifiers};

impl Ui {
    /// Open the first menu from the keyboard, or close the open one.
    fn toggle_menu(&mut self) -> bool {
        let s = &mut self.session;
        let open = s.open_menu();
        s.close_overlay();
        s.set_menu(if open.is_some() { None } else { Some(menu::Open::new(&s.menus(), 0, true)) });
        true
    }

    /// A key pressed or released.
    pub fn key(&mut self, event: &KeyEvent) -> bool {
        self.input(|ui| ui.key_event(event))
    }

    /// Alt plus the first letter of a menu's title opens it (Y: Play, the
    /// Pattern's P taken). Whether `code` was such a letter.
    fn open_menu_by_letter(&mut self, code: Key) -> bool {
        let Some(i) = code.letter() else { return false };
        let s = &mut self.session;
        let menus = s.menus();
        let Some(menu) = menus.iter().position(|m| menu_letter(m.title, i)) else { return false };
        s.close_overlay();
        s.set_menu(Some(menu::Open::new(&menus, menu, true)));
        true
    }

    fn key_event(&mut self, event: &KeyEvent) -> bool {
        let code = event.key;
        let is_alt = code == Key::Alt;
        if !event.pressed {
            // Alt pressed and released alone opens the menus, as on Windows.
            return is_alt && std::mem::take(&mut self.alt_alone) && self.toggle_menu();
        }
        self.alt_alone = is_alt && !event.repeat;
        if is_alt {
            return false;
        }
        if code == Key::F(10) {
            return self.toggle_menu();
        }
        let shift = self.modifiers.shift;
        let control = self.modifiers.command();
        let alt = self.modifiers.alt;
        if let Some(open) = self.session.open_menu() {
            return self.menu_key(open, code);
        }
        if alt && self.open_menu_by_letter(code) {
            return true;
        }
        if self.session.overlay_open() {
            return self.overlay_key(event);
        }
        // Space plays or stops wherever the focus is (a button, a knob, an open
        // list…), except while writing text: an open entry keeps it (it types a
        // space). Menus and the file picker are modal and take it first.
        if code == Key::Space && !event.repeat && !control && !self.session.entry_open() {
            if shift {
                self.session.toggle_play_song();
            } else {
                self.session.toggle_play();
            }
            return true;
        }
        // The keys of the rows (1-9 mute, Q-P hear) are read by their place, a
        // keyboard of its own on every layout; every other key is the one the
        // layout marks. The kit editor keeps its own letters (N, R, E).
        let layout = event.layout_key();
        let kit_letter = self.session.kit_editor_open() && matches!(layout, Key::Letter('N' | 'R' | 'E'));
        let row_key = !control && !kit_letter && (ROW_DIGITS.contains(&code) || ROW_LETTERS.contains(&code));
        let key = if row_key { code } else { layout };
        if self.widget_key(event, key, shift || alt, control) {
            return true;
        }
        session_key(&mut self.session, event, (code, key, row_key), self.modifiers)
    }

    /// Keys for the editors' widgets: an open list or number entry takes
    /// them all; a focused knob, slider, select or button its own; Tab moves
    /// the focus. Whether the key was theirs.
    fn widget_key(&mut self, event: &KeyEvent, key: Key, fine: bool, control: bool) -> bool {
        let size = self.size();
        let key = match key {
            Key::ArrowUp => Some(PanelKey::Up),
            Key::ArrowDown => Some(PanelKey::Down),
            Key::ArrowLeft => Some(PanelKey::Left),
            Key::ArrowRight => Some(PanelKey::Right),
            // Ctrl+- and Ctrl+= stay the zoom.
            Key::Minus | Key::NumpadSubtract if !control => Some(PanelKey::Minus),
            Key::Equal | Key::NumpadAdd if !control => Some(PanelKey::Plus),
            Key::Enter | Key::NumpadEnter => Some(PanelKey::Enter),
            Key::Space => Some(PanelKey::Space),
            Key::Escape => Some(PanelKey::Escape),
            Key::Tab => Some(PanelKey::Tab),
            Key::PageUp => Some(PanelKey::PageUp),
            Key::PageDown => Some(PanelKey::PageDown),
            Key::Home => Some(PanelKey::Home),
            Key::End => Some(PanelKey::End),
            Key::Backspace => Some(PanelKey::Backspace),
            _ => None,
        };
        let s = &mut self.session;
        if s.popup_open() {
            match key {
                Some(key) if !(key == PanelKey::Minus || key == PanelKey::Plus) => {
                    s.panel_key(size, key, fine, control);
                }
                _ => {
                    for c in event.text.as_deref().unwrap_or("").chars() {
                        s.panel_text(c);
                    }
                }
            }
            return true;
        }
        key.is_some_and(|key| s.panel_key(size, key, fine, control))
    }

    /// Keys while a menu is open.
    fn menu_key(&mut self, open: menu::Open, code: Key) -> bool {
        let s = &mut self.session;
        let menus = s.menus();
        match code {
            Key::Escape => s.set_menu(None),
            Key::ArrowLeft => s.set_menu(Some(open.switch(&menus, -1))),
            Key::ArrowRight => s.set_menu(Some(open.switch(&menus, 1))),
            Key::ArrowUp => s.set_menu(Some(open.step(&menus, -1))),
            Key::ArrowDown => s.set_menu(Some(open.step(&menus, 1))),
            Key::Home => s.set_menu(Some(open.step(&menus, isize::MIN / 2))),
            Key::End => s.set_menu(Some(open.step(&menus, isize::MAX / 2))),
            Key::Letter(c) => s.set_menu(Some(open.jump(&menus, c))),
            Key::Enter | Key::NumpadEnter | Key::Space => match open.command(&menus) {
                Some(command) => s.run(command),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// Keys while the file picker (or another panel) is open.
    fn overlay_key(&mut self, event: &KeyEvent) -> bool {
        let key = match event.key {
            Key::Escape => OverlayKey::Escape,
            Key::Enter | Key::NumpadEnter => OverlayKey::Enter,
            Key::Backspace => OverlayKey::Back,
            Key::ArrowUp => OverlayKey::Move(-1),
            Key::ArrowDown => OverlayKey::Move(1),
            Key::ArrowLeft => OverlayKey::Side(Step::Previous),
            Key::ArrowRight => OverlayKey::Side(Step::Next),
            Key::PageUp => OverlayKey::Move(-PAGE),
            Key::PageDown => OverlayKey::Move(PAGE),
            Key::Home => OverlayKey::Edge(false),
            Key::End => OverlayKey::Edge(true),
            _ => match event.text.as_ref().and_then(|t| t.chars().next()).filter(|c| c.is_alphanumeric()) {
                Some(c) => OverlayKey::Letter(c),
                None => return false,
            },
        };
        self.session.overlay_key(key);
        true
    }
}

/// A key no menu, overlay or widget took: a row's mute, solo or note, a
/// shortcut, a Shift hint, the rewind, else the open view's. `code` is the
/// key by its place, `key` as the layout marks it; `row_key` whether it is
/// read as a row's.
fn session_key(
    s: &mut Session,
    event: &KeyEvent,
    (code, key, row_key): (Key, Key, bool),
    modifiers: Modifiers,
) -> bool {
    let (shift, control, alt) = (modifiers.shift, modifiers.command(), modifiers.alt);
    if let Some(row) = ROW_DIGITS.iter().position(|&k| k == code).filter(|_| row_key) {
        s.switch_track(row, if shift { Switch::Solo } else { Switch::Mute });
        return true;
    }
    if let Some(row) = ROW_LETTERS.iter().position(|&k| k == code).filter(|_| row_key) {
        s.audition_row(row);
        if !event.repeat {
            s.record_note(row);
        }
        return true;
    }
    // Ctrl+D on a clip selected in the song panel repeats it; else it duplicates the pattern.
    if control && !shift && key == Key::Letter('D') && s.song_view_open() && s.repeat_selected_clip() {
        return true;
    }
    if let Some(command) = shortcut(key, shift, control) {
        s.run(command);
        return true;
    }
    if control {
        return false;
    }
    if let Some(hint) = shift_hint(key).filter(|_| !shift && !s.kit_editor_open()) {
        s.say(hint);
        return true;
    }
    // Home and the keypad's Enter rewind the transport (the kit editor's Enter hears a sample).
    if matches!(key, Key::Home | Key::NumpadEnter) && !s.kit_editor_open() && !s.synth_open() {
        s.rewind();
        return true;
    }
    view_key(s, key, shift, alt)
}

/// What `key` does in the view that is open (the kit editor, the synth, the
/// mixer, the piano roll, the song view, else the grid): whether it was one of its keys.
fn view_key(s: &mut Session, key: Key, shift: bool, alt: bool) -> bool {
    if s.kit_editor_open() {
        kit_editor_key(s, key)
    } else if s.synth_open() {
        synth_key(s, key)
    } else if s.mixer_open() {
        mixer_key(s, key)
    } else if s.piano_roll_open() {
        piano_roll_key(s, key, shift)
    } else if s.song_view_open() {
        song_view_key(s, key)
    } else {
        grid_key(s, key, shift, alt)
    }
}

fn kit_editor_key(s: &mut Session, key: Key) -> bool {
    match key {
        Key::Escape => s.toggle_kit_editor(),
        Key::ArrowUp => s.kit_select(-1),
        Key::ArrowDown => s.kit_select(1),
        Key::Enter | Key::NumpadEnter => s.kit_audition(),
        Key::Delete | Key::Backspace => s.kit_remove(),
        Key::Letter('N') => s.kit_normalize(),
        Key::Letter('E') => s.run(Command::ExportMapping),
        _ => return false,
    }
    true
}

fn synth_key(s: &mut Session, key: Key) -> bool {
    if key != Key::Escape {
        return false;
    }
    s.toggle_synth();
    true
}

fn mixer_key(s: &mut Session, key: Key) -> bool {
    match key {
        Key::Escape => s.toggle_mixer(),
        Key::ArrowLeft => s.mixer_select(-1),
        Key::ArrowRight => s.mixer_select(1),
        _ => return false,
    }
    true
}

fn piano_roll_key(s: &mut Session, key: Key, shift: bool) -> bool {
    match key {
        Key::Escape => s.roll_escape(),
        Key::ArrowLeft => s.roll_arrow(Move::Left, shift),
        Key::ArrowRight => s.roll_arrow(Move::Right, shift),
        Key::ArrowUp => s.roll_arrow(Move::Up, shift),
        Key::ArrowDown => s.roll_arrow(Move::Down, shift),
        Key::Enter | Key::NumpadEnter => s.roll_enter(),
        Key::Delete | Key::Backspace => s.roll_delete(),
        _ => return false,
    }
    true
}

fn song_view_key(s: &mut Session, key: Key) -> bool {
    match key {
        Key::Escape => s.toggle_song_view(),
        Key::ArrowLeft => s.move_song_cursor(-1),
        Key::ArrowRight => s.move_song_cursor(1),
        Key::ArrowUp => s.step_pattern(-1),
        Key::ArrowDown => s.step_pattern(1),
        Key::Delete | Key::Backspace => return s.remove_selected_clip(),
        _ => return false,
    }
    true
}

fn grid_key(s: &mut Session, key: Key, shift: bool, alt: bool) -> bool {
    let tempo = if shift { 10.0 } else { 1.0 };
    match key {
        Key::Minus | Key::NumpadSubtract => s.step_tempo(-tempo),
        Key::Equal | Key::NumpadAdd => s.step_tempo(tempo),
        Key::ArrowLeft if alt => s.nudge(Move::Left),
        Key::ArrowRight if alt => s.nudge(Move::Right),
        Key::ArrowUp if alt => s.nudge(Move::Up),
        Key::ArrowDown if alt => s.nudge(Move::Down),
        Key::ArrowLeft if shift => s.extend_zone(Move::Left),
        Key::ArrowRight if shift => s.extend_zone(Move::Right),
        Key::ArrowUp if shift => s.extend_zone(Move::Up),
        Key::ArrowDown if shift => s.extend_zone(Move::Down),
        Key::ArrowLeft => s.move_cursor(Move::Left),
        Key::ArrowRight => s.move_cursor(Move::Right),
        Key::ArrowUp => s.move_cursor(Move::Up),
        Key::ArrowDown => s.move_cursor(Move::Down),
        Key::Enter | Key::NumpadEnter => s.enter(),
        Key::Delete | Key::Backspace => s.delete_step(),
        Key::Escape => s.deselect(),
        _ => return false,
    }
    true
}
