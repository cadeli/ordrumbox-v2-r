//! The song panel's automation lanes: the add-automation button opens the
//! instruments the song plays, each opening the controls a lane can switch
//! (Mute, Solo, Volume); the one picked adds its lane. A press on a lane's
//! step and a drag along it paint the steps selected, and no other: on a
//! switch's lane, on where the step pressed was not on, off where it was; on
//! a level's lane, a line from the level pressed to the one the pointer is
//! at; with the right button, erased (as the pattern has it). The zone is whole measures, beats or
//! steps, as fine as the zoom shows them; Shift held, steps. A click (left
//! or right) on its name opens its menu: clear its points, delete it, or
//! delete every lane. Each edit, a whole drag included, is one undo step.

use crate::scale::Scale;
use ordrumbox_core::automation::{
    AutomationKind, AutomationLane, AutomationStroke, AutomationTarget, STEPS_PER_MEASURE, song_instruments,
    switch_value,
};

use super::Session;
use crate::panel::{Dropdown, Id, SUBMENU_MARK};
use crate::song_view::SongHit;
use crate::ui_input::Button;

/// The values of the automation menus' lines.
const INSTRUMENT_VALUE: &str = "instrument:";
const LANE_VALUE: &str = "lane:";
const REMOVE_VALUE: &str = "remove:";
const CLEAR_VALUE: &str = "clear:";
const REMOVE_ALL_VALUE: &str = "remove-all";

/// A press on a lane's step, which a drag turns into a painted zone.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LanePaint {
    lane: usize,
    /// The step pressed and the one the pointer is on.
    from: u32,
    to: u32,
    /// The steps the zone is made of at the zoom pressed (a measure's, a
    /// beat's or one), and whether Shift makes them one.
    snap: u32,
    is_fine: bool,
    /// What the zone is painted with.
    brush: Brush,
    /// The lane as pressed: each move paints it anew.
    base: AutomationLane,
    /// The undo key of this press's edits.
    key: String,
}

/// What a press on a lane paints.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Brush {
    /// One value: a switch's (off when the pressed step was on, else on), or
    /// `None` (erased) with the right button.
    Fill(Option<f64>),
    /// A level's lane: a line from the level pressed to the pointer's, over
    /// the lane's band `(top, height)`.
    Line { band: (i32, i32), from: f64, to: f64 },
}

impl LanePaint {
    /// The stroke it paints now.
    fn stroke(&self) -> AutomationStroke {
        match self.brush {
            Brush::Fill(value) => AutomationStroke::Fill(value),
            Brush::Line { from, to, .. } => AutomationStroke::Line((self.from, from), (self.to, to)),
        }
    }
}

impl Session {
    /// The automation lanes of the arrangement shown.
    pub(super) fn song_lane_count(&self) -> usize {
        self.song().map_or(0, |song| song.automations.len())
    }

    /// A click at `(x, y)` on the add-automation button, a lane's name or one of its steps.
    pub(super) fn automation_click(
        &mut self,
        size: (i32, i32, Scale),
        hit: SongHit,
        (x, y): (i32, i32),
        button: Button,
    ) {
        match (hit, button) {
            (SongHit::AddAutomation, Button::Left) => self.automation_menu(x, y),
            (SongHit::LaneName(lane), _) => self.lane_menu(lane, x, y),
            (SongHit::LaneStep { lane, step }, _) => self.press_lane(size, lane, (step, y), button, false),
            _ => {}
        }
    }

    /// A press on `step` of lane `lane`, at `y` (`is_fine`: Shift held),
    /// which paints its zone at once.
    pub(super) fn press_lane(
        &mut self,
        size: (i32, i32, Scale),
        lane: usize,
        (step, y): (u32, i32),
        button: Button,
        is_fine: bool,
    ) {
        let Some(base) = self.song().and_then(|song| song.automations.get(lane)).cloned() else { return };
        let layout = self.song_layout(size);
        let brush = match (button, base.target.kind()) {
            (Button::Right, _) => Brush::Fill(None),
            (_, AutomationKind::Switch) => Brush::Fill(Some(switch_value(base.is_on_at(step) != Some(true)))),
            (_, AutomationKind::Level) => {
                let band = layout.lane_band_at(y);
                let level = layout.level_at(band, y);
                Brush::Line { band, from: level, to: level }
            }
        };
        let paint = LanePaint {
            lane,
            from: step,
            to: step,
            snap: layout.snap_steps(),
            is_fine,
            brush,
            base,
            key: format!("lane-paint:{lane}:{}", self.editor.revision()),
        };
        self.paint_lane(&paint);
        self.song_panel.paint = Some(paint);
    }

