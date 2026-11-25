//! The song panel's automation lanes: the add-automation button opens the
//! instruments the song plays, each opening the controls a lane can set
//! ([`AutomationTarget::ALL`]); the one picked adds its lane. A press on a lane's
//! step and a drag along it paint the steps selected, and no other: on a
//! switch's lane, on where the step pressed was not on, off where it was; on
//! a level's lane, a line from the level pressed to the one the pointer is
//! at; with the right button, erased (as the pattern has it). The zone is whole measures, beats or
//! steps, as fine as the zoom shows them; Shift held, steps. A click (left
//! or right) on its name opens its menu: clear its points, delete it, or
//! delete every lane. Each edit, a whole drag included, is one undo step.

use crate::scale::Scale;
use ordrumbox_core::automation::{
    AutomationKind, AutomationLane, AutomationStroke, AutomationTarget, song_instruments, step_label, switch_value,
};

use super::Session;
use crate::menu_action::AutomationAction;
use crate::panel::{Dropdown, Id, SUBMENU_MARK};
use crate::song_view::SongSpot;
use crate::ui_input::Button;

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
    /// A level's segment grabbed on its line: steps `zone`, at `level` when
    /// pressed at `grab_y`, moved up or down as one with the pointer at `y`.
    Segment { band: (i32, i32), zone: (u32, u32), level: f64, grab_y: i32, y: i32 },
}

impl LanePaint {
    /// The stroke it paints now.
    fn stroke(&self) -> AutomationStroke {
        match self.brush {
            Brush::Fill(value) => AutomationStroke::Fill(value),
            Brush::Line { from, to, .. } => AutomationStroke::Line((self.from, from), (self.to, to)),
            Brush::Segment { band: (_, height), level, grab_y, y, .. } => {
                AutomationStroke::Fill(Some(level + f64::from(grab_y - y) / f64::from(height.max(1))))
            }
        }
    }
}

impl Session {
    /// The rows the automation lanes of the song shown take.
    pub(super) fn song_lane_rows(&self) -> usize {
        self.song().map_or(0, |song| song.automations.iter().map(|lane| lane.row_count() as usize).sum())
    }

