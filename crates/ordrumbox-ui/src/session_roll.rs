//! The session's side of the piano roll (`piano_roll_panel.js` and its
//! sections), on the track editor's track: a click on an empty cell adds a
//! note there, on a note selects it (and plays it), on the selected note
//! deletes it; dragging a note changes its pitch (up, down) or its velocity
//! (sideways), one undo step per drag; a click on a key plays it. Right-click
//! menus: on the keyboard, clear the track or write a chord progression from
//! that key; on the grid, add or delete the note, add a minor or major
//! chord. Keys: arrows move the cursor (the page follows), Shift+arrows
//! change the selected note's velocity (up, down) and pitch (left, right),
//! Enter adds, selects or deletes at the cursor, Delete deletes the selected
//! note. The wheel scrolls the keyboard (Shift: the pages).

use crate::scale::Scale;
use ordrumbox_core::json::Value;
use ordrumbox_core::note_positions::sub_positions;
use ordrumbox_core::num;
use ordrumbox_core::pattern_engine::SpanResolver;
use ordrumbox_core::steps::{TICK, absolute_step};
use ordrumbox_logic::composition;

use super::*;
use crate::editors;
use crate::note_drag::{Axis, DragEvent, MIDDLE_C, MIDI_MIN, NoteDrag, midi_name};
use crate::panel::{Item, Kind, Rect};
use crate::piano_roll::{self, BEATS_PER_PAGE, RollHit, RollLayout, RollView, TOTAL_KEYS, note_row, steps_per_beat};

/// A note being dragged in the piano roll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RollDrag {
    track: usize,
    note: usize,
    drag: NoteDrag,
}

/// The piano roll's view and what the mouse holds in it.
#[derive(Debug, Default)]
pub(super) struct RollState {
    /// The page shown.
    page: i32,
    /// Highest row shown; `None`: centered on the track's pitch.
    top: Option<i32>,
    /// The keyboard cursor: step, row.
    pub(super) cursor: Option<(i64, i32)>,
    /// The note pressed.
    drag: Option<RollDrag>,
    /// The edited value shown over the note.
    gauge: Option<String>,
    /// The next chord progression.
    sequence: i64,
    /// The note under the mouse.
    hover: Option<usize>,
    /// A Shift+press on a cell, which may become a zone.
    pub(super) zone_drag: Option<super::session_roll_zone::RollZoneDrag>,
}

impl RollState {
    /// A note is held by the mouse.
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Let go of the note held, if any.
    pub(super) fn release(&mut self) {
        self.drag = None;
        self.zone_drag = None;
    }
}

/// Rows a notch of the wheel scrolls.
const WHEEL_ROWS: i32 = 3;

/// A chord type offered by the piano roll's right-click menu.
#[derive(Debug, Clone, Copy)]
struct RollChord {
    /// Key used in the menu value (`"{key}:{step}:{pitch}"`) and in the
    /// result message (`"Added {key} chord"`).
    key: &'static str,
    /// Label shown in the menu (`"Add {key} chord"`).
    label: &'static str,
    /// Intervals in semitones from the root.
    intervals: &'static [f64],
}

/// The chord types generated from a right-click on the grid: the common
/// triads first, then the enriched (seventh) ones.
const ROLL_CHORDS: &[RollChord] = &[
    RollChord { key: "minor", label: "Add minor chord", intervals: &[0.0, 3.0, 7.0] },
    RollChord { key: "major", label: "Add major chord", intervals: &[0.0, 4.0, 7.0] },
    RollChord { key: "diminished", label: "Add diminished chord", intervals: &[0.0, 3.0, 6.0] },
    RollChord { key: "major7", label: "Add major7 chord", intervals: &[0.0, 4.0, 7.0, 11.0] },
    RollChord { key: "minor7", label: "Add minor7 chord", intervals: &[0.0, 3.0, 7.0, 10.0] },
    RollChord { key: "sus4", label: "Add sus4 chord", intervals: &[0.0, 5.0, 7.0] },
    RollChord { key: "seventh", label: "Add seventh chord", intervals: &[0.0, 4.0, 7.0, 10.0] },
];

impl Session {
    // ─── State ───────────────────────────────────────────────────────────────

