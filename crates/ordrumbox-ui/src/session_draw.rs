//! The session drawn: the window split and what it shows, and the frame.

use super::Session;
use super::session_overlay::Overlay;
use super::session_types::Workspace;
use crate::action_log::LogLayout;
use crate::canvas::Canvas;
use crate::picker::PickerLayout;
use crate::scale::Scale;
use crate::view::View;
use crate::{action_log, image, menu, picker, theme, view};
use ordrumbox_core::{Library, Song};
use ordrumbox_engine::{Mode, Position};

impl Session {
    /// The header's first group: a plugin shows its host's transport there.
    pub(super) fn transport_shown(&self) -> view::Transport {
        if self.host.hosted { view::Transport::Host(self.host.shown) } else { view::Transport::Own }
    }

    /// The window split for `size` (a real window size): the main editor
    /// region (top-left) and the note editor under it.
    pub(crate) fn body(&self, size: (i32, i32, Scale)) -> view::Body {
        let (width, height, scale) = size;
        let right = self.track_editor_width(width, scale);
        view::Body::new(width, height, scale, right, self.status.len(), self.note_editor_shown())
    }

    /// The note editor is shown, under the grid or the piano roll: the
    /// views that edit notes.
    pub(crate) fn note_editor_shown(&self) -> bool {
        self.chrome.show_note_editor
            && !self.kit_editor.is_open()
            && matches!(self.workspace, Workspace::Grid | Workspace::PianoRoll)
    }

