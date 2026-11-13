//! The overlays drawn over the window: the keyboard shortcuts, the edit
//! history, About, the factory reset, and the keys and clicks they take.

use super::*;
use crate::scale::Scale;

/// Every key, for Help > Keyboard shortcuts.
const SHORTCUTS: &[&str] = &[
    "MENUS",
    "  F10 or Alt             open the menus: Arrows move, Enter runs, Esc closes",
    "  F1-F7, Ctrl+1-7        this list, track editor, song, piano roll, grid, synth, note editor",
    "                         (Ctrl+K: kit editor)",
    "GRID",
    "  Arrows                 move the cursor",
    "  Enter / click          switch the step on (a note) or off (all its notes)",
    "  Shift+click            select the step's note for the note editor, and hear it",
    "  Shift+drag             draw a zone: Ctrl+C, Ctrl+X, Delete take its notes",
    "  Shift+Arrows           pitch (up/down) of the selected note, one step in time (left/right)",
    "  Delete                 clear the zone or the step   Esc  deselect",
    "  L                      accent the cursor step's notes, or take the accent off",
    "  right-click            a track's name: track menu   a step: notes menu",
    "  F, Shift+G             random pattern, random drumkit",
    "  Shift+V, Shift+C       VU meters on/off, next colour scheme",
    "  Shift+H, J, K          every track: synth sounds, auto-assigned, random samples",
    "PLAY",
    "  M, S                   mute / solo the track under the cursor",
    "  1-9 / Shift+1-9        mute / solo track 1-9        Q-P  hear track 1-10 (any view)",
    "                         (these two rows by their place; other keys as the layout marks them)",
    "  Space / Shift+Space    play or stop the pattern / the song",
    "  [ ] / Shift+[ ]        previous / next pattern / song (also PageUp PageDown)",
    "  - = (Shift: x10)       tempo (every pattern's; the song's in the song panel)",
    "  , .                    previous / next kit",
    "FILE AND EDIT",
    "  Ctrl+S                 save the library now (it is also saved on its own)",
    "  Ctrl+Shift+S           export the pattern as JSON",
    "  Ctrl+O, Ctrl+Q         import a MIDI file, quit",
    "  Ctrl+E, Ctrl+Shift+M   export WAV (Shift: the song), export MIDI",
    "  Ctrl+Z, Ctrl+Shift+Z   undo, redo (also Ctrl+Y)",
    "  Ctrl+C, Ctrl+X, Ctrl+V copy, cut, paste the zone or the step (Shift: the track)",
    "  Ctrl+N, Ctrl+D         new pattern, duplicate pattern",
    "  Ctrl+T, Ctrl+Backspace add a KICK track, remove a track",
    "  Ctrl+G, Ctrl+L         next grid, loop one step shorter",
    "  Ctrl+- Ctrl+= Ctrl+0   zoom out, in, back to the display's size (also the keypad's - +)",
    "GENERATE",
    "  Shift+B, Ctrl+Shift+B  the pattern again in its style, a new pattern in the style",
    "  Shift+D, Shift+S       drums, bass in auto mode (again: off)",
    "  Shift+X                random notes on the track",
    "KNOBS, SLIDERS, LISTS (track editor, note editor, kit editor, volumes)",
    "  Tab / Shift+Tab        focus the next / previous control      Esc  back to the grid",
    "  Arrows, - =, wheel     change (Shift or Alt: finely, Ctrl: x10); a list: next option",
    "  drag                   a knob up / down, a slider along (Shift: finely)",
    "  double-click           default value     right-click or Enter: type a value",
    "  Enter or Space         open a list, press a button",
    "PIANO ROLL (F4)",
    "  click / drag / right-click a cell: add, select, delete; pitch (up/down) or velocity; menu",
    "  Arrows, Shift+Arrows   the cursor; the note's pitch (up/down), time (left/right)   Enter, Delete",
    "  Shift+drag             a zone of steps and keys: Ctrl+C, Ctrl+X, Delete; Ctrl+V at the cursor",
    "  a key: hear it, right-click: clear, chord progression     wheel (Shift: pages)",
    "SYNTH EDITOR (F6)",
    "  OSC, FLT, MOD, ENV     the track's preset: knobs, waves, lists, power buttons (saved)",
    "OUTPUT PANEL (bottom strip, always shown)",
    "  Volume, Comp, Filter   the master volume and spectrum, the compressor, the filters (saved)",
    "SONG PANEL (F3)",
    "  click / right-click    the ruler: aim the cursor; a pattern, row, clip, the grid: menu",
    "  Left Right, Up Down    the cursor a measure back / on, previous / next pattern",
    "  Space, wheel           play the song from the cursor; scroll (Shift: the rows), or drag the bars",
    "KIT EDITOR (Ctrl+K)",
    "  Up Down                sample      Enter  hear      Delete  remove",
    "  N, E                   normalize, export the mapping",
    "  drop files             a .wav joins the kit (Shift: replaces the sample), a folder",
    "                         becomes a kit, a .json mapping loads, a .mid imports",
];