    /// The pointer at `(x, y)` while a lane is pressed (`shift` held): the
    /// zone from the pressed step to the one under it painted (on a level's
    /// lane, to the level at `y`); whether it changed.
    pub(super) fn lane_paint_move(&mut self, size: (i32, i32, Scale), (x, y): (i32, i32), shift: bool) -> bool {
        let layout = self.song_layout(size);
        let first = self.song_scroll(&layout, self.song_rows().len() + self.song_lane_count()).0;
        let step = layout.step_at(x, first).min(self.song_step_count() - 1);
        let Some(paint) = &mut self.song_panel.paint else { return false };
        let brush = match paint.brush {
            Brush::Line { band, from, .. } => Brush::Line { band, from, to: layout.level_at(band, y) },
            fill @ Brush::Fill(_) => fill,
        };
        if (step, shift, brush) == (paint.to, paint.is_fine, paint.brush) {
            return false;
        }
        (paint.to, paint.is_fine, paint.brush) = (step, shift, brush);
        let paint = paint.clone();
        self.paint_lane(&paint);
        true
    }

    /// The steps of the arrangement shown, a measure at least.
    fn song_step_count(&self) -> u32 {
        self.song().map_or(1, |song| song.length_measures().max(1.0).ceil() as u32) * STEPS_PER_MEASURE
    }

    /// The zone of `paint`, widened to whole snaps, painted on its lane as it was pressed.
    fn paint_lane(&mut self, paint: &LanePaint) {
        let snap = if paint.is_fine { 1 } else { paint.snap.max(1) };
        let (first, last) = (paint.from.min(paint.to), paint.from.max(paint.to));
        let zone = (first / snap * snap, (last / snap * snap + snap - 1).min(self.song_step_count() - 1));
        match self.editor.paint_automation(None, paint.lane, &paint.base, zone, paint.stroke(), &paint.key) {
            Ok(()) => self.edited(),
            Err(message) => self.message = message,
        }
    }

    /// The lane released: the zone stays as painted; whether one was pressed.
    pub fn lane_paint_release(&mut self) -> bool {
        self.song_panel.paint.take().is_some()
    }

    /// The instruments the song plays, at `(x, y)`: each opens its controls.
    fn automation_menu(&mut self, x: i32, y: i32) {
        let Some(song) = self.song() else { return };
        let names = song_instruments(song, &self.editor.library().patterns);
        if names.is_empty() {
            self.message = "The song's patterns have no tracks to automate".into();
            return;
        }
        let values = names.iter().map(|name| format!("{INSTRUMENT_VALUE}{name}")).collect();
        let labels = names.iter().map(|name| format!("{name}{SUBMENU_MARK}")).collect();
        let menu = Dropdown::menu(Id::AutomationMenu, x, y, "New automation".into(), values, labels);
        self.chrome.widgets.dropdown = Some(menu);
    }

    /// Lane `lane`'s menu at `(x, y)`: clear its points, delete it, delete every lane.
    fn lane_menu(&mut self, lane: usize, x: i32, y: i32) {
        let Some(lane_shown) = self.song().and_then(|song| song.automations.get(lane)) else { return };
        let title = format!("{} {}", lane_shown.target.label(), lane_shown.instrument);
        let values = vec![format!("{CLEAR_VALUE}{lane}"), format!("{REMOVE_VALUE}{lane}"), REMOVE_ALL_VALUE.into()];
        let labels = vec!["Clear all points".into(), "Delete lane".into(), "Delete all lanes".into()];
        let menu = Dropdown::menu(Id::AutomationMenu, x, y, title, values, labels);
        self.chrome.widgets.dropdown = Some(menu);
    }