    /// The piano roll is shown (the kit editor opens over it).
    pub fn piano_roll_open(&self) -> bool {
        self.workspace == Workspace::PianoRoll && self.kit_editor.is_none()
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
        RollLayout::new(body.main_w, body.editor_h, size.2, self.status.len(), spb)
    }

    /// The step playing on the roll's track, while the pattern plays.
    fn roll_playhead(&self) -> Option<i64> {
        if !self.playback.plays(Mode::Pattern(self.pattern_index)) {
            return None;
        }
        let (pattern, track) = (self.pattern()?, self.track(self.roll_track()?)?);
        let ticks = u64::from(TICK) * u64::try_from(pattern.beat_count.max(1)).ok()?;
        let tick = self.playback.tick()? % ticks;
        let spb = steps_per_beat(track);
        let in_beat = (tick % u64::from(TICK)) as f64 / (f64::from(TICK) / f64::from(spb));
        Some((tick / u64::from(TICK)) as i64 * i64::from(spb) + in_beat.floor() as i64)
    }

    /// What the piano roll shows: while playing, the page the playhead is on.
    pub(super) fn roll_view(&self, layout: &RollLayout) -> Option<RollView<'_>> {
        let t = self.roll_track()?;
        let (pattern, track) = (self.pattern()?, self.track(t)?);
        let playhead = self.roll_playhead();
        let mut view = RollView {
            track,
            color: crate::theme::track(t),
            beat_count: pattern.beat_count.max(1),
            page: 0,
            top_row: self.roll.top.map_or_else(
                || layout.centered_top((MIDDLE_C + track.transpose - MIDI_MIN).round() as i32),
                |top| layout.clamp_top(top),
            ),
            selected: self.selection.selected.filter(|&(st, _)| st == t).map(|(_, n)| n),
            cursor: self.roll.cursor,
            playhead,
            subs: Vec::new(),
            zone: self.roll_zone().map(|zone| self.roll_span(&zone, track)),
        };
        let per_page = i64::from(BEATS_PER_PAGE * steps_per_beat(track));
        view.page = match playhead {
            Some(step) => (step / per_page) as i32,
            None => self.roll.page,
        }
        .clamp(0, view.pages() - 1);
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
    fn roll_pitch(&self, t: usize, row: i32) -> f64 {
        f64::from(piano_roll::row_midi(row)) - MIDDLE_C - self.track(t).map_or(0.0, |track| track.transpose)
    }

    fn roll_beat(&self, t: usize, step: i64) -> (i32, i32) {
        let spb = i64::from(self.track(t).map_or(4, steps_per_beat));
        ((step / spb) as i32, (step % spb) as i32)
    }

    /// Select note `n` of track `t`, the cursor on it.
    pub(super) fn roll_select(&mut self, t: usize, n: usize) {
        self.selection.selected = Some((t, n));
        self.roll_follow();
    }

    /// The cursor and the page on the selected note (it follows its
    /// transposition and its moves in time).
    fn roll_follow(&mut self) {
        let Some((t, n)) = self.selection.selected else { return };
        let Some(track) = self.track(t) else { return };
        let Some(note) = track.notes.get(n) else { return };
        let spb = steps_per_beat(track);
        let step = absolute_step(note.beat, note.step, spb);
        let row = note_row(note, track);
        self.roll.cursor = Some((step, row));
        self.roll.page = (step / i64::from(BEATS_PER_PAGE * spb)) as i32;
    }

    /// Add a note at `step`, `row` and select it.
    fn roll_add(&mut self, t: usize, step: i64, row: i32) -> Option<usize> {
        let (beat, beat_step) = self.roll_beat(t, step);
        let n = self.editor.add_note(self.pattern_index, t, beat, beat_step, self.roll_pitch(t, row))?;
        self.selection.selected = Some((t, n));
        self.roll.cursor = Some((step, row));
        self.edited();
        Some(n)
    }