    /// A click at `(x, y)` on the add-automation button, a lane's name or one of its steps.
    pub(super) fn automation_click(
        &mut self,
        size: (i32, i32, Scale),
        spot: SongSpot,
        (x, y): (i32, i32),
        button: Button,
    ) {
        match (spot, button) {
            (SongSpot::AddAutomation, Button::Left) => self.automation_menu(x, y),
            (SongSpot::LaneBadge(lane), Button::Left) => self.reenable_lane(lane),
            (SongSpot::LaneName(lane) | SongSpot::LaneBadge(lane), _) | (SongSpot::LaneEdge(lane), Button::Right) => {
                self.lane_menu(lane, x, y);
            }
            (SongSpot::LaneStep { lane, step }, _) => self.press_lane(size, lane, (step, y), button, false),
            (SongSpot::LaneEdge(lane), _) => {
                self.song_panel.lane_resize = Some((lane, format!("lane-rows:{lane}:{}", self.editor.revision())));
            }
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
        let rows = self.song_rows();
        let Some(band) = layout.lane_band(&self.song_view_data(&rows, &layout), lane) else { return };
        let brush = match (button, base.target().kind()) {
            (Button::Right, _) => Brush::Fill(None),
            (_, AutomationKind::Switch) => Brush::Fill(Some(switch_value(base.is_on_at(step) != Some(true)))),
            (_, AutomationKind::Level) => match base.value_at(step) {
                Some(level) if layout.is_on_level_line(band, level, y) => {
                    Brush::Segment { band, zone: base.segment_at(step, self.song_step_count()), level, grab_y: y, y }
                }
                _ => {
                    let level = layout.level_at_y(band, y);
                    Brush::Line { band, from: level, to: level }
                }
            },
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
        let first = self.song_scroll(&layout, self.song_rows().len() + self.song_lane_rows()).0;
        let step = layout.step_at(x, first).min(self.song_step_count() - 1);
        let Some(paint) = &mut self.song_panel.paint else { return false };
        let brush = match paint.brush {
            Brush::Line { band, from, .. } => Brush::Line { band, from, to: layout.level_at_y(band, y) },
            Brush::Segment { band, zone, level, grab_y, .. } => Brush::Segment { band, zone, level, grab_y, y },
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

    /// The bottom edge of the lane held moved to `y`: the lane as many
    /// rows tall as the edge is near; whether that changed it.
    pub(super) fn lane_resize_move(&mut self, size: (i32, i32, Scale), y: i32) -> bool {
        let Some((lane, key)) = self.song_panel.lane_resize.clone() else { return false };
        let layout = self.song_layout(size);
        let rows = self.song_rows();
        let count = layout.lane_rows_at(&self.song_view_data(&rows, &layout), lane, y);
        let changed = self.editor.set_automation_lane_rows(None, lane, count, &key);
        if changed {
            self.edited();
        }
        changed
    }

    /// The steps of the song shown, a measure at least.
    fn song_step_count(&self) -> u32 {
        self.song().map_or(ordrumbox_core::automation::STEPS_PER_MEASURE, ordrumbox_core::Song::step_count)
    }

    /// The zone of `paint`, widened to whole snaps, painted on its lane as it was pressed.
    fn paint_lane(&mut self, paint: &LanePaint) {
        let snap = if paint.is_fine { 1 } else { paint.snap.max(1) };
        let (first, last) = (paint.from.min(paint.to), paint.from.max(paint.to));
        let zone = match paint.brush {
            Brush::Segment { zone, .. } => zone,
            _ => (first / snap * snap, (last / snap * snap + snap - 1).min(self.song_step_count() - 1)),
        };
        match self.editor.paint_automation(None, paint.lane, &paint.base, zone, paint.stroke(), &paint.key) {
            Ok(()) => self.edited(),
            Err(message) => self.message = message,
        }
    }

    /// What the lane painted, else the one under the mouse, sets at the
    /// pointer's step: `KICK Volume -4.5 dB at 2.3`.
    pub(super) fn lane_tooltip(&self) -> Option<String> {
        let (lane, step, value) = match &self.song_panel.paint {
            Some(paint) => (&paint.base, paint.to, paint.stroke().value_at(paint.to)),
            None => {
                let (lane, step) = self.song_panel.hovered_lane?;
                let lane = self.song()?.automations.get(lane)?;
                (lane, step, lane.value_at(step))
            }
        };
        let value = value.map_or_else(|| "as the pattern".to_owned(), |value| lane.target().value_label(value));
        let (instrument, target) = (lane.instrument(), lane.target());
        let taken_back = if self.is_taken_back(instrument, target) { ", taken back: not played" } else { "" };
        Some(format!("{instrument} {} {value} at {}{taken_back}", target.label(), step_label(step)))
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
        let lines =
            names.iter().map(|name| (AutomationAction::Instrument(name.clone()), format!("{name}{SUBMENU_MARK}")));
        let menu = Dropdown::menu(Id::AutomationMenu, x, y, "New automation".into(), lines);
        self.chrome.widgets.open_dropdown(menu);
    }

    /// Lane `lane`'s menu at `(x, y)`: re-enable it when a control took it
    /// back, make it taller or shorter, clear its points, delete it, delete
    /// every lane.
    fn lane_menu(&mut self, lane: usize, x: i32, y: i32) {
        let Some(lane_shown) = self.song().and_then(|song| song.automations.get(lane)) else { return };
        let title = format!("{} {}", lane_shown.target().label(), lane_shown.instrument());
        let is_taken_back = self.is_taken_back(lane_shown.instrument(), lane_shown.target());
        let reenable = is_taken_back.then_some((AutomationAction::Reenable(lane), "Re-enable lane"));
        let lines = [
            (AutomationAction::Taller(lane), "Taller"),
            (AutomationAction::Shorter(lane), "Shorter"),
            (AutomationAction::Clear(lane), "Clear all points"),
            (AutomationAction::Remove(lane), "Delete lane"),
            (AutomationAction::RemoveAll, "Delete all lanes"),
        ];
        let lines = reenable.into_iter().chain(lines).map(|(action, label)| (action, label.to_owned()));
        let menu = Dropdown::menu(Id::AutomationMenu, x, y, title, lines);
        self.chrome.widgets.open_dropdown(menu);
    }

    /// A line of the automation menus, picked in `menu`: an instrument opens
    /// its controls beside it, a control adds its lane, the lane's menu
    /// clears or removes it, or removes every lane.
    pub(super) fn automation_menu_pick(&mut self, action: AutomationAction, menu: Dropdown) {
        match action {
            AutomationAction::Instrument(instrument) => {
                let lines = AutomationTarget::ALL.map(|target| {
                    (AutomationAction::AddLane { target, instrument: instrument.clone() }, target.label().to_owned())
                });
                let size = self.chrome.panel_size;
                let submenu = Dropdown::submenu(menu, size, Id::AutomationMenu, instrument, lines);
                self.chrome.widgets.open_dropdown(submenu);
            }
            AutomationAction::AddLane { target, instrument } => self.add_lane(target, &instrument),
            AutomationAction::Reenable(lane) => self.reenable_lane(lane),
            AutomationAction::Remove(lane) => self.undoable(|s| {
                if s.editor.remove_automation_lane(None, lane) {
                    s.edited();
                }
            }),
            AutomationAction::Clear(lane) => self.undoable(|s| {
                if s.editor.clear_automation_lane(None, lane) {
                    s.edited();
                }
            }),
            AutomationAction::Taller(lane) => self.resize_lane(lane, 1),
            AutomationAction::Shorter(lane) => self.resize_lane(lane, -1),
            AutomationAction::RemoveAll => self.undoable(|s| {
                if s.editor.remove_automation_lanes(None) {
                    s.edited();
                }
            }),
        }
    }

    /// A lane of `target` for `instrument` added to the song.
    fn add_lane(&mut self, target: AutomationTarget, instrument: &str) {
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
    }

    /// Lane `lane` made `delta` rows taller (shorter below 0).
    fn resize_lane(&mut self, lane: usize, delta: i32) {
        let Some(rows) = self.song().and_then(|song| song.automations.get(lane)).map(|l| l.row_count()) else {
            return;
        };
        let key = format!("lane-rows:{lane}:{}", self.editor.revision());
        if self.editor.set_automation_lane_rows(None, lane, rows.saturating_add_signed(delta), &key) {
            self.edited();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, choose, find_spot, menu, session};
    use super::Session;
    use crate::menu_action::{AutomationAction, MenuAction};
    use crate::song_view::{MeasureZoom, SongSpot};
    use crate::ui_input::Button;
    use ordrumbox_core::automation::AutomationTarget;

    /// The lines of an automation menu.
    fn lines(actions: impl IntoIterator<Item = AutomationAction>) -> Vec<MenuAction> {
        actions.into_iter().map(MenuAction::from).collect()
    }

    const ON: Option<f64> = Some(1.0);
    const OFF: Option<f64> = Some(0.0);

    fn points(s: &Session) -> Vec<(u32, Option<f64>)> {
        s.song().unwrap().automations[0].points().iter().map(|p| (p.step, p.value)).collect()
    }

    /// X of `step` of the first lane, and Y of its row.
    fn step_at(s: &Session, step: u32) -> (i32, i32) {
        find_spot(s, |spot| matches!(spot, SongSpot::LaneStep { lane: 0, step: at } if at == step))
    }

    /// A press on step `from` of the first lane with `button`, dragged to step `to`.
    fn drag(s: &mut Session, (from, to): (u32, u32), button: Button) {
        let ((from, y), (to, _)) = (step_at(s, from), step_at(s, to));
        assert!(s.song_click(SIZE, from, y, button));
        assert!(s.mouse_move(SIZE, to, y + 30, false), "the row under the pointer does not matter");
        assert!(s.lane_paint_release());
    }

    /// The band the first lane is drawn in.
    fn band(s: &Session) -> (i32, i32) {
        let layout = s.song_layout(SIZE);
        let rows = s.song_rows();
        layout.lane_band(&s.song_view_data(&rows, &layout), 0).expect("a lane")
    }

    /// A session whose song has a lane of KICK's `target`.
    fn with_lane(target: AutomationTarget) -> Session {
        let mut s = session();
        s.editor.add_track(1, "KICK", 4);
        s.editor.add_automation_lane(None, "KICK", target).unwrap();
        s.set_size(SIZE);
        s
    }

    #[test]
    fn the_button_adds_a_lane_whose_steps_switch_on_and_off() {
        let mut s = session();
        for name in ["SNARE", "KICK", "SNARE"] {
            s.editor.add_track(1, name, 4);
        }
        s.set_size(SIZE);
        let (x, y) = find_spot(&s, |spot| spot == SongSpot::AddAutomation);
        assert!(s.song_click(SIZE, x, y, Button::Left));
        let instruments = ["KICK", "SNARE"].map(|name| AutomationAction::Instrument(name.into()));
        assert_eq!(menu(&s), lines(instruments), "the song's instruments, once each");
        choose(&mut s, 1);
        let targets =
            AutomationTarget::ALL.map(|target| AutomationAction::AddLane { target, instrument: "SNARE".into() });
        assert_eq!(menu(&s), lines(targets));
        choose(&mut s, 1);
        let lanes = &s.song().unwrap().automations;
        assert_eq!((lanes[0].instrument(), lanes[0].target()), ("SNARE", AutomationTarget::Solo));

        // The lane's row is under the patterns', the button under it.
        let (lx, ly) = find_spot(&s, |spot| matches!(spot, SongSpot::LaneStep { lane: 0, step: 0 }));
        assert!(ly > y - 20, "after the pattern rows");
        assert!(find_spot(&s, |spot| spot == SongSpot::AddAutomation).1 > ly);
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
        let (nx, ny) = find_spot(&s, |spot| spot == SongSpot::LaneName(0));
        assert!(s.song_click(SIZE, nx, ny, Button::Right));
        assert_eq!(
            menu(&s),
            lines([
                AutomationAction::Taller(0),
                AutomationAction::Shorter(0),
                AutomationAction::Clear(0),
                AutomationAction::Remove(0),
                AutomationAction::RemoveAll
            ])
        );
        choose(&mut s, 3);
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
        let ((from, _), (to, _)) = (step_at(&s, 0), step_at(&s, 4));
        let (top, height) = band(&s);
        assert!(s.song_click(SIZE, from, top + height, Button::Left));
        assert_eq!(points(&s), [(0, OFF), (1, None)], "pressed at the bottom of the band: 0");
        assert_eq!(s.lane_tooltip().as_deref(), Some("KICK Volume silent at 1.1"));
        assert!(s.mouse_move(SIZE, to, top - 40, false));
        assert_eq!(s.lane_tooltip().as_deref(), Some("KICK Volume 0.0 dB at 1.5"), "said as it is drawn");
        assert!(s.lane_paint_release());
        let (x, _) = step_at(&s, 2);
        assert!(s.song_hover(SIZE, x, top + 1));
        assert_eq!(s.lane_tooltip().as_deref(), Some("KICK Volume -15.0 dB at 1.3"), "and under the mouse");
        let (x, _) = step_at(&s, 9);
        assert!(s.song_hover(SIZE, x, top + 1));
        assert_eq!(s.lane_tooltip().as_deref(), Some("KICK Volume as the pattern at 1.10"));
        let expected = [(0, OFF), (1, Some(0.25)), (2, Some(0.5)), (3, Some(0.75)), (4, ON), (5, None)];
        assert_eq!(points(&s), expected, "up to the top, and above it: the whole level");
        s.undo();
        assert!(points(&s).is_empty(), "the whole line, one undo step");
    }

    #[test]
    fn a_lane_s_edge_makes_it_taller_and_its_segments_move_as_one() {
        let mut s = with_lane(AutomationTarget::Volume);
        let (_, height) = band(&s);
        // Its bottom edge, dragged two rows down: three rows tall, in one undo step.
        let (x, y) = find_spot(&s, |spot| spot == SongSpot::LaneEdge(0));
        assert!(s.song_click(SIZE, x, y, Button::Left));
        let row = s.song_layout(SIZE).row_height();
        assert!(s.mouse_move(SIZE, x, y + row, false));
        assert!(s.mouse_move(SIZE, x, y + 2 * row, false));
        assert!(s.song_release(SIZE));
        assert_eq!(s.song().unwrap().automations[0].row_count(), 3);
        let (top, tall) = band(&s);
        assert_eq!(tall, height + 2 * row, "drawn over three rows");
        s.undo();
        assert_eq!(s.song().unwrap().automations[0].row_count(), 1, "the drag, one step");
        s.redo();

        // A segment at half, grabbed on its line, moved up a quarter of the band.
        let base = s.song().unwrap().automations[0].clone();
        s.editor
            .paint_automation(None, 0, &base, (4, 7), ordrumbox_core::AutomationStroke::Fill(Some(0.5)), "a")
            .unwrap();
        let (x, _) = step_at(&s, 5);
        let line = top + tall / 2;
        assert!(s.song_click(SIZE, x, line, Button::Left));
        assert!(s.mouse_move(SIZE, x + 40, line - tall / 4, false), "the pointer's steps do not matter");
        assert!(s.lane_paint_release());
        let moved = points(&s);
        assert_eq!((moved.len(), moved[0].0, moved[1]), (2, 4, (8, None)), "the whole segment, nothing else");
        // A quarter of the band up, to a pixel.
        assert!(moved[0].1.is_some_and(|level| (level - 0.75).abs() <= 1.0 / f64::from(tall)), "{moved:?}");
        // Off its line, a press draws.
        assert!(s.song_click(SIZE, x, top + tall, Button::Left));
        assert!(s.lane_paint_release());
        assert_eq!(points(&s)[1], (5, Some(0.0)), "at the bottom: silent");
    }
}
