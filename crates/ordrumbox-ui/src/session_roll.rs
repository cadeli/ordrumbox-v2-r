//! The session's side of the piano roll, on the track editor's track: a click on an empty cell adds a
//! note there, on a note selects it (and plays it), a double-click on a note
//! deletes it; dragging a note moves it in time and pitch
//! (`session_roll_move.rs`), one undo step per drag; a click on a key plays it. Right-click
//! menus: on the keyboard, clear the track or write a chord progression from
//! that key; on the grid, add or delete the note, add a minor or major
//! chord. Keys: arrows move the cursor (the page follows), Shift+arrows
//! change the selected note's velocity (up, down) and pitch (left, right),
//! Enter adds, selects or deletes at the cursor, Delete deletes the selected
//! note. The wheel scrolls the keyboard (Shift: the pages).

use crate::scale::Scale;
use ordrumbox_core::note_positions::sub_positions;
use ordrumbox_core::pattern_engine::SpanResolver;
use ordrumbox_core::steps::{TICK, absolute_step};

use super::session_labels::beat_label;
use super::session_roll_bar::{bar_at, bar_hold};
use super::session_selection::NoteFocus;
use super::session_types::{Move, Workspace};
use super::{Session, session_panel};
use crate::canvas::Canvas;
use crate::editors;
use crate::note_drag::{MIDDLE_C, MIDI_MIN, midi_name};
use crate::panel::Id;
use crate::panel::{Item, Kind, Rect};
use crate::piano_roll::{self, BEATS_PER_PAGE, RollHit, RollLayout, RollView, TOTAL_KEYS, note_row, steps_per_beat};
use crate::ui_input::Button;
use crate::view;
use crate::view::Cursor;
use ordrumbox_core::{Note, Rng};
use ordrumbox_engine::Mode;

/// The piano roll's view and what the mouse holds in it.
#[derive(Debug, Default)]
pub(super) struct RollState {
    /// The beats shown: where they start and how many.
    pub(super) window: crate::view::BeatWindow,
    /// The beats' scroll bar, held by the mouse.
    pub(super) bar_drag: Option<crate::view::BarHold>,
    /// Highest row shown; `None`: centered on the track's pitch.
    top: Option<i32>,
    /// The keyboard cursor: step, row.
    pub(super) cursor: Option<(i64, i32)>,
    /// The note pressed.
    pub(super) drag: Option<super::session_roll_move::RollDrag>,
    /// The note last clicked, and when: a second click soon after deletes it.
    pub(super) last_note_click: Option<((usize, usize), std::time::Instant)>,
    /// The edited value shown over the note.
    pub(super) gauge: Option<String>,
    /// The next chord progression.
    pub(super) sequence: i64,
    /// The note under the mouse.
    hover: Option<usize>,
    /// A Shift+press on a cell, which may become a zone.
    pub(super) zone_drag: Option<super::session_roll_zone::RollZoneDrag>,
    /// A press on a note of the zone, which may drag the zone's notes.
    pub(super) zone_move: Option<super::session_roll_zone_move::RollZoneMove>,
    /// A press on the right edge of a note, which draws it out.
    pub(super) length_drag: Option<super::session_roll_length::RollLengthDrag>,
}

impl RollState {
    /// A note is held by the mouse.
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some() || self.length_drag.is_some()
    }

    /// Let go of the note held, if any.
    pub(super) fn release(&mut self) {
        self.drag = None;
        self.zone_drag = None;
        self.length_drag = None;
        self.bar_drag = None;
    }
}

/// Rows a notch of the wheel scrolls.
const WHEEL_ROWS: i32 = 3;

impl Session {
    // ─── State ───────────────────────────────────────────────────────────────

    /// The piano roll is shown (the kit editor opens over it).
    pub fn piano_roll_open(&self) -> bool {
        self.workspace == Workspace::PianoRoll && !self.kit_editor.is_open()
    }

