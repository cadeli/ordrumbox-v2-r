//! The session drawn: the window split and what it shows, and the frame.

use super::session_types::Workspace;
use super::{Overlay, Session};
use crate::action_log::LogLayout;
use crate::canvas::Canvas;
use crate::picker::PickerLayout;
use crate::scale::Scale;
use crate::view::View;
use crate::{action_log, menu, picker, theme, view};
use ordrumbox_engine::Mode;

impl Session {
    /// The header's first group: a plugin shows its host's transport there.
    pub(super) fn transport_shown(&self) -> view::Transport {
        if self.host.hosted { view::Transport::Host(self.host.shown) } else { view::Transport::Own }
    }

    /// The window split for `size` (a real window size): the main editor
    /// region (top-left) and the output strip (bottom).
    pub(crate) fn body(&self, size: (i32, i32, Scale)) -> view::Body {
        let (width, height, scale) = size;
        let right = self.track_editor_width(width, scale);
        view::Body::new(width, height, scale, right, self.status.len(), self.output_shown(), self.note_editor_shown())
    }

    /// The note editor is shown, under the grid or the piano roll: the
    /// views that edit notes.
    pub(crate) fn note_editor_shown(&self) -> bool {
        self.chrome.show_note_editor
            && self.kit_editor.is_none()
            && matches!(self.workspace, Workspace::Grid | Workspace::PianoRoll)
    }

    /// The master strip (the output panel) is shown along the bottom: in
    /// the app, unless View > Master folded it.
    pub(crate) fn output_shown(&self) -> bool {
        !self.host.hosted && self.chrome.show_master_strip
    }

    /// What the window shows that sets the room the layout needs.
    pub fn shown(&self) -> view::Shown {
        view::Shown {
            output: self.output_shown(),
            track_editor: self.chrome.show_track_editor,
            grid: self.kit_editor.is_none() && self.workspace == Workspace::Grid,
            note_editor: self.chrome.show_note_editor,
        }
    }

    /// While the pattern shown waits for the one playing to end its loop:
    /// the blink of its marker, on in the first half of each beat.
    pub(crate) fn waiting_blink(&self) -> Option<bool> {
        let playing = self.playback.controller()?.playing_pattern()?;
        if playing == self.pattern_index {
            return None;
        }
        let at = self.playback.position()?;
        let beat_count = self.editor.library().patterns.get(playing).map_or(1, |p| p.beat_count.max(1));
        let beat = at.frame as f64 / at.frames.max(1) as f64 * beat_count as f64;
        Some(beat.fract() < 0.5)
    }

    /// The window's size and scale (where the lists opened from the menus go).
    pub fn set_size(&mut self, size: (i32, i32, Scale)) {
        self.chrome.panel_size = size;
    }

