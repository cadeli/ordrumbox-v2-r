//! The song panel's automation lanes: the add-automation button opens the
//! instruments the song plays, each opening the controls a lane can switch
//! (Mute, Solo); the one picked adds its lane. A press on a lane's step and
//! a drag along it paint the steps selected, and no other: on where the
//! step pressed was not on, off where it was (a click paints that step
//! alone). A click (left or right) on its name opens its menu: clear its
//! points, delete it, or delete every lane. Each edit, a whole drag
//! included, is one undo step.

use crate::scale::Scale;
use ordrumbox_core::automation::{AutomationLane, AutomationTarget, STEPS_PER_MEASURE, song_instruments};

use super::Session;
use crate::panel::{Dropdown, Id, SUBMENU_MARK};
use crate::song_view::SongHit;

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
    /// What the zone is painted: off when the pressed step was on, else on.
    is_on: bool,
    /// The lane as pressed: each move paints it anew.
    base: AutomationLane,
    /// The undo key of this press's edits.
    key: String,
}

impl Session {
    /// The automation lanes of the arrangement shown.
    pub(super) fn song_lane_count(&self) -> usize {
        self.song().map_or(0, |song| song.automations.len())
    }

    /// A click at `(x, y)` on the add-automation button, a lane's name or one of its steps.
    pub(super) fn automation_click(&mut self, hit: SongHit, x: i32, y: i32) {
        match hit {
            SongHit::AddAutomation => self.automation_menu(x, y),
            SongHit::LaneName(lane) => self.lane_menu(lane, x, y),
            SongHit::LaneStep { lane, step } => {
                let Some(base) = self.song().and_then(|song| song.automations.get(lane)).cloned() else { return };
                let paint = LanePaint {
                    lane,
                    from: step,
                    to: step,
                    is_on: base.value_at(step) != Some(true),
                    base,
                    key: format!("lane-paint:{lane}:{}", self.editor.revision()),
                };
                self.paint_lane(&paint);
                self.song_panel.paint = Some(paint);
            }
            _ => {}
        }
    }

    /// The pointer at `x` while a lane is pressed: the zone from the pressed
    /// step to the one under it painted; whether it changed.
    pub(super) fn lane_paint_move(&mut self, size: (i32, i32, Scale), x: i32) -> bool {
        let layout = self.song_layout(size);
        let first = self.song_scroll(&layout, self.song_rows().len() + self.song_lane_count()).0;
        let total = self.song().map_or(1, |song| song.length_measures().max(1.0).ceil() as u32) * STEPS_PER_MEASURE;
        let step = layout.step_at(x, first).min(total - 1);
        let Some(paint) = &mut self.song_panel.paint else { return false };
        if step == paint.to {
            return false;
        }
        paint.to = step;
        let paint = paint.clone();
        self.paint_lane(&paint);
        true
    }

    /// The zone of `paint` painted on its lane as it was pressed.
    fn paint_lane(&mut self, paint: &LanePaint) {
        let zone = (paint.from, paint.to);
        match self.editor.paint_automation(None, paint.lane, &paint.base, zone, paint.is_on, &paint.key) {
            Ok(()) => self.edited(),
            Err(message) => self.message = message,
        }
    }

    /// The lane released: the zone stays as painted.
    pub(super) fn lane_paint_release(&mut self) -> bool {
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
                    self.message =
                        format!("{} lane of {instrument} added: click a step to switch it on or off", target.label());
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
    use crate::song_view::SongHit;
    use crate::ui_input::Button;
    use ordrumbox_core::automation::AutomationTarget;

    /// Where `hit` is on the song panel, scanning it.
    fn find(s: &super::Session, wanted: impl Fn(SongHit) -> bool) -> (i32, i32) {
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
        assert_eq!(menu(&s), ["lane:mute:SNARE", "lane:solo:SNARE"]);
        choose(&mut s, 1);
        let lanes = &s.song().unwrap().automations;
        assert_eq!((lanes[0].instrument.as_str(), lanes[0].target), ("SNARE", AutomationTarget::Solo));

        // The lane's row is under the patterns', the button under it.
        let (lx, ly) = find(&s, |hit| matches!(hit, SongHit::LaneStep { lane: 0, step: 0 }));
        assert!(ly > y - 20, "after the pattern rows");
        assert!(find(&s, |hit| hit == SongHit::AddAutomation).1 > ly);
        let points = |s: &super::Session| -> Vec<(u32, bool)> {
            s.song().unwrap().automations[0].points().iter().map(|p| (p.step, p.is_on)).collect()
        };
        assert!(s.song_click(SIZE, lx, ly, Button::Left));
        assert!(s.song_release(SIZE));
        assert_eq!(points(&s), [(0, true), (1, false)], "a click: that step on, no more");
        assert!(s.song_click(SIZE, lx, ly, Button::Left));
        assert!(s.song_release(SIZE));
        assert_eq!(points(&s), [(0, false)], "clicked again: off");
        s.undo();
        s.undo();
        assert!(points(&s).is_empty());

        // A drag paints the steps it crosses, in one undo step.
        let step_x = |s: &super::Session, step: u32| {
            find(s, |hit| matches!(hit, SongHit::LaneStep { lane: 0, step: at } if at == step)).0
        };
        let drag = |s: &mut super::Session, from: u32, to: u32| {
            let (from, to) = (step_x(s, from), step_x(s, to));
            assert!(s.song_click(SIZE, from, ly, Button::Left));
            assert!(s.mouse_move(SIZE, to, ly + 30, false), "the row under the pointer does not matter");
            assert!(s.song_release(SIZE));
        };
        drag(&mut s, 2, 9);
        assert_eq!(points(&s), [(2, true), (10, false)], "from an empty step: the zone on, no more");
        drag(&mut s, 4, 6);
        assert_eq!(points(&s), [(2, true), (4, false), (7, true), (10, false)], "from an on step: the zone off");
        s.undo();
        assert_eq!(points(&s), [(2, true), (10, false)], "a drag is one undo step");
        s.undo();
        assert!(points(&s).is_empty());

        // A right-click on its name opens its menu: clear it, delete it, delete every lane.
        let (nx, ny) = find(&s, |hit| hit == SongHit::LaneName(0));
        assert!(s.song_click(SIZE, nx, ny, Button::Right));
        assert_eq!(menu(&s), ["clear:0", "remove:0", "remove-all"]);
        choose(&mut s, 1);
        assert!(s.song().unwrap().automations.is_empty());
    }
}
