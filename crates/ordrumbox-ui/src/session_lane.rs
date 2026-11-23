//! The lane under the grid: one note setting of the track editor's track
//! (velocity, chance, retriggers, pan, the parameter locks...:
//! `editors::LANES`), a bar per step. A press on a bar sets the setting of
//! the notes of its step to the height pressed; a drag paints the bars it
//! crosses, the steps between two moves taking the values between, all of it
//! one undo step. A parameter lock painted to the bottom is cleared. A bubble
//! gives the value being painted, or that of the bar under the pointer; the
//! right button opens a menu to put a step's notes, or all the track's, back
//! to the setting's default.

use crate::canvas::Canvas;
use crate::scale::Scale;
use ordrumbox_core::Note;
use ordrumbox_core::steps::absolute_step;

use super::Session;
use crate::editors::{self, LANES, Prop};
use crate::panel::{Dropdown, Id, Rect};
use crate::view;

/// A press on the lane, held: the track and the setting it paints, and the
/// last step painted and its value.
#[derive(Debug, Clone, Copy)]
pub(super) struct LaneDrag {
    track: usize,
    prop: &'static Prop,
    step: i64,
    value: f64,
}

/// The lane's top edge pressed: where, and how tall the lane was (scale units).
#[derive(Debug, Clone, Copy)]
pub(super) struct LaneResize {
    y: i32,
    height: i32,
    scale: Scale,
}