    /// What the window shows that sets the room the layout needs.
    pub fn shown(&self) -> view::Shown {
        view::Shown {
            track_editor: self.chrome.show_track_editor,
            grid: !self.kit_editor.is_open() && self.workspace == Workspace::Grid,
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
        let size = (canvas.width(), canvas.height(), scale);
        let waiting = self.waiting_blink();
        let played = self.played_fraction(position, waiting);
        let shown_beats = self.follow_beats(size, played);
        let history = self.editor.history();
        let library = self.editor.library();
        let song_mode = match self.playback.mode() {
            Some(Mode::Song(s)) => library.songs.get(s),
            _ => None,
        };
        let song = song_text(library, song_mode, position, self.editor.selected_song());
        // Smoothed per-track levels for the VU meter stripe (peak-hold + decay).
        let levels = self.chrome.smooth_levels(&self.playback.levels()).to_vec();
        let pattern = self.pattern();
        let song_layout = self.song_layout((canvas.width(), canvas.height(), scale));
        let rows = if self.workspace == Workspace::Song { self.song_rows() } else { Vec::new() };
        let kit_first = self.kit_first(&self.kit_layout((canvas.width(), canvas.height(), scale)));
        let roll_layout = self.roll_layout((canvas.width(), canvas.height(), scale));
        let roll = self.piano_roll_open().then(|| Some((self.roll_view(&roll_layout)?, self.roll_title()))).flatten();
        let view = View {
            pattern,
            sounds: pattern.map_or_else(Vec::new, |p| (0..p.tracks.len()).map(|t| self.track_sound_lines(t)).collect()),
            arrangement: self.song_view_open().then(|| self.song_view_data(&rows, &song_layout)),
            piano_roll: roll,
            synth: self.synth_open(),
            mixer: self.mixer_open(),
            kit_view: self.kit_view(kit_first, layout.details_top),
            right: layout.right,
            beat_window: self.chrome.beat_window,
            lane: (self.editor_track(), usize::from(self.editors.lane)),
            note_editor: self.note_editor_shown(),
            levels: levels.clone(),
            show_vus: self.chrome.show_vus,
            transport: self.transport_shown(),
            host_muted: self.host.muted,
            switches: self.shown_switches(),
            switches_taken_back: self.switches_taken_back(),
            hint: self.hint(),
            hovered_track: match self.chrome.hover {
                Some(crate::view::Hit::Name(t)) => Some(t),
                _ => None,
            },
            host_moved: self.host.mix.moved_tracks(),
            host_split: self.host.mix.split,
            track_drop: self.track_drop((canvas.width(), canvas.height(), scale)),
            waiting,
            playhead: played.map(|played| {
                let total_beats = self.pattern().map_or(1, |pat| pat.beat_count.max(1));
                let first = self.chrome.beat_window.first().clamp(0, total_beats - 1);
                playhead_in_window(played, total_beats, first, shown_beats)
            }),
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
        self.draw_mixer(canvas, scale, &levels);
        crate::panel::draw(canvas, &layout.items, self.chrome.widgets.focus, scale);
        self.draw_tooltip(canvas, scale);
        let menus = self.menus();
        menu::draw(canvas, &menus, self.chrome.menu, scale);
        // In the bar, which the open menu's panel hangs under.
        menu::draw_library(canvas, &menus, &self.project_name(), self.library_state(), scale);
        if let Some(dropdown) = &self.chrome.widgets.dropdown {
            dropdown.draw(canvas, scale);
        }
        if let Some(entry) = &self.chrome.widgets.entry {
            entry.draw(canvas, scale);
        }
        for (rect, toast) in self.flash.rects(canvas.width(), canvas.height(), scale) {
            view::draw_toast(canvas, &toast.text, rect.y + rect.h, scale);
        }
        self.draw_overlay(canvas, scale);
    }

    /// How far the pattern shown has played, from 0 to 1: the pattern's
    /// pass, or the song's clip of it; `None` while it does not play (it
    /// waits for another pattern, or the song plays another).
    pub(super) fn played_fraction(&self, position: Option<Position>, waiting: Option<bool>) -> Option<f64> {
        if !self.playing() || waiting.is_some() {
            return None;
        }
        match self.playback.mode() {
            Some(Mode::Song(_)) => self
                .song_clip_playing()
                .filter(|clip| clip.pattern == self.pattern_index)
                .map(|clip| clip.local_tick as f64 / clip.pattern_ticks.max(1) as f64),
            _ => position.map(|p| p.frame as f64 / p.frames.max(1) as f64),
        }
    }

    /// Bring the playhead, else the cursor, in sight of the grid; the beats
    /// a full window of it shows.
    fn follow_beats(&mut self, size: (i32, i32, Scale), played: Option<f64>) -> i32 {
        let total = self.pattern().map_or(1, |p| p.beat_count.max(1));
        let shown = self.chrome.beat_window.shown(view::page_beats(self.body(size).main_w, size.2), total);
        let playhead = played.map(|played| (played * f64::from(total)).floor() as i32);
        self.chrome.beat_window.follow(shown, playhead, self.selection.cursor.map(|c| c.beat));
        shown
    }

    /// The overlay open over the window, if any.
    fn draw_overlay(&self, canvas: &mut Canvas, scale: Scale) {
        match &self.chrome.overlay {
            Some(Overlay::Picker(p)) => {
                let layout = PickerLayout::new(canvas.width(), canvas.height(), scale);
                picker::draw(canvas, p, &layout);
            }
            Some(Overlay::Shortcuts { scroll }) => {
                view::draw_notice_scrolled(canvas, "Keyboard shortcuts", &self.shortcut_lines(), *scroll, scale);
            }
            Some(Overlay::About) => {
                view::draw_notice_with_image(canvas, "About orDrumbox", &image::LOGO, &self.about(), scale);
            }
            Some(Overlay::Preferences(prefs)) => {
                view::draw_notice(canvas, "Preferences", &prefs.lines(), scale);
            }
            Some(Overlay::Controls(draft)) => self.draw_controls(canvas, draft, scale),
            Some(Overlay::NoteTool(draft)) => Self::draw_note_tool(canvas, draft, scale),
            Some(Overlay::LoopExport(draft)) => Self::draw_loop_export(canvas, draft, scale),
            Some(Overlay::History { scroll }) => {
                let layout = LogLayout::new(canvas.width(), canvas.height(), scale);
                action_log::draw(canvas, &self.action_log, *scroll, &layout);
            }
            // The choice notices: what `notice_choice` describes, drawn with
            // its two buttons.
            Some(Overlay::FactoryReset { .. } | Overlay::ConfirmOverwrite { .. } | Overlay::Confirm { .. }) => {
                if let Some(choice) = self.notice_choice() {
                    view::draw_notice_choice(
                        canvas,
                        &choice.title,
                        &choice.lines,
                        [&choice.buttons[0], &choice.buttons[1]],
                        choice.focus,
                        scale,
                    );
                }
            }
            None => {}
        }
    }
}

/// The header's song text: the song playing and where it is, else the one
/// selected.
fn song_text(library: &Library, playing: Option<&Song>, position: Option<Position>, selected: usize) -> Option<String> {
    match (playing, position) {
        (Some(song), Some(p)) => {
            let total = song.length_measures().max(1.0);
            let measure = (p.pass as f64 + p.frame as f64 / p.frames.max(1) as f64) % total;
            Some(format!("SONG {}  measure {}/{}", song.name, measure.floor() + 1.0, total.ceil()))
        }
        _ => library.songs.get(selected).map(|s| format!("song {}", s.name)),
    }
}

/// Where the playhead, `played` (0 to 1) through the pattern, is on the page
/// that starts at `start_beat` and holds `shown_beats` of the pattern's
/// `total_beats`, from 0 to 1.
fn playhead_in_window(played: f64, total_beats: i32, start_beat: i32, shown_beats: i32) -> f64 {
    let global_beat = played * f64::from(total_beats);
    let start = f64::from(start_beat);
    let page_end = (start + f64::from(shown_beats)).min(f64::from(total_beats));
    ((global_beat - start) / (page_end - start).max(1.0)).clamp(0.0, 1.0)
}