/// Every key of a plugin, which leaves Space, the Ctrl/Cmd keys and the songs
/// to the host (`Ui::key`).
const PLUGIN_SHORTCUTS: &[&str] = &[
    "THE HOST'S KEYS",
    "  Space, every Ctrl/Cmd key   the host's: its transport, its save, undo, zoom...",
    "  Alt + the app's Ctrl key    orDrumbox's: Alt+Z undo, Alt+C copy, Alt+K kit editor...",
    "                              (the menus show them)",
    "MENUS",
    "  F10 or Alt             open the menus: Arrows move, Enter runs, Esc closes",
    "  F1 F2 F4 F5 F6 F7      this list, track editor, piano roll, grid, synth, note editor",
    "  F2, F7                 the track editor, the note editor: shown or folded (the window follows)",
    "  songs, tempo, saving   the app's or the host's: no F3, no - = (the tempo is HOST's)",
    "GRID",
    "  Arrows                 move the cursor",
    "  Enter / click          switch the step on (a note) or off (all its notes)",
    "  Shift+click            select the step's note for the note editor, and hear it",
    "  Shift+drag             draw a zone: Alt+C, Alt+X, Delete take its notes",
    "  Shift+Arrows           pitch (up/down) of the selected note, one step in time (left/right)",
    "  Delete                 clear the zone or the step   Esc  deselect",
    "  L                      accent the cursor step's notes, or take the accent off",
    "  right-click            a track's name: track menu   a step: notes menu",
    "  F, Shift+G             random pattern, random drumkit",
    "  Shift+V, Shift+C       VU meters on/off, next colour scheme",
    "  Shift+H, J, K          every track: synth sounds, auto-assigned, random samples",
    "PLAY",
    "  M, S                   mute / solo the track under the cursor",
    "  1-9 / Shift+1-9        mute / solo track 1-9        Q-P  hear track 1-10 (any view)",
    "                         (these two rows by their place; other keys as the layout marks them)",
    "  [ ]                    previous / next pattern (also PageUp PageDown)",
    "  , .                    previous / next kit",
    "GENERATE",
    "  Shift+B                the pattern again in its style (Alt+Shift+B: a new one)",
    "  Shift+D, Shift+S       drums, bass in auto mode (again: off)",
    "  Shift+X                random notes on the track",
    "KNOBS, SLIDERS, LISTS (track editor, note editor, kit editor, volumes)",
    "  Tab / Shift+Tab        focus the next / previous control      Esc  back to the grid",
    "  Arrows, - =, wheel     change (Shift or Alt: finely); a list: next option",
    "  drag                   a knob up / down, a slider along (Shift: finely)",
    "  double-click           default value     right-click or Enter: type a value",
    "  Enter                  open a list, press a button",
    "PIANO ROLL (F4)",
    "  click / drag / right-click a cell: add, select, delete; pitch (up/down) or velocity; menu",
    "  Arrows, Shift+Arrows   the cursor; the note's pitch (up/down), time (left/right)   Enter, Delete",
    "  Shift+drag             a zone of steps and keys: copy, cut, Delete; paste at the cursor",
    "  a key: hear it, right-click: clear, chord progression     wheel (Shift: pages)",
    "SYNTH EDITOR (F6)",
    "  OSC, FLT, MOD, ENV     the track's preset: knobs, waves, lists, power buttons (saved)",
    "KIT EDITOR (Alt+K)",
    "  Up Down                sample      Enter  hear      Delete  remove",
    "  N, E                   normalize, export the mapping",
    "  drop files             a .wav joins the kit (Shift: replaces the sample), a folder",
    "                         becomes a kit, a .json mapping loads, a .mid imports",
];

impl Session {
    /// A panel (file picker, key list, about) is over the window.
    pub fn overlay_open(&self) -> bool {
        self.chrome.overlay.is_some()
    }

    pub fn close_overlay(&mut self) {
        self.chrome.overlay = None;
    }

    /// A click on the status line: what was done so far.
    pub fn open_history(&mut self) {
        self.note_actions();
        self.chrome.overlay = Some(Overlay::History { scroll: 0 });
    }

    /// Add what the last events did to the log.
    pub(super) fn note_actions(&mut self) {
        let history = self.editor.history();
        self.action_log.note(Seen {
            message: &self.message,
            revision: self.editor.revision(),
            steps: history.undo_len(),
            undo: history.next_undo(),
            dragging: self.chrome.widgets.drag.is_some(),
        });
    }

    /// The lines of the keyboard shortcuts panel, as the platform shows them
    /// (Cmd in place of Ctrl on macOS; a plugin's say Alt, as its menus do).
    pub(super) fn shortcut_lines(&self) -> Vec<String> {
        let table: &[&str] = if self.hosted { PLUGIN_SHORTCUTS } else { SHORTCUTS };
        table
            .iter()
            .map(
                |line| {
                    if self.hosted { (*line).to_owned() } else { crate::menu::platform_shortcuts(line).into_owned() }
                },
            )
            .collect()
    }