    /// Delete note `n` of track `t`.
    fn roll_delete_note(&mut self, t: usize, n: usize) -> bool {
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
        let (page, pages) = (view.page, view.pages());
        let Some(hit) = layout.hit(&view, x, y) else { return false };
        let Some(t) = self.roll_track() else { return false };
        self.roll.gauge = None;
        if matches!(hit, RollHit::Cell { .. }) {
            self.selection.roll_zone = None;
        }
        match (hit, button) {
            (RollHit::PreviousPage, Button::Left) => self.roll.page = (page - 1).max(0),
            (RollHit::NextPage, Button::Left) => self.roll.page = (page + 1).min(pages - 1),
            (RollHit::Key(midi), Button::Left) => self.roll_play_key(t, midi),
            (RollHit::Key(midi), Button::Right) => self.roll_key_menu(t, midi, x, y),
            (RollHit::Cell { step, row }, Button::Left) => match self.roll_note_at(t, step, row) {
                // A press on a note: a drag, or a click when it does not move.
                Some(n) => {
                    let pitch = self.track(t).map_or(0.0, |track| track.transpose);
                    self.roll.drag = Some(RollDrag { track: t, note: n, drag: NoteDrag::new(x, y, pitch) });
                }
                None => {
                    if let Some(n) = self.roll_add(t, step, row) {
                        self.audition(t, Some(n));
                    }
                }
            },
            (RollHit::Cell { step, row }, Button::Right) => self.roll_grid_menu(t, step, row, x, y),
            _ => return false,
        }
        true
    }

    /// The mouse moved while a note is pressed; whether to redraw.
    pub fn roll_drag_move(&mut self, x: i32, y: i32) -> bool {
        let Some(RollDrag { track: t, note: n, mut drag }) = self.roll.drag else { return false };
        let Some(note) = self.track(t).and_then(|track| track.notes.get(n)).cloned() else { return false };
        let events = drag.moved(x, y, &note);
        self.roll.drag = Some(RollDrag { track: t, note: n, drag });
        for event in &events {
            match *event {
                // A drag makes no click: the note is selected here.
                DragEvent::Start(..) => self.roll_select(t, n),
                DragEvent::Value(axis, value, _) => {
                    let name = self.track(t).map(|track| track.name.clone()).unwrap_or_default();
                    let desc = format!("Edit note {} on {name}", axis.key());
                    let updates = [(axis.key(), Value::Number(value))];
                    if self.editor.update_note(self.pattern_index, t, n, &updates, Some(&desc), true) {
                        self.edited();
                        self.audition(t, Some(n));
                    }
                    self.roll_follow();
                    self.roll.gauge = Some(self.gauge_text(t, n, axis));
                }
            }
        }
        !events.is_empty()
    }

    /// The press ended: a press that did not drag is a click on its note —
    /// it selects it, or deletes it when it was selected.
    pub(super) fn roll_release(&mut self) {
        let Some(RollDrag { track: t, note: n, drag }) = self.roll.drag.take() else { return };
        self.roll.gauge = None;
        if drag.axis().is_some() {
            return;
        }
        if self.selection.selected == Some((t, n)) {
            self.roll_delete_note(t, n);
        } else if self.track(t).is_some_and(|track| n < track.notes.len()) {
            self.roll_select(t, n);
            self.audition(t, Some(n));
        }
    }