    /// F4, View > Piano roll: the piano roll in the place of the grid, or
    /// back; it opens centered on the track's pitch, with nothing selected.
    pub fn toggle_piano_roll(&mut self) {
        let open = self.workspace == Workspace::PianoRoll;
        self.set_workspace(if open { Workspace::Grid } else { Workspace::PianoRoll });
        self.roll.top = None;
        self.roll.cursor = None;
        self.roll.gauge = None;
        self.selection.selected = None;
        self.selection.roll_zone = None;
    }

    /// The track the piano roll shows (the track editor's).
    pub(super) fn roll_track(&self) -> Option<usize> {
        self.editor_track()
    }

    pub(super) fn roll_layout(&self, size: (i32, i32, Scale)) -> RollLayout {
        let body = self.body(size);
        let spb = self.roll_track().and_then(|t| self.track(t)).map_or(4, steps_per_beat);
        RollLayout::new(body.main_w, body.editor_h, size.2, self.status.len(), (spb, self.roll_shown_beats()))
    }

    /// The beats across the grid: four, or as many as the scroll bar zoomed to.
    pub(super) fn roll_shown_beats(&self) -> i32 {
        self.roll.window.shown(BEATS_PER_PAGE, self.pattern().map_or(1, |p| p.beat_count.max(1)))
    }

    /// The step playing on the roll's track, while the pattern plays (alone
    /// or in a song's clip).
    pub(super) fn roll_playhead(&self) -> Option<i64> {
        let track = self.track(self.roll_track()?)?;
        let tick = if self.playback.plays(Mode::Pattern(self.pattern_index)) {
            u64::from(self.playback.pattern_tick()?)
        } else {
            self.song_clip_playing().filter(|clip| clip.pattern == self.pattern_index)?.local_tick
        };
        let spb = steps_per_beat(track);
        let in_beat = (tick % u64::from(TICK)) as f64 / (f64::from(TICK) / f64::from(spb));
        Some((tick / u64::from(TICK)) as i64 * i64::from(spb) + in_beat.floor() as i64)
    }