    /// A line of the automation menus, picked in `menu`: an instrument opens
    /// its controls beside it, a control adds its lane, the lane's menu
    /// clears or removes it, or removes every lane.
    pub(super) fn automation_menu_pick(&mut self, value: &str, menu: Dropdown) {
        if let Some(instrument) = value.strip_prefix(INSTRUMENT_VALUE) {
            let values = AutomationTarget::ALL.map(|t| format!("{LANE_VALUE}{}:{instrument}", t.key())).into();
            let labels = AutomationTarget::ALL.map(|t| t.label().to_owned()).into();
            let size = self.chrome.panel_size;
            let submenu = Dropdown::submenu(menu, size, Id::AutomationMenu, instrument.to_owned(), values, labels);
            self.chrome.widgets.dropdown = Some(submenu);
        } else if let Some((key, instrument)) = value.strip_prefix(LANE_VALUE).and_then(|v| v.split_once(':')) {
            let Some(target) = AutomationTarget::from_key(key) else { return };
            match self.editor.add_automation_lane(None, instrument, target) {
                Ok(_) => {
                    self.edited();
                    let how = match target.kind() {
                        AutomationKind::Switch => "drag on it to switch it on or off",
                        AutomationKind::Level => "drag on it to draw its level",
                    };
                    self.message = format!("{} lane of {instrument} added: {how}, right-drag to erase", target.label());
                }
                Err(message) => self.message = message,
            }
        } else if let Some(lane) = value.strip_prefix(REMOVE_VALUE).and_then(|l| l.parse().ok()) {
            self.undoable(|s| {
                if s.editor.remove_automation_lane(None, lane) {
                    s.edited();
                }
            });
        } else if let Some(lane) = value.strip_prefix(CLEAR_VALUE).and_then(|l| l.parse().ok()) {
            self.undoable(|s| {
                if s.editor.clear_automation_lane(None, lane) {
                    s.edited();
                }
            });
        } else if value == REMOVE_ALL_VALUE {
            self.undoable(|s| {
                if s.editor.remove_automation_lanes(None) {
                    s.edited();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, choose, menu, session};
    use super::Session;
    use crate::song_view::{MeasureZoom, SongHit};
    use crate::ui_input::Button;
    use ordrumbox_core::automation::AutomationTarget;

    const ON: Option<f64> = Some(1.0);
    const OFF: Option<f64> = Some(0.0);

    fn points(s: &Session) -> Vec<(u32, Option<f64>)> {
        s.song().unwrap().automations[0].points().iter().map(|p| (p.step, p.value)).collect()
    }

    /// X of `step` of the first lane, and Y of its row.
    fn step_at(s: &Session, step: u32) -> (i32, i32) {
        find(s, |hit| matches!(hit, SongHit::LaneStep { lane: 0, step: at } if at == step))
    }

    /// A press on step `from` of the first lane with `button`, dragged to step `to`.
    fn drag(s: &mut Session, (from, to): (u32, u32), button: Button) {
        let ((from, y), (to, _)) = (step_at(s, from), step_at(s, to));
        assert!(s.song_click(SIZE, from, y, button));
        assert!(s.mouse_move(SIZE, to, y + 30, false), "the row under the pointer does not matter");
        assert!(s.lane_paint_release());
    }

    /// A session whose song has a lane of KICK's `target`.
    fn with_lane(target: AutomationTarget) -> Session {
        let mut s = session();
        s.editor.add_track(1, "KICK", 4);
        s.editor.add_automation_lane(None, "KICK", target).unwrap();
        s.set_size(SIZE);
        s
    }

    /// Where `hit` is on the song panel, scanning it.
    fn find(s: &Session, wanted: impl Fn(SongHit) -> bool) -> (i32, i32) {
        let layout = s.song_layout(SIZE);
        let rows = s.song_rows();
        let view = s.song_view_data(&rows, &layout);
        let g = layout.grid;
        (g.y..g.y + g.h)
            .flat_map(|y| (g.x..g.x + g.w).map(move |x| (x, y)))
            .find(|&(x, y)| layout.hit(&view, x, y).is_some_and(&wanted))
            .expect("on the panel")
    }

    #[test]
    fn the_button_adds_a_lane_whose_steps_switch_on_and_off() {
        let mut s = session();
        for name in ["SNARE", "KICK", "SNARE"] {
            s.editor.add_track(1, name, 4);
        }
        s.set_size(SIZE);
        let (x, y) = find(&s, |hit| hit == SongHit::AddAutomation);
        assert!(s.song_click(SIZE, x, y, Button::Left));
        assert_eq!(menu(&s), ["instrument:KICK", "instrument:SNARE"], "the song's instruments, once each");
        choose(&mut s, 1);
        assert_eq!(menu(&s), ["lane:mute:SNARE", "lane:solo:SNARE", "lane:volume:SNARE"]);
        choose(&mut s, 1);
        let lanes = &s.song().unwrap().automations;
        assert_eq!((lanes[0].instrument.as_str(), lanes[0].target), ("SNARE", AutomationTarget::Solo));

        // The lane's row is under the patterns', the button under it.
        let (lx, ly) = find(&s, |hit| matches!(hit, SongHit::LaneStep { lane: 0, step: 0 }));
        assert!(ly > y - 20, "after the pattern rows");
        assert!(find(&s, |hit| hit == SongHit::AddAutomation).1 > ly);
        assert!(s.song_click(SIZE, lx, ly, Button::Left));
        assert!(s.song_release(SIZE));
        assert_eq!(points(&s), [(0, ON), (1, None)], "a click: that step on, no more");
        assert!(s.song_click(SIZE, lx, ly, Button::Left));
        assert!(s.song_release(SIZE));
        assert_eq!(points(&s), [(0, OFF), (1, None)], "clicked again: off");
        s.undo();
        s.undo();
        assert!(points(&s).is_empty());

        // A drag paints the steps it crosses, in one undo step.
        drag(&mut s, (2, 9), Button::Left);
        assert_eq!(points(&s), [(2, ON), (10, None)], "from an empty step: the zone on, no more");
        drag(&mut s, (4, 6), Button::Left);
        assert_eq!(points(&s), [(2, ON), (4, OFF), (7, ON), (10, None)], "from an on step: the zone off");
        s.undo();
        assert_eq!(points(&s), [(2, ON), (10, None)], "a drag is one undo step");
        // The right button erases: the pattern's own state again.
        drag(&mut s, (5, 3), Button::Right);
        assert_eq!(points(&s), [(2, ON), (3, None), (6, ON), (10, None)]);
        s.undo();
        s.undo();
        assert!(points(&s).is_empty());

        // A right-click on its name opens its menu: clear it, delete it, delete every lane.
        let (nx, ny) = find(&s, |hit| hit == SongHit::LaneName(0));
        assert!(s.song_click(SIZE, nx, ny, Button::Right));
        assert_eq!(menu(&s), ["clear:0", "remove:0", "remove-all"]);
        choose(&mut s, 1);
        assert!(s.song().unwrap().automations.is_empty());
    }

    #[test]
    fn a_press_paints_what_the_zoom_shows_and_shift_a_step() {
        let mut s = with_lane(AutomationTarget::Mute);
        s.song_panel.zoom = MeasureZoom::new(24);
        assert_eq!(s.song_layout(SIZE).snap_steps(), 16, "measures only on the ruler");
        let (x, y) = step_at(&s, 5);
        assert!(s.song_click(SIZE, x, y, Button::Left));
        assert!(s.lane_paint_release());
        assert_eq!(points(&s), [(0, ON), (16, None)], "the whole measure");
        assert!(s.song_shift_click(SIZE, x, y));
        assert!(s.lane_paint_release());
        assert_eq!(points(&s), [(0, ON), (5, OFF), (6, ON), (16, None)], "Shift: the step alone");

        s.song_panel.zoom = MeasureZoom::new(40);
        assert_eq!(s.song_layout(SIZE).snap_steps(), 4, "beats marked");
        s.undo();
        s.undo();
        drag(&mut s, (5, 9), Button::Left);
        assert_eq!(points(&s), [(4, ON), (12, None)], "whole beats");
    }

    #[test]
    fn a_drag_on_a_volume_lane_draws_a_line_by_height() {
        let mut s = with_lane(AutomationTarget::Volume);
        let ((from, y), (to, _)) = (step_at(&s, 0), step_at(&s, 4));
        let (top, height) = s.song_layout(SIZE).lane_band_at(y);
        assert!(s.song_click(SIZE, from, top + height, Button::Left));
        assert_eq!(points(&s), [(0, OFF), (1, None)], "pressed at the bottom of the band: 0");
        assert!(s.mouse_move(SIZE, to, top - 40, false));
        assert!(s.lane_paint_release());
        let expected = [(0, OFF), (1, Some(0.25)), (2, Some(0.5)), (3, Some(0.75)), (4, ON), (5, None)];
        assert_eq!(points(&s), expected, "up to the top, and above it: the whole level");
        s.undo();
        assert!(points(&s).is_empty(), "the whole line, one undo step");
    }
}