    pub fn draw(&mut self, canvas: &mut Canvas, scale: Scale) {
        // The scheme is per thread: a plugin draws on another than it loads on.
        theme::set_scheme(self.chrome.color_scheme);
        self.note_actions();
        self.note_message();
        let layout = self.panel_layout(canvas.width(), canvas.height(), scale);
        let position = self.playback.position();
        let history = self.editor.history();
        let library = self.editor.library();
        let song_mode = match self.playback.mode() {
            Some(Mode::Song(s)) => library.songs.get(s),
            _ => None,
        };
        let song = match (song_mode, position) {
            (Some(song), Some(p)) => {
                let total = song.length_measures().max(1.0);
                let measure = (p.pass as f64 + p.frame as f64 / p.frames.max(1) as f64) % total;
                Some(format!("SONG {}  measure {}/{}", song.name, measure.floor() + 1.0, total.ceil()))
            }
            _ => library.songs.get(self.editor.selected_song()).map(|s| format!("song {}", s.name)),
        };
        // Auto-page: keep the cursor / playhead on the current page.
        let size = (canvas.width(), canvas.height(), scale);
        let page_beats = view::page_beats(self.body(size).main_w, scale) as usize;
        let beat_count = self.pattern().map(|p| p.beat_count.max(1)).unwrap_or(1);
        let page_count = (beat_count as usize).div_ceil(page_beats).max(1);
        let cursor_page = self.selection.cursor.map(|c| (c.beat.max(0) as usize) / page_beats);
        // The pass playing is another pattern's while the one shown waits.
        let waiting = self.waiting_blink();
        let playhead_page = if self.playing() && waiting.is_none() {
            position.map(|p| {
                let frac = p.frame as f64 / p.frames.max(1) as f64;
                let global_beat = (frac * beat_count as f64).floor() as usize;
                global_beat / page_beats
            })
        } else {
            None
        };
        let new_page = playhead_page.or(cursor_page).unwrap_or(self.chrome.current_page);
        self.chrome.current_page = new_page.min(page_count.saturating_sub(1));

        // Smoothed per-track levels for the VU meter stripe (peak-hold + decay).
        let levels = self.chrome.smooth_levels(&self.playback.levels()).to_vec();
        let pattern = self.pattern();
        let song_layout = self.song_layout((canvas.width(), canvas.height(), scale));
        let rows = if self.workspace == Workspace::Song { self.song_rows() } else { Vec::new() };
        let roll_layout = self.roll_layout((canvas.width(), canvas.height(), scale));
        let roll = self.piano_roll_open().then(|| Some((self.roll_view(&roll_layout)?, self.roll_title()))).flatten();
        let view = View {
            pattern,
            sounds: pattern.map_or_else(Vec::new, |p| (0..p.tracks.len()).map(|t| self.track_sound_lines(t)).collect()),
            arrangement: self.song_view_open().then(|| self.song_view_data(&rows, &song_layout)),
            piano_roll: roll,
            output: self.output_view(),
            synth: self.synth_open(),
            kit_view: self.kit_view(layout.details_top),
            right: layout.right,
            current_page: self.chrome.current_page,
            lane: (self.editor_track(), usize::from(self.editors.lane)),
            note_editor: self.note_editor_shown(),
            levels,
            show_vus: self.chrome.show_vus,
            transport: self.transport_shown(),
            host_muted: self.host.muted,
            track_drop: self.track_drop((canvas.width(), canvas.height(), scale)),
            waiting,
            playhead: if song_mode.is_some() || waiting.is_some() {
                None
            } else {
                position.map(|p| {
                    let global_frac = p.frame as f64 / p.frames.max(1) as f64;
                    let total_beats = self.pattern().map(|pat| pat.beat_count.max(1)).unwrap_or(1) as f64;
                    let global_beat = global_frac * total_beats;
                    let start_beat = (self.chrome.current_page * page_beats) as f64;
                    let page_end = (start_beat + page_beats as f64).min(total_beats);
                    ((global_beat - start_beat) / (page_end - start_beat).max(1.0)).clamp(0.0, 1.0)
                })
            },
            song: song.as_deref().filter(|_| !self.host.hosted),
            cursor: self.selection.cursor,
            selected: self.selection.selected,
            zone: self.selection.zone,
            undo: history.next_undo(),
            redo: history.next_redo(),
            message: &self.message,
            status: &self.status,
        };
        view::draw(canvas, &view, scale);
        self.draw_synth(canvas, scale);
        crate::panel::draw(canvas, &layout.items, self.chrome.widgets.focus, scale);
        self.draw_tooltip(canvas, scale);
        let menus = self.menus();
        menu::draw(canvas, &menus, self.chrome.menu, scale);
        // In the bar, which the open menu's panel hangs under.
        menu::draw_library(canvas, &menus, &self.song_name(), self.library_state(), scale);
        if let Some(dropdown) = &self.chrome.widgets.dropdown {
            dropdown.draw(canvas, scale);
        }
        if let Some(entry) = &self.chrome.widgets.entry {
            entry.draw(canvas, scale);
        }
        for (rect, toast) in self.flash.rects(canvas.width(), canvas.height(), scale) {
            view::draw_toast(canvas, &toast.text, rect.y + rect.h, scale);
        }
        match &self.chrome.overlay {
            Some(Overlay::Picker(p)) => {
                let layout = PickerLayout::new(canvas.width(), canvas.height(), scale);
                picker::draw(canvas, p, &layout);
            }
            Some(Overlay::Shortcuts { scroll }) => {
                view::draw_notice_scrolled(canvas, "Keyboard shortcuts", &self.shortcut_lines(), *scroll, scale);
            }
            Some(Overlay::About) => view::draw_notice(canvas, "About orDrumbox", &self.about(), scale),
            Some(Overlay::Preferences(prefs)) => {
                view::draw_notice(canvas, "Preferences", &prefs.lines(), scale);
            }
            Some(Overlay::History { scroll }) => {
                let layout = LogLayout::new(canvas.width(), canvas.height(), scale);
                action_log::draw(canvas, &self.action_log, *scroll, &layout);
            }
            Some(Overlay::FactoryReset { yes }) => view::draw_notice_choice(
                canvas,
                "Factory reset",
                &self.factory_reset_lines(),
                ["Keep", "Reset"],
                usize::from(*yes),
                scale,
            ),
            Some(Overlay::ConfirmOverwrite { path, yes, .. }) => {
                // The name alone: the folder was just picked, and a long path would not fit.
                let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                let lines = [format!("{name} already exists in this folder."), String::new()];
                view::draw_notice_choice(
                    canvas,
                    "Replace the file?",
                    &lines,
                    ["Keep it", "Replace it"],
                    usize::from(*yes),
                    scale,
                )
            }
            Some(Overlay::Confirm { pending, yes }) => {
                let (title, lines, action) = self.confirm_text(pending);
                view::draw_notice_choice(canvas, title, &lines, ["Keep", &action], usize::from(*yes), scale)
            }
            None => {}
        }
    }
}