    /// The wheel over the piano roll: the keyboard (Shift: the pages);
    /// whether it was its.
    pub fn roll_wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, lines: isize, shift: bool) -> bool {
        let layout = self.roll_layout(size);
        if self.workspace != Workspace::PianoRoll
            || lines == 0
            || !(layout.grid.contains(x, y) || layout.keys.contains(x, y))
        {
            return false;
        }
        let Some(view) = self.roll_view(&layout) else { return false };
        if shift {
            self.roll.page = (view.page - lines.signum() as i32).clamp(0, view.pages() - 1);
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

    /// What the gauge shows for note `n` edited along `axis`.
    fn gauge_text(&self, t: usize, n: usize, axis: Axis) -> String {
        let Some(track) = self.track(t) else { return String::new() };
        let Some(note) = track.notes.get(n) else { return String::new() };
        match axis {
            Axis::Velocity => format!("velocity {}", num::text(note.velocity)),
            Axis::Pitch => {
                let midi = (MIDDLE_C + track.transpose + note.pitch).round() as i32;
                format!("pitch {}", midi_name(midi))
            }
        }
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
            let axis = match direction {
                Move::Up | Move::Down => Some(Axis::Pitch),
                _ => None,
            };
            if let Some((_, n)) = self.selection.selected {
                self.roll.gauge = axis.map(|axis| self.gauge_text(t, n, axis));
            }
            return;
        }
        self.roll.gauge = None;
        let Some(spb) = self.track(t).map(steps_per_beat) else { return };
        let total = i64::from(self.pattern().map_or(4, |p| p.beat_count.max(1))) * i64::from(spb);
        let page_start = i64::from(self.roll.page * BEATS_PER_PAGE * spb);
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
        // `#syncCursor`: the page follows, the note there is selected.
        self.roll.page = (step / i64::from(BEATS_PER_PAGE * spb)) as i32;
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

    // ─── The menus ───────────────────────────────────────────────────────────

    /// The keyboard: clear the track, or write the next progression from
    /// that key.
    fn roll_key_menu(&mut self, t: usize, midi: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let progression = composition::progression(self.roll.sequence);
        let title = format!("{} - {} {}", track.name, midi_name(midi), progression.name);
        let tonic = self.roll_pitch(t, midi - MIDI_MIN as i32);
        let values = vec!["clear".into(), format!("sequence:{tonic}")];
        let labels = vec!["Clear all".into(), "Add sequence".into()];
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::RollMenu, x, y, title, values, labels));
    }

    /// A cell: add or delete its note, add a chord on it
    /// (`#showGridContextMenu`; what cannot run is left out).
    fn roll_grid_menu(&mut self, t: usize, step: i64, row: i32, x: i32, y: i32) {
        let Some(track) = self.track(t) else { return };
        let (beat, beat_step) = self.roll_beat(t, step);
        let title = format!("{} @ {}.{}", track.name, beat + 1, beat_step + 1);
        let pitch = self.roll_pitch(t, row);
        let at = format!("{step}:{pitch}");
        let (mut values, mut labels) = (Vec::new(), Vec::new());
        match self.roll_note_at(t, step, row) {
            None => {
                values.push(format!("add:{at}"));
                labels.push("Add note".to_owned());
            }
            Some(n) => {
                values.push(format!("delete:{n}"));
                labels.push("Delete note".to_owned());
            }
        }
        for chord in ROLL_CHORDS {
            values.push(format!("{}:{at}", chord.key));
            labels.push(chord.label.to_owned());
        }
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::RollMenu, x, y, title, values, labels));
    }

    /// An action of a piano roll menu.
    pub(super) fn roll_menu_pick(&mut self, value: &str) {
        let Some(t) = self.roll_track() else { return };
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return };
        let parts: Vec<&str> = value.split(':').collect();
        let number = |i: usize| parts.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        match parts[0] {
            "add" => {
                let (step, pitch) = (number(1) as i64, number(2));
                let row =
                    (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + pitch - MIDI_MIN).round() as i32;
                if let Some(n) = self.roll_add(t, step, row) {
                    self.audition(t, Some(n));
                    let (beat, beat_step) = self.roll_beat(t, step);
                    self.message =
                        format!("Added note (pitch {}) - {name} @ {}", num::text(pitch), beat_label(beat, beat_step));
                }
            }
            "delete" => {
                let n = number(1) as usize;
                let Some(note) = self.track(t).and_then(|track| track.notes.get(n)).cloned() else { return };
                if self.roll_delete_note(t, n) {
                    self.message = format!("Deleted note - {name} @ {}", beat_label(note.beat, note.step));
                }
            }
            key if ROLL_CHORDS.iter().any(|c| c.key == key) => {
                let (step, root) = (number(1) as i64, number(2));
                if let Some(chord) = ROLL_CHORDS.iter().find(|c| c.key == key) {
                    self.roll_chord(t, step, root, chord.intervals, chord.key);
                }
            }
            "clear" => self.clear_track(t),
            "sequence" => self.roll_sequence_from(t, number(1)),
            _ => {}
        }
    }

    /// Add the chord's notes the step has not got, the
    /// first selected.
    fn roll_chord(&mut self, t: usize, step: i64, root: f64, intervals: &[f64], quality: &str) {
        let (beat, beat_step) = self.roll_beat(t, step);
        let Some(track) = self.track(t) else { return };
        let name = track.name.clone();
        let existing: Vec<f64> =
            track.notes.iter().filter(|n| n.beat == beat && n.step == beat_step).map(|n| n.pitch).collect();
        let mut added = Vec::new();
        for interval in intervals {
            let pitch = root + interval;
            if !existing.contains(&pitch)
                && let Some(n) = self.editor.add_note(self.pattern_index, t, beat, beat_step, pitch)
            {
                added.push(n);
            }
        }
        let Some(&first) = added.first() else {
            self.message = "Chord already present".into();
            return;
        };
        self.edited();
        self.selection.selected = Some((t, first));
        let row = (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + root - MIDI_MIN).round() as i32;
        self.roll.cursor = Some((step, row));
        self.message =
            format!("Added {quality} chord ({}) - {name} @ {}", notes_label(added.len()), beat_label(beat, beat_step));
    }

    /// Replace the track's notes with the next progression from `tonic`,
    /// a chord per measure.
    fn roll_sequence_from(&mut self, t: usize, tonic: f64) {
        let progression = composition::progression(self.roll.sequence);
        let beat_count = self.pattern().map_or(4, |p| p.beat_count);
        let planned = composition::progression_notes(progression, tonic, beat_count);
        if planned.is_empty() {
            return;
        }
        if self.track(t).is_some_and(|track| !track.notes.is_empty()) {
            self.editor.clean_track(self.pattern_index, t);
        }
        let mut first = None;
        for &(beat, pitch) in &planned {
            let n = self.editor.add_note(self.pattern_index, t, beat, 0, pitch);
            first = first.or(n);
        }
        self.roll.sequence += 1;
        self.edited();
        self.selection.selected = first.map(|n| (t, n));
        let row = (MIDDLE_C + self.track(t).map_or(0.0, |track| track.transpose) + tonic - MIDI_MIN).round() as i32;
        self.roll.cursor = Some((0, row));
        let name = self.track(t).map(|track| track.name.clone()).unwrap_or_default();
        self.message = format!(
            "Replaced with sequence \"{}\" ({}, 1 chord/measure) - {name}",
            progression.name,
            notes_label(planned.len())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;

    const SIZE: (i32, i32, Scale) = (1100, 680, Scale::ONE);

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
    fn clicks_add_select_and_delete() {
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
        assert_eq!(notes(&s)[1], (1, 2, 5.0, 0.8));
        assert_eq!(s.selection.selected, Some((1, 1)));
        // A click on the selected note deletes it.
        s.roll_click(SIZE, x, y, Button::Left);
        s.mouse_up();
        assert_eq!(notes(&s).len(), 1);
        assert_eq!(s.selection.selected, None);
    }

    #[test]
    fn a_drag_edits_one_parameter_in_one_undo_step() {
        let mut s = session();
        s.editor.set_clock(Some(0.0));
        let (x, y) = cell(&s, 4, 39);
        s.roll_click(SIZE, x, y, Button::Left);
        // Up and down: the pitch, semitone by semitone (the roll's vertical axis).
        for dy in [2, 10, 40, 41] {
            s.roll_drag_move(x + 1, y - dy);
        }
        s.mouse_up();
        assert_eq!(notes(&s), [(1, 0, 6.0, 0.8)]);
        // Dragged, not clicked: still there and selected.
        assert_eq!(s.selection.selected, Some((1, 0)));
        // Sideways: the velocity, to the right louder (the note moved up: row 42).
        let (x, y) = cell(&s, 4, 42);
        s.roll_click(SIZE, x, y, Button::Left);
        s.roll_drag_move(x + 30, y);
        s.mouse_up();
        assert_eq!(notes(&s), [(1, 0, 6.0, 0.95)]);
        s.undo();
        assert_eq!(notes(&s), [(1, 0, 6.0, 0.8)]);
        s.undo();
        assert_eq!(notes(&s), [(1, 0, 3.0, 0.8)]);
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
        assert_eq!((s.roll.cursor, s.roll.page), (Some((31, 40)), 1));
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