impl Session {
    /// A press at `(x, y)` on the strip above the lane takes its top edge,
    /// which sets its height; whether it was there.
    pub fn grid_lane_resize_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(layout) = self.grid_layout(size) else { return false };
        if !layout.lane_edge_rect().contains(x, y) {
            return false;
        }
        let scale = size.2;
        let height = scale.units(layout.lane_rect().h);
        self.chrome.lane_resize = Some(LaneResize { y, height, scale });
        true
    }

    /// The mouse moved to `y` with the lane's top edge held: the lane as
    /// much taller as the pointer went up; whether to redraw.
    pub(super) fn grid_lane_resize_move(&mut self, y: i32) -> bool {
        let Some(LaneResize { y: from, height, scale }) = self.chrome.lane_resize else { return false };
        let moved = scale.units(from - y);
        let wanted = Some((height + moved).clamp(view::lane_height(), view::MAX_LANE_SET));
        let changed = wanted != self.chrome.lane_height;
        self.chrome.lane_height = wanted;
        changed
    }

    /// The lane's setting (velocity, chance...).
    fn lane_prop(&self) -> Option<&'static Prop> {
        LANES.get(usize::from(self.editors.lane)).and_then(|(key, _)| editors::note_prop(key))
    }

    /// A press at `(x, y)` on the lane's bars: their notes take the value
    /// pressed, and a drag begins; whether it was the lane's.
    pub fn lane_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(layout) = self.grid_layout(size) else { return false };
        let (Some(t), Some(prop)) = (self.editor_track(), self.lane_prop()) else { return false };
        let Some(track) = self.track(t) else { return false };
        let Some(step) = layout.lane_step(x, y, view::steps_per_beat(track)) else { return false };
        let value = lane_value(prop, layout.lane_fraction(y));
        self.chrome.lane_drag = Some(LaneDrag { track: t, prop, step, value });
        self.lane_paint(&layout, t, prop, [(step, value), (step, value)]);
        true
    }

    /// The mouse moved with the lane pressed: the bars from the last step
    /// painted to the one under `(x, y)`; whether to redraw.
    pub(super) fn lane_drag_move(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(drag) = self.chrome.lane_drag else { return false };
        let Some(layout) = self.grid_layout(size) else { return false };
        let Some(track) = self.track(drag.track) else { return false };
        let step = layout.lane_step_clamped(x, view::steps_per_beat(track));
        let value = lane_value(drag.prop, layout.lane_fraction(y));
        if (step, value) == (drag.step, drag.value) {
            return false;
        }
        self.chrome.lane_drag = Some(LaneDrag { step, value, ..drag });
        self.lane_paint(&layout, drag.track, drag.prop, [(drag.step, drag.value), (step, value)]);
        true
    }

    /// The bubble over the lane: the value being painted, else the value of
    /// the bar under the pointer; whether one was drawn.
    pub(super) fn draw_lane_tooltip(&self, canvas: &mut Canvas, scale: Scale) -> bool {
        if self.chrome.covered() {
            return false;
        }
        let Some(layout) = self.grid_layout((canvas.width(), canvas.height(), scale)) else { return false };
        let (Some(t), Some(prop)) = (self.editor_track(), self.lane_prop()) else { return false };
        let Some(track) = self.track(t) else { return false };
        let (x, y) = self.chrome.widgets.pointer;
        let spb = view::steps_per_beat(track);
        let value = if let Some(drag) = self.chrome.lane_drag {
            lane_setting(prop, drag.value)
        } else {
            let Some(step) = layout.lane_step(x, y, spb) else { return false };
            let at = i64::from(layout.start_beat()) * i64::from(spb) + step;
            let note = track.notes.iter().find(|n| absolute_step(n.beat, n.step, spb) == at);
            let Some(note) = note else { return false };
            editors::note_lane_value(note, prop)
        };
        let text = match value {
            Some(value) => format!("{} {}", prop.label, prop.format.value(value, "")),
            None => format!("{} off", prop.label),
        };
        let lane = layout.lane_rect();
        view::draw_tooltip(canvas, &text, Rect::new(x, lane.y, 0, lane.h), scale);
        true
    }

    /// A right press at `(x, y)` on the lane's bars: its menu opens; whether
    /// it was the lane's.
    pub fn grid_lane_menu(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(layout) = self.grid_layout(size) else { return false };
        let (Some(t), Some(prop)) = (self.editor_track(), self.lane_prop()) else { return false };
        let Some(track) = self.track(t) else { return false };
        let spb = view::steps_per_beat(track);
        let Some(step) = layout.lane_step(x, y, spb) else { return false };
        let at = i64::from(layout.start_beat()) * i64::from(spb) + step;
        let title = format!("{} - {}", track.name, prop.label);
        let lines = [
            (format!("step:{at}"), "Reset this step".to_owned()),
            ("all".to_owned(), "Reset all notes".to_owned()),
            ("auto-height".to_owned(), "Automatic height".to_owned()),
        ];
        self.chrome.widgets.dropdown = Some(Dropdown::menu(Id::LaneMenu, x, y, title, lines));
        true
    }

    /// An action of the lane's menu: `step:N` puts the notes of absolute
    /// step `N`, `all` every note of the track, back to the default setting.
    pub(super) fn lane_menu_pick(&mut self, value: &str) {
        if value == "auto-height" {
            self.chrome.lane_height = None;
            return;
        }
        let (Some(t), Some(prop)) = (self.editor_track(), self.lane_prop()) else { return };
        let Some(track) = self.track(t) else { return };
        let spb = view::steps_per_beat(track);
        let only = match value.split_once(':') {
            Some(("step", at)) => match at.parse::<i64>() {
                Ok(at) => Some(at),
                Err(_) => return,
            },
            _ => None,
        };
        let default = lane_default(prop);
        let values: Vec<(usize, Option<f64>)> = track
            .notes
            .iter()
            .enumerate()
            .filter(|(_, note)| only.is_none_or(|at| absolute_step(note.beat, note.step, spb) == at))
            .map(|(n, _)| (n, default))
            .collect();
        let desc = format!("Reset note {} on {}", prop.label.to_lowercase(), track.name);
        if self.editor.set_note_values(self.pattern_index, t, prop.key, &values, &desc, false) {
            self.edited();
        }
        self.message = format!("{} reset on {}", prop.label.to_lowercase(), notes_count(values.len()));
    }

    /// The notes of track `t` on the page's steps from `from` to `to` (each
    /// a step of the page and a value) take the values between, each its
    /// step's.
    fn lane_paint(&mut self, layout: &view::Layout, t: usize, prop: &'static Prop, [from, to]: [(i64, f64); 2]) {
        let Some(track) = self.track(t) else { return };
        let spb = view::steps_per_beat(track);
        let start = i64::from(layout.start_beat()) * i64::from(spb);
        let (low, high) = (from.0.min(to.0), from.0.max(to.0));
        let values: Vec<(usize, Option<f64>)> = track
            .notes
            .iter()
            .enumerate()
            .filter_map(|(n, note)| {
                let step = absolute_step(note.beat, note.step, spb) - start;
                (low..=high).contains(&step).then(|| (n, lane_setting(prop, between(from, to, step, prop))))
            })
            .collect();
        if values.is_empty() {
            return;
        }
        let desc = format!("Edit note {} on {}", prop.label.to_lowercase(), track.name);
        if self.editor.set_note_values(self.pattern_index, t, prop.key, &values, &desc, true) {
            self.edited();
        }
    }
}