    /// What the piano roll shows.
    pub(super) fn roll_view(&self, layout: &RollLayout) -> Option<RollView<'_>> {
        let t = self.roll_track()?;
        let (pattern, track) = (self.pattern()?, self.track(t)?);
        let playhead = self.roll_playhead();
        let mut view = RollView {
            track,
            color: crate::theme::track(t),
            beat_count: pattern.beat_count.max(1),
            first_beat: self.roll.window.first(),
            shown_beats: self.roll_shown_beats(),
            top_row: self.roll.top.map_or_else(
                || layout.centered_top((MIDDLE_C + track.transpose - MIDI_MIN).round() as i32),
                |top| layout.clamp_top(top),
            ),
            selected: self.selection.selected.filter(|&(st, _)| st == t).map(|(_, n)| n),
            cursor: self.roll.cursor,
            playhead,
            subs: Vec::new(),
            zone: self.roll_zone().map(|zone| self.roll_span(&zone, track)),
            ruler_marks: self.ruler_marks(),
        };
        let span = SpanResolver::new(track);
        let mut rng = Rng::new(1);
        view.subs = track.notes.iter().map(|n| sub_positions(n, track, view.total_steps(), &span, &mut rng)).collect();
        Some(view)
    }

    /// The title line (`Piano Roll — TRACK`).
    pub(super) fn roll_title(&self) -> String {
        match self.roll_track().and_then(|t| self.track(t)) {
            Some(track) => format!("Piano Roll - {}", track.name),
            None => "Piano Roll".into(),
        }
    }

    /// The header's track list (which track the roll shows).
    pub(super) fn roll_items(&self, layout: &mut session_panel::PanelLayout, size: (i32, i32, Scale)) {
        let Some(pattern) = self.pattern().filter(|p| !p.tracks.is_empty()) else { return };
        let roll = self.roll_layout(size);
        let s = size.2;
        let x = roll.title.x + crate::canvas::text_width(&self.roll_title(), s) + 16 * s;
        let rect = Rect::new(x, roll.title.y - 2 * s, 140 * s, 12 * s);
        let labels = pattern.tracks.iter().map(|t| t.name.clone()).collect();
        let values = (0..pattern.tracks.len()).map(|i| i.to_string()).collect();
        let selected = self.roll_track().unwrap_or(0);
        layout.items.push(Item::new(Some(Id::RollTrack), rect, Kind::Select { values, labels, selected }));
    }

    /// The header's track list: show track `t`.
    pub(super) fn select_roll_track(&mut self, t: usize) {
        if self.track(t).is_none() {
            return;
        }
        self.selection.cursor = Some(Cursor { track: t, beat: 0, step: 0 });
        self.selection.selected = None;
        self.selection.roll_zone = None;
        self.selection.note_focus = Some(NoteFocus::Track(t));
        self.roll.cursor = None;
        self.roll.top = None;
        self.roll.gauge = None;
    }

    /// The note of track `t` at `step` on `row`.
    pub(super) fn roll_note_at(&self, t: usize, step: i64, row: i32) -> Option<usize> {
        let track = self.track(t)?;
        let spb = steps_per_beat(track);
        let midi = f64::from(piano_roll::row_midi(row));
        track
            .notes
            .iter()
            .position(|n| absolute_step(n.beat, n.step, spb) == step && MIDDLE_C + track.transpose + n.pitch == midi)
    }

    /// The pitch a row has on track `t`, relative to the track.
    pub(super) fn roll_pitch(&self, t: usize, row: i32) -> f64 {
        f64::from(piano_roll::row_midi(row)) - MIDDLE_C - self.track(t).map_or(0.0, |track| track.transpose)
    }

    pub(super) fn roll_beat(&self, t: usize, step: i64) -> (i32, i32) {
        let spb = i64::from(self.track(t).map_or(4, steps_per_beat));
        ((step / spb) as i32, (step % spb) as i32)
    }

    /// Select note `n` of track `t`, the cursor on it.
    pub(super) fn roll_select(&mut self, t: usize, n: usize) {
        self.selection.selected = Some((t, n));
        self.roll_follow();
    }

    /// The steps of the pattern, on a track of `spb` steps per beat.
    pub(super) fn roll_step_count(&self, spb: i32) -> i64 {
        i64::from(self.pattern().map_or(4, |p| p.beat_count.max(1))) * i64::from(spb)
    }

    /// The cursor and the page on the selected note (it follows its
    /// transposition and its moves in time).
    pub(super) fn roll_follow(&mut self) {
        let Some((t, n)) = self.selection.selected else { return };
        let Some(track) = self.track(t) else { return };
        let Some(note) = track.notes.get(n) else { return };
        let spb = steps_per_beat(track);
        let step = absolute_step(note.beat, note.step, spb);
        let row = note_row(note, track);
        self.roll.cursor = Some((step, row));
        self.roll_reveal(step, spb);
    }

    /// Bring `step` of a track of `spb` steps per beat in sight.
    fn roll_reveal(&mut self, step: i64, spb: i32) {
        let beat = (step / i64::from(spb)) as i32;
        self.roll.window.reveal(self.roll_shown_beats(), beat);
    }

    /// Add a note at `step`, `row` and select it.
    pub(super) fn roll_add(&mut self, t: usize, step: i64, row: i32) -> Option<usize> {
        let (beat, beat_step) = self.roll_beat(t, step);
        let n = self.editor.add_note(self.pattern_index, t, beat, beat_step, self.roll_pitch(t, row))?;
        self.selection.selected = Some((t, n));
        self.roll.cursor = Some((step, row));
        // The click that adds a note is not the first of a double-click on it.
        self.roll.last_note_click = None;
        self.edited();
        Some(n)
    }

    /// Delete note `n` of track `t`.
    pub(super) fn roll_delete_note(&mut self, t: usize, n: usize) -> bool {
        let Some(note) = self.track(t).and_then(|track| track.notes.get(n)).cloned() else { return false };
        let deleted = self.editor.delete_note(self.pattern_index, t, note.beat, note.step, note.pitch);
        if deleted {
            if self.selection.selected == Some((t, n)) {
                self.selection.selected = None;
            } else if let Some((st, sn)) = self.selection.selected
                && st == t
                && sn > n
            {
                self.selection.selected = Some((st, sn - 1));
            }
            self.edited();
        }
        deleted
    }

    // ─── The mouse ───────────────────────────────────────────────────────────

    /// A press on the piano roll; whether it was its.
    pub fn roll_click(&mut self, size: (i32, i32, Scale), x: i32, y: i32, button: Button) -> bool {
        let layout = self.roll_layout(size);
        let Some(view) = self.roll_view(&layout) else { return false };
        let Some(hit) = layout.hit(&view, x, y) else { return false };
        // A note drawn out covers the cells after its own: the note under the pointer first.
        let drawn = layout.note_at(&view, x, y);
        let Some(t) = self.roll_track() else { return false };
        match (hit, button) {
            (RollHit::Ruler, Button::Left) => return self.press_pattern_ruler(size, x, y),
            (RollHit::Ruler, Button::Right) => return self.pattern_ruler_menu(size, x, y),
            _ => {}
        }
        let bar = match hit {
            RollHit::ScrollBar(edge) => Some((bar_hold(&layout, &view, edge), bar_at(&layout, &view, x))),
            _ => None,
        };
        self.roll.gauge = None;
        // A press on a note of the zone may drag it: the zone stays.
        let on_zone_note = button == Button::Left
            && matches!(hit, RollHit::Cell { step, row } if drawn
                .or_else(|| self.roll_note_at(t, step, row).map(|n| (n, false)))
                .is_some_and(|(n, edge)| !edge && self.roll_zone().is_some_and(|z| self.roll_zone_holds_note(&z, n))));
        if matches!(hit, RollHit::Cell { .. }) && !on_zone_note {
            self.selection.roll_zone = None;
        }
        match (hit, button) {
            (RollHit::ScrollBar(_), Button::Left) => {
                self.roll.bar_drag = bar.map(|(hold, _)| hold);
                self.roll_bar_held(bar.map_or((0, 0.0, 1), |(_, at)| at));
            }
            (RollHit::Key(midi), Button::Left) => self.roll_play_key(t, midi),
            (RollHit::Key(midi), Button::Right) => self.roll_key_menu(t, midi, x, y),
            (RollHit::Cell { step, row }, Button::Left) => {
                match drawn.or_else(|| self.roll_note_at(t, step, row).map(|n| (n, false))) {
                    // A press on the right edge of a note draws it out.
                    Some((n, true)) => self.roll_length_press(t, n, x, layout.cell),
                    // A press on a note: a drag, or a click when it does not move.
                    Some((n, false)) => {
                        if !self.roll_zone_move_press(n, (step, row), (x, y)) {
                            self.roll_note_press(t, n, (x, y), (layout.cell, layout.row));
                        }
                    }
                    // A press on an empty cell: a click adds a note, a drag draws a zone.
                    None => self.roll_zone_press(step, row, (x, y), true),
                }
            }
            (RollHit::Cell { step, row }, Button::Right) => self.roll_grid_menu(t, step, row, x, y),
            _ => return false,
        }
        true
    }

    /// The mouse moved while a note is pressed; whether to redraw.
    pub fn roll_drag_move(&mut self, x: i32, y: i32) -> bool {
        if self.roll.length_drag.is_some() {
            return self.roll_length_move(x);
        }
        self.roll_note_move(x, y)
    }

    /// The press ended: a press that did not drag is a click on its note
    /// (`session_roll_move.rs`).
    pub(super) fn roll_release(&mut self) {
        self.roll.bar_drag = None;
        self.roll_zone_release();
        self.roll_zone_move_release();
        if self.roll.length_drag.take().is_some() {
            self.roll.gauge = None;
            return;
        }
        self.roll_note_release();
    }

    /// The wheel over the piano roll: the keyboard; Ctrl zooms the beats
    /// around the pointer, Shift scrolls them; whether it was its.
    pub fn roll_wheel(
        &mut self,
        size: (i32, i32, Scale),
        (x, y): (i32, i32),
        lines: isize,
        (zoom, shift): (bool, bool),
    ) -> bool {
        let layout = self.roll_layout(size);
        if self.workspace != Workspace::PianoRoll
            || lines == 0
            || !(layout.grid.contains(x, y) || layout.keys.contains(x, y) || layout.bar.contains(x, y))
        {
            return false;
        }
        let Some(view) = self.roll_view(&layout) else { return false };
        let shown = (layout.shown(&view) as i32, view.beat_count);
        if zoom {
            let bar = (layout.bar.x, layout.bar.w);
            self.roll.window.zoom_at(lines, shown, (layout.beat_under(&view, x), x), bar);
        } else if shift {
            self.roll.window.scroll_by(lines, shown.0, shown.1);
        } else {
            self.roll.top = Some(layout.clamp_top(view.top_row + lines as i32 * WHEEL_ROWS));
        }
        true
    }

    /// The note under the mouse, for its tooltip; whether it changed.
    pub fn roll_hover(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let hover = (self.workspace == Workspace::PianoRoll && self.roll.drag.is_none())
            .then(|| {
                let layout = self.roll_layout(size);
                let view = self.roll_view(&layout)?;
                (0..view.track.notes.len()).find(|&n| layout.note_rect(&view, n).is_some_and(|r| r.contains(x, y)))
            })
            .flatten();
        let changed = hover != self.roll.hover;
        self.roll.hover = hover;
        changed
    }

    /// The bubble over a note: the drag's value (the gauge), else the hovered
    /// note's tooltip.
    pub(super) fn draw_roll_tooltip(&self, canvas: &mut Canvas, scale: Scale) {
        if self.chrome.covered() {
            return;
        }
        let size = (canvas.width(), canvas.height(), scale);
        let layout = self.roll_layout(size);
        let Some(view) = self.roll_view(&layout) else { return };
        let (n, text) = match (&self.roll.gauge, self.selection.selected, self.roll.hover) {
            (Some(gauge), Some((_, n)), _) => (n, gauge.clone()),
            (None, _, Some(n)) => match view.track.notes.get(n) {
                Some(note) => (n, editors::note_tooltip(note, view.track.transpose)),
                None => return,
            },
            _ => return,
        };
        if let Some(rect) = layout.note_rect(&view, n) {
            view::draw_tooltip(canvas, &text, rect, scale);
        }
    }

    /// What the gauge shows over note `n` while its pitch changes.
    pub(super) fn pitch_gauge(&self, t: usize, n: usize) -> String {
        let Some(track) = self.track(t) else { return String::new() };
        let Some(note) = track.notes.get(n) else { return String::new() };
        let midi = (MIDDLE_C + track.transpose + note.pitch).round() as i32;
        format!("pitch {}", midi_name(midi))
    }

    /// A key clicked: play that pitch on the track.
    fn roll_play_key(&mut self, t: usize, midi: i32) {
        let note = Note {
            pitch: f64::from(midi) - MIDDLE_C - self.track(t).map_or(0.0, |track| track.transpose),
            ..Note::default()
        };
        if let Some(controller) = self.playback.controller_mut() {
            controller.preview_note(self.editor.library(), self.pattern_index, t, Some(&note));
        }
        self.message = midi_name(midi);
    }

    // ─── The keyboard ────────────────────────────────────────────────────────

    /// An arrow: the cursor (wrapping, the page following), or with Shift the
    /// selected note's pitch (↑↓) and its time (←→).
    pub fn roll_arrow(&mut self, direction: Move, shift: bool) {
        let Some(t) = self.roll_track() else { return };
        if shift && self.selection.selected.is_some_and(|(st, _)| st == t) {
            self.nudge(direction);
            self.roll_follow();
            let pitch = matches!(direction, Move::Up | Move::Down);
            if let Some((_, n)) = self.selection.selected {
                self.roll.gauge = pitch.then(|| self.pitch_gauge(t, n));
            }
            return;
        }
        self.roll.gauge = None;
        let Some(spb) = self.track(t).map(steps_per_beat) else { return };
        let total = self.roll_step_count(spb);
        let page_start = i64::from(self.roll.window.first() * spb);
        let middle = TOTAL_KEYS - 1 - TOTAL_KEYS / 2;
        let (step, row) = match (direction, self.roll.cursor) {
            (Move::Left, None) => ((page_start - 1).rem_euclid(total), middle),
            (Move::Right, None) => ((page_start + 1).rem_euclid(total), middle),
            (Move::Up, None) => (page_start, 0),
            (Move::Down, None) => (page_start, TOTAL_KEYS - 1),
            (Move::Left, Some((s, r))) => ((s - 1).rem_euclid(total), r),
            (Move::Right, Some((s, r))) => ((s + 1).rem_euclid(total), r),
            (Move::Up, Some((s, r))) => (s, (r + 1).rem_euclid(TOTAL_KEYS)),
            (Move::Down, Some((s, r))) => (s, (r - 1).rem_euclid(TOTAL_KEYS)),
        };
        self.roll.cursor = Some((step, row));
        // The page follows, the note there is selected.
        self.roll_reveal(step, spb);
        self.selection.selected = self.roll_note_at(t, step, row).map(|n| (t, n));
        self.roll_show_row(row);
        let (beat, beat_step) = self.roll_beat(t, step);
        self.message = format!("{} {}", beat_label(beat, beat_step), midi_name(piano_roll::row_midi(row)));
    }

    /// Scroll so that `row` is on screen.
    fn roll_show_row(&mut self, row: i32) {
        let layout = self.roll_layout(self.chrome.panel_size);
        let Some(top) = self.roll_view(&layout).map(|v| v.top_row) else { return };
        let rows = layout.visible_rows();
        if row > top {
            self.roll.top = Some(layout.clamp_top(row));
        } else if row <= top - rows {
            self.roll.top = Some(layout.clamp_top(row + rows - 1));
        }
    }

    /// Enter: at the cursor, add a note, select the one there, or delete it
    /// when it is selected.
    pub fn roll_enter(&mut self) {
        let (Some(t), Some((step, row))) = (self.roll_track(), self.roll.cursor) else { return };
        match self.roll_note_at(t, step, row) {
            Some(n) if self.selection.selected == Some((t, n)) => {
                self.roll_delete_note(t, n);
            }
            Some(n) => self.selection.selected = Some((t, n)),
            None => {
                self.roll_add(t, step, row);
            }
        }
    }

    /// Esc: no zone, no selection and no cursor, else back to the grid.
    pub fn roll_escape(&mut self) {
        if self.selection.selected.is_some() || self.roll.cursor.is_some() || self.selection.roll_zone.is_some() {
            self.selection.roll_zone = None;
            self.selection.selected = None;
            self.roll.cursor = None;
            self.roll.gauge = None;
        } else {
            self.toggle_piano_roll();
        }
    }

    /// Delete: the zone's notes, else the selected note.
    pub fn roll_delete(&mut self) {
        if let Some(zone) = self.roll_zone() {
            self.undoable(|s| s.roll_delete_zone(&zone));
        } else if let Some((t, n)) = self.selection.selected {
            self.roll_delete_note(t, n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::OverlayKey;
    use super::super::session_panel::PanelKey;
    use super::*;
    use crate::kits::KitSet;
    use ordrumbox_core::Library;

    use crate::test_support::SIZE;

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","beatCount":8,"tracks":[{"name":"KICK","notes":[{}]},
                {"name":"BASS","pitch":-12,"beatCount":8,"notes":[{"beat":1,"pitch":3}]}]}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session.select_roll_track(1);
        session.toggle_piano_roll();
        session
    }

    /// The middle of the cell at `step`, `row`.
    fn cell(s: &Session, step: i64, row: i32) -> (i32, i32) {
        let layout = s.roll_layout(SIZE);
        let view = s.roll_view(&layout).unwrap();
        let (start, _) = view.page_steps();
        let x = layout.grid.x + (step - start) as i32 * layout.cell + layout.cell / 2;
        let y = layout.grid.y + (view.top_row - row) * layout.row + layout.row / 2;
        (x, y)
    }

    fn notes(s: &Session) -> Vec<(i32, i32, f64, f64)> {
        s.track(1).unwrap().notes.iter().map(|n| (n.beat, n.step, n.pitch, n.velocity)).collect()
    }

    /// Option `i` of the open menu.
    fn choose(s: &mut Session, i: usize) {
        for _ in 0..i {
            s.panel_key(SIZE, PanelKey::Down, false, false);
        }
        s.panel_key(SIZE, PanelKey::Enter, false, false);
    }

    #[test]
    fn clicks_add_and_select() {
        let mut s = session();
        assert_eq!(s.roll_title(), "Piano Roll - BASS");
        // BASS's note: beat 1 (step 4), pitch 3 on a track an octave down: MIDI 51, row 39.
        let (x, y) = cell(&s, 4, 39);
        assert!(s.roll_click(SIZE, x, y, Button::Left));
        s.mouse_up();
        assert_eq!(s.selection.selected, Some((1, 0)));
        // An empty cell: a note there, selected.
        let (x, y) = cell(&s, 6, 41);
        s.roll_click(SIZE, x, y, Button::Left);
        assert_eq!(notes(&s).len(), 1, "a note is added once the click is over");
        s.mouse_up();
        assert_eq!(notes(&s)[1], (1, 2, 5.0, 0.8));
        assert_eq!(s.selection.selected, Some((1, 1)));
        // A click right after, on the note just added: it stays, selected.
        s.mouse_up();
        s.roll_click(SIZE, x, y, Button::Left);
        s.mouse_up();
        assert_eq!((notes(&s).len(), s.selection.selected), (2, Some((1, 1))));
    }

    #[test]
    fn keys_move_the_cursor_and_edit() {
        let mut s = session();
        s.roll_arrow(Move::Right, false);
        assert_eq!(s.roll.cursor, Some((1, TOTAL_KEYS - 1 - TOTAL_KEYS / 2)));
        s.roll_enter();
        assert_eq!(notes(&s).len(), 2);
        s.roll_arrow(Move::Up, true);
        assert_eq!(notes(&s)[1].2, 13.0, "Shift+Up: a semitone up");
        s.roll_arrow(Move::Right, true);
        assert_eq!(notes(&s)[1], (0, 2, 13.0, 0.8), "Shift+Right: one step later");
        s.roll_delete();
        assert_eq!(notes(&s).len(), 1);
        // Left from step 0 wraps to the last step, on page 2.
        s.roll.cursor = Some((0, 40));
        s.roll_arrow(Move::Left, false);
        assert_eq!((s.roll.cursor, s.roll.window.first()), (Some((31, 40)), 4));
    }

    #[test]
    fn menus_write_chords_and_progressions() {
        let mut s = session();
        let (x, y) = cell(&s, 0, 36);
        s.roll_click(SIZE, x, y, Button::Right);
        choose(&mut s, 2); // Add major chord
        assert_eq!(&notes(&s)[1..], [(0, 0, 0.0, 0.8), (0, 0, 4.0, 0.8), (0, 0, 7.0, 0.8)]);
        assert_eq!(s.message, "Added major chord (3 notes) - BASS @ beat 1.1");
        s.roll_click(SIZE, x, y, Button::Right);
        choose(&mut s, 2);
        assert_eq!(s.message, "Chord already present");
        // The keyboard at C3 (the track's 0): I-V-vi-IV over 8 beats.
        let layout = s.roll_layout(SIZE);
        s.roll_click(SIZE, layout.keys.x + 4, y, Button::Right);
        choose(&mut s, 1);
        let pitches: Vec<(i32, f64)> = notes(&s).iter().map(|n| (n.0, n.2)).collect();
        assert_eq!(pitches, [(0, 0.0), (0, 4.0), (0, 7.0), (4, 7.0), (4, 11.0), (4, 14.0)]);
        // The next one is the next progression.
        s.roll_click(SIZE, layout.keys.x + 4, y, Button::Right);
        assert!(s.chrome.widgets.dropdown.as_ref().unwrap().title.as_deref().unwrap().ends_with("I-vi-IV-V"));
        choose(&mut s, 0); // Clear all
        s.overlay_key(OverlayKey::Enter);
        assert!(notes(&s).is_empty());
    }
}