    /// Scroll the shortcuts panel, if open, for `key`; whether it was.
    fn shortcuts_key(&mut self, key: &OverlayKey) -> bool {
        let count = self.shortcut_lines().len();
        let Some(Overlay::Shortcuts { scroll }) = &mut self.chrome.overlay else { return false };
        let (_, height, scale) = self.chrome.panel_size;
        let max = count.saturating_sub(view::notice_rows(height, scale));
        match key {
            OverlayKey::Move(delta) => *scroll = (*scroll).min(max).saturating_add_signed(*delta).min(max),
            OverlayKey::Edge(end) => *scroll = if *end { max } else { 0 },
            _ => self.chrome.overlay = None,
        }
        true
    }

    /// Scroll the history panel, if open, for `key`; whether it was.
    fn history_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::History { scroll }) = &mut self.chrome.overlay else { return false };
        let (width, height, scale) = self.chrome.panel_size;
        let max = LogLayout::new(width, height, scale).max_scroll(&self.action_log);
        match key {
            OverlayKey::Move(delta) => *scroll = (*scroll).min(max).saturating_add_signed(*delta).min(max),
            OverlayKey::Edge(end) => *scroll = if *end { max } else { 0 },
            OverlayKey::Escape => self.chrome.overlay = None,
            OverlayKey::Enter | OverlayKey::Back | OverlayKey::Letter(_) => {}
        }
        true
    }

    /// A key while a panel is open: the file picker's keys, or any key closes it.
    pub fn overlay_key(&mut self, key: OverlayKey) {
        if self.history_key(&key) || self.shortcuts_key(&key) {
            return;
        }
        let is_factory = matches!(self.chrome.overlay, Some(Overlay::FactoryReset));
        let Some(Overlay::Picker(picker)) = &mut self.chrome.overlay else {
            if is_factory && matches!(key, OverlayKey::Enter) {
                self.factory_reset();
                return;
            }
            if matches!(key, OverlayKey::Enter) {
                match self.chrome.overlay.take() {
                    Some(Overlay::ConfirmOverwrite { export, path }) => self.write_export(export, &path),
                    Some(Overlay::Confirm(pending)) => self.run_pending(pending),
                    _ => {}
                }
                return;
            }
            if matches!(key, OverlayKey::Escape) {
                self.chrome.overlay = None;
            }
            return;
        };
        let outcome = match key {
            OverlayKey::Escape => {
                self.chrome.overlay = None;
                return;
            }
            OverlayKey::Enter => picker.activate(),
            OverlayKey::Back => {
                picker.up();
                Outcome::Browsing
            }
            OverlayKey::Move(delta) => {
                picker.step(delta);
                Outcome::Browsing
            }
            OverlayKey::Edge(end) => {
                picker.home_end(end);
                Outcome::Browsing
            }
            OverlayKey::Letter(c) => {
                picker.jump(c);
                Outcome::Browsing
            }
        };
        self.picker_outcome(outcome);
    }

    /// A click while a panel is open.
    pub fn overlay_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) {
        if matches!(self.chrome.overlay, Some(Overlay::History { .. })) {
            if !LogLayout::new(width, height, scale).contains(x, y) {
                self.chrome.overlay = None;
            }
            return;
        }
        let Some(Overlay::Picker(picker)) = &mut self.chrome.overlay else {
            self.chrome.overlay = None;
            return;
        };
        let layout = PickerLayout::new(width, height, scale);
        if !layout.contains(x, y) {
            self.chrome.overlay = None;
            return;
        }
        if let Some(i) = layout.hit(picker, x, y) {
            let outcome = picker.click(i);
            self.picker_outcome(outcome);
        }
    }

    /// Lines of Help > About.
    pub(super) fn about(&self) -> Vec<String> {
        let shown = |p: Option<PathBuf>| p.map_or_else(|| "-".to_owned(), |p| p.display().to_string());
        vec![
            format!("orDrumbox {} - native port of ordrumbox-v2", update::BUILD),
            "MIT - https://www.ordrumbox.com".into(),
            String::new(),
            format!("library   {}", shown(self.saving.library_path.clone())),
            format!("user data {}{}", shown(self.user_folder()), self.user_folder_origin()),
            format!("exports   {}", shown(store::exports_dir())),
            format!("assets    {}", shown(self.assets_dir.clone())),
        ]
    }

    /// Where the user data folder comes from, after its path in Help > About.
    fn user_folder_origin(&self) -> &'static str {
        if store::env_library().is_some() {
            " (ORDRUMBOX_LIBRARY)"
        } else if self.user_folder().zip(store::default_dir()).is_some_and(|(d, default)| d != default) {
            " (chosen in the app)"
        } else {
            ""
        }
    }

    /// Lines of Help > Factory reset.
    pub(super) fn factory_reset_lines(&self) -> Vec<String> {
        vec![
            "Restore the factory settings?".into(),
            String::new(),
            "The user files are moved to backups/ in the user data folder:".into(),
            "  library.json, kits.json, settings.json,".into(),
            "  synth_presets.json, samples/, songs/".into(),
            "File > Restore user data brings them back.".into(),
            String::new(),
            format!("folder: {}", self.user_folder().map_or_else(|| "-".to_owned(), |p| p.display().to_string())),
            String::new(),
            "ENTER: restore     ESC or click: cancel".into(),
        ]
    }
}