/// The default of lane setting `prop`: a parameter lock has none (`None`).
fn lane_default(prop: &Prop) -> Option<f64> {
    if editors::is_lock(prop) {
        None
    } else {
        Some(Note::default().field(prop.key).and_then(|v| v.as_f64()).unwrap_or(prop.min))
    }
}

/// `1 note`, `3 notes`.
fn notes_count(count: usize) -> String {
    if count == 1 { "1 note".to_owned() } else { format!("{count} notes") }
}

/// What a note's `prop` is set to by a painted `value`: a parameter lock at
/// the bottom of the lane is cleared (`None`).
fn lane_setting(prop: &Prop, value: f64) -> Option<f64> {
    (!(editors::is_lock(prop) && value <= prop.min)).then_some(value)
}

/// The value of setting `prop` at `fraction` of the lane's height, on its steps.
fn lane_value(prop: &Prop, fraction: f64) -> f64 {
    snap(prop, prop.min + fraction * (prop.max - prop.min))
}

/// `value` on the steps of `prop`, in its range.
fn snap(prop: &Prop, value: f64) -> f64 {
    let stepped = if prop.step > 0.0 { (value / prop.step).round() * prop.step } else { value };
    // Two decimals at most, as the note's settings are saved.
    ((stepped * 100.0).round() / 100.0).clamp(prop.min, prop.max)
}

