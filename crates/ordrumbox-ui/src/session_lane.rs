//! The lane under the grid: one note setting of the track editor's track
//! (velocity, chance, retriggers, pan: `editors::LANES`), a bar per step. A
//! press on a bar sets the setting of the notes of its step to the height
//! pressed; a drag paints the bars it crosses, the steps between two moves
//! taking the values between, all of it one undo step.

use crate::scale::Scale;
use ordrumbox_core::steps::absolute_step;

use super::Session;
use crate::editors::{self, LANES, Prop};
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

impl Session {
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

    /// The notes of track `t` on the page's steps from `from` to `to` (each
    /// a step of the page and a value) take the values between, each its
    /// step's.
    fn lane_paint(&mut self, layout: &view::Layout, t: usize, prop: &'static Prop, [from, to]: [(i64, f64); 2]) {
        let Some(track) = self.track(t) else { return };
        let spb = view::steps_per_beat(track);
        let start = i64::from(layout.start_beat()) * i64::from(spb);
        let (low, high) = (from.0.min(to.0), from.0.max(to.0));
        let values: Vec<(usize, f64)> = track
            .notes
            .iter()
            .enumerate()
            .filter_map(|(n, note)| {
                let step = absolute_step(note.beat, note.step, spb) - start;
                (low..=high).contains(&step).then(|| (n, between(from, to, step, prop)))
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
}