/// The value at `step` on the line from `from` to `to` (step, value).
fn between(from: (i64, f64), to: (i64, f64), step: i64, prop: &Prop) -> f64 {
    if from.0 == to.0 {
        return to.1;
    }
    let at = (step - from.0) as f64 / (to.0 - from.0) as f64;
    snap(prop, from.1 + at * (to.1 - from.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::view::Cursor;
    use ordrumbox_core::Library;

    use crate::test_support::{SIZE, click};

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[
                {"name":"KICK","notes":[{"beat":0},{"beat":1},{"beat":2},{"beat":3}]},
                {"name":"SNARE","notes":[{"beat":1}]}]}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        s.editor.set_clock(Some(0.0));
        s
    }

    /// The middle of the bar of beat `beat` (a track of 4 steps per beat), at
    /// `fraction` of the lane's height.
    fn bar(s: &Session, beat: i32, fraction: f64) -> (i32, i32) {
        let layout = s.grid_layout(SIZE).expect("the grid is shown");
        let lane = layout.lane_rect();
        let x = layout.cell_rect(0, beat, 0, s.track(0).unwrap()).x + 2;
        let y = lane.y + lane.h - (fraction * f64::from(lane.h)) as i32;
        (x, y.min(lane.y + lane.h - 1))
    }

    fn velocities(s: &Session, t: usize) -> Vec<f64> {
        s.track(t).unwrap().notes.iter().map(|n| n.velocity).collect()
    }

    #[test]
    fn a_press_sets_the_notes_of_its_step() {
        let mut s = session();
        let (x, y) = bar(&s, 1, 0.5);
        assert!(s.lane_press(SIZE, x, y));
        s.mouse_up();
        let v = velocities(&s, 0);
        assert!((v[1] - 0.5).abs() <= 0.02, "half the lane's height: {v:?}");
        assert_eq!([v[0], v[2], v[3]], [0.8; 3], "only the step pressed, of the first track without a cursor");
        assert_eq!(velocities(&s, 1), [0.8]);
    }

    #[test]
    fn a_drag_paints_the_steps_between_in_one_undo_step() {
        let mut s = session();
        let (x0, y0) = bar(&s, 0, 0.0);
        let (x3, y3) = bar(&s, 3, 1.0);
        assert!(s.lane_press(SIZE, x0, y0));
        assert!(s.mouse_move(SIZE, x3, y3, false));
        s.mouse_up();
        let v = velocities(&s, 0);
        assert_eq!((v[0], v[3]), (0.0, 1.0));
        assert!(v[0] < v[1] && v[1] < v[2] && v[2] < v[3], "the steps between take the values between: {v:?}");
        s.undo();
        assert_eq!(velocities(&s, 0), [0.8; 4]);
    }

    #[test]
    fn the_lane_follows_its_tab_and_the_cursor() {
        let mut s = session();
        s.selection.cursor = Some(Cursor { track: 1, beat: 0, step: 0 });
        let layout = s.grid_layout(SIZE).expect("the grid is shown");
        let tab = layout.lane_tab_rect(2);
        assert!(click(&mut s, tab.x + 2, tab.y + 2));
        assert_eq!(s.editors.lane, 2);
        let (x, y) = bar(&s, 1, 1.0);
        assert!(s.lane_press(SIZE, x, y));
        s.mouse_up();
        assert_eq!(s.track(1).unwrap().notes[0].retrigger_count, 16);
        assert_eq!(s.track(0).unwrap().notes[1].retrigger_count, 1);
    }

    #[test]
    fn a_lock_is_set_by_a_bar_and_cleared_at_the_bottom() {
        let mut s = session();
        s.editors.lane = LANES.iter().position(|(key, _)| *key == "filterLock").expect("a filter lane") as u8;
        let locks = |s: &Session| s.track(0).unwrap().notes.iter().map(|n| n.filter_lock).collect::<Vec<_>>();
        assert_eq!(locks(&s), [None; 4], "no lock to begin with");
        let (x, y) = bar(&s, 1, 0.5);
        assert!(s.lane_press(SIZE, x, y));
        s.mouse_up();
        let locked = locks(&s);
        assert!(locked[1].is_some_and(|v| (v - 0.5).abs() <= 0.02), "half the lane's height: {locked:?}");
        assert_eq!([locked[0], locked[2], locked[3]], [None; 3], "only the step pressed");
        // A second gesture, past the coalescing window: its own undo step.
        s.editor.set_clock(Some(1000.0));
        let (x, y) = bar(&s, 1, 0.0);
        assert!(s.lane_press(SIZE, x, y));
        s.mouse_up();
        assert_eq!(locks(&s), [None; 4], "painted to the bottom, the lock is cleared");
        s.undo();
        assert!(locks(&s)[1].is_some(), "the clearing is one undo step");
    }

    #[test]
    fn a_press_beside_the_bars_is_not_the_lanes() {
        let mut s = session();
        let layout = s.grid_layout(SIZE).expect("the grid is shown");
        let lane = layout.lane_rect();
        assert!(!s.lane_press(SIZE, lane.x - 2, lane.y + 2));
        assert!(!s.lane_press(SIZE, lane.x + 2, lane.y - 2));
        assert!(layout.lane_tab_rect(0).y > layout.add_track_rect().y, "under the add-track button");
    }

    #[test]
    fn the_tabs_stay_put_and_the_bars_take_the_room_left() {
        let mut s = session();
        let before = s.grid_layout(SIZE).expect("the grid is shown");
        let tabs: Vec<_> = (0..LANES.len()).map(|i| before.lane_tab_rect(i)).collect();
        let lane = before.lane_rect();
        assert!(lane.y >= before.add_track_rect().y + before.add_track_rect().h, "under the add-track button");
        assert!(lane.y <= tabs[0].y && lane.y + lane.h == tabs[LANES.len() - 1].y + tabs[LANES.len() - 1].h);
        for _ in 0..12 {
            s.add_track_named("CLAP");
        }
        let after = s.grid_layout(SIZE).expect("the grid is shown");
        let moved: Vec<_> = (0..LANES.len()).map(|i| after.lane_tab_rect(i)).collect();
        assert_eq!(tabs, moved, "the tabs never move");
        assert!(after.lane_rect().h <= lane.h && after.lane_rect().h >= tabs[0].h * view::lane_tab_rows());
        assert!(after.lane_rect().y >= after.add_track_rect().y + after.add_track_rect().h);
    }

    /// Option `i` of the open menu.
    fn choose(s: &mut Session, i: usize) {
        use super::super::session_panel::PanelKey;
        for _ in 0..i {
            s.panel_key(SIZE, PanelKey::Down, false, false);
        }
        s.panel_key(SIZE, PanelKey::Enter, false, false);
    }

    #[test]
    fn the_pitch_lane_sets_whole_semitones_around_the_middle() {
        let mut s = session();
        s.editors.lane = LANES.iter().position(|(key, _)| *key == "pitch").expect("a pitch lane") as u8;
        let (x, y) = bar(&s, 1, 1.0);
        assert!(s.lane_press(SIZE, x, y));
        s.mouse_up();
        let pitches: Vec<f64> = s.track(0).unwrap().notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [0.0, 24.0, 0.0, 0.0]);
    }

    #[test]
    fn the_menu_resets_a_step_or_all_the_notes_in_one_undo_step() {
        let mut s = session();
        for beat in [0, 1, 2] {
            let (x, y) = bar(&s, beat, 0.2);
            assert!(s.lane_press(SIZE, x, y));
            s.mouse_up();
            s.editor.set_clock(Some(1000.0 * f64::from(beat + 1)));
        }
        assert!(velocities(&s, 0)[..3].iter().all(|v| *v < 0.3));
        let (x, y) = bar(&s, 1, 0.5);
        assert!(s.grid_lane_menu(SIZE, x, y));
        choose(&mut s, 0);
        assert_eq!(velocities(&s, 0)[1], 0.8, "this step is back to the default");
        assert!(velocities(&s, 0)[0] < 0.3, "the others stay");
        assert!(s.grid_lane_menu(SIZE, x, y));
        choose(&mut s, 1);
        assert_eq!(velocities(&s, 0), [0.8; 4]);
        s.undo();
        assert_eq!(velocities(&s, 0)[1], 0.8);
        assert!(velocities(&s, 0)[0] < 0.3, "one undo step for the whole reset");
        assert!(!s.grid_lane_menu(SIZE, 1, 1), "outside the lane, no lane menu");
    }

    #[test]
    fn a_reset_clears_a_lock() {
        let mut s = session();
        s.editors.lane = LANES.iter().position(|(key, _)| *key == "filterLock").expect("a filter lane") as u8;
        let (x, y) = bar(&s, 1, 0.5);
        assert!(s.lane_press(SIZE, x, y));
        s.mouse_up();
        assert!(s.track(0).unwrap().notes[1].filter_lock.is_some());
        assert!(s.grid_lane_menu(SIZE, x, y));
        choose(&mut s, 1);
        assert!(s.track(0).unwrap().notes.iter().all(|n| n.filter_lock.is_none()));
    }

    /// Whether the lane's bubble was drawn, over an empty window.
    fn bubble(s: &Session) -> bool {
        let (w, h, scale) = SIZE;
        let mut pixels = vec![0; (w * h) as usize];
        let drawn = s.draw_lane_tooltip(&mut Canvas::new(&mut pixels, w as usize, h as usize), scale);
        assert_eq!(drawn, pixels.iter().any(|p| *p != 0), "it draws what it says");
        drawn
    }

    #[test]
    fn a_bubble_gives_the_value_painted_or_under_the_pointer() {
        let mut s = session();
        let (x, y) = bar(&s, 1, 0.5);
        s.chrome.widgets.pointer = (x, y);
        assert!(bubble(&s), "over a bar");
        // Over a step without a note: nothing.
        let layout = s.grid_layout(SIZE).unwrap();
        s.chrome.widgets.pointer = (layout.cell_rect(0, 1, 2, s.track(0).unwrap()).x + 2, y);
        assert!(!bubble(&s));
        // While painting, the value being set.
        assert!(s.lane_press(SIZE, x, y));
        assert!(bubble(&s));
    }

    #[test]
    fn a_drag_of_the_top_edge_sets_the_lane_height_and_the_menu_resets_it() {
        let mut s = session();
        let before = s.grid_layout(SIZE).unwrap();
        let edge = before.lane_edge_rect();
        let (x, y) = (edge.x + edge.w / 2, edge.y + edge.h / 2);
        assert!(s.grid_lane_resize_press(SIZE, x, y));
        assert!(s.grid_lane_resize_move(y - 40), "going up makes it taller");
        s.mouse_up();
        let after = s.grid_layout(SIZE).unwrap();
        let (b, a) = (before.lane_rect().h, after.lane_rect().h);
        assert!(a > b || a == b && s.chrome.lane_height.is_some(), "taller, or already as tall as the room allows");
        // Tabs keep their place whatever the bars' height.
        assert_eq!(before.lane_tab_rect(3), after.lane_tab_rect(3));
        // Down as far as it goes: the tabs' height.
        assert!(s.grid_lane_resize_press(SIZE, x, after.lane_edge_rect().y + 1));
        s.grid_lane_resize_move(5000);
        s.mouse_up();
        assert_eq!(s.chrome.lane_height, Some(view::lane_height()));
        assert!(s.grid_layout(SIZE).unwrap().lane_rect().h <= view::lane_height() * SIZE.2);
        let (x, y) = bar(&s, 1, 0.5);
        assert!(s.grid_lane_menu(SIZE, x, y));
        choose(&mut s, 2);
        assert_eq!(s.chrome.lane_height, None, "automatic again");
        assert_eq!(s.grid_layout(SIZE).unwrap().lane_rect(), before.lane_rect());
    }
}
