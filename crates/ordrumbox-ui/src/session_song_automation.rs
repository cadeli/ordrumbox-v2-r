//! The song panel's automation lanes: the add-automation button opens the
//! instruments the song plays, each opening the controls a lane can switch
//! (Mute, Solo); the one picked adds its lane. A click on a lane's step
//! places an on or off point there (or removes the one there), a click on
//! its name opens its menu. Each edit is one undo step.

use ordrumbox_core::automation::{AutomationTarget, song_instruments};

use super::Session;
use crate::panel::{Dropdown, Id, SUBMENU_MARK};
use crate::song_view::SongHit;

/// The values of the automation menus' lines.
const INSTRUMENT_VALUE: &str = "instrument:";
const LANE_VALUE: &str = "lane:";
const REMOVE_VALUE: &str = "remove:";

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
            SongHit::LaneStep { lane, step } => match self.editor.toggle_automation_point(None, lane, step) {
                Ok(_) => self.edited(),
                Err(message) => self.message = message,
            },
            _ => {}
        }
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

    /// Lane `lane`'s menu at `(x, y)`: remove it.
    fn lane_menu(&mut self, lane: usize, x: i32, y: i32) {
        let Some(lane_shown) = self.song().and_then(|song| song.automations.get(lane)) else { return };
        let title = format!("{} {}", lane_shown.target.label(), lane_shown.instrument);
        let values = vec![format!("{REMOVE_VALUE}{lane}")];
        let menu = Dropdown::menu(Id::AutomationMenu, x, y, title, values, vec!["Delete lane".into()]);
        self.chrome.widgets.dropdown = Some(menu);
    }

    /// A line of the automation menus, picked in `menu`: an instrument opens
    /// its controls beside it, a control adds its lane, Delete removes one.
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
        assert!(s.song_click(SIZE, lx, ly, Button::Left));
        assert_eq!(s.song().unwrap().automations[0].value_at(0), Some(true));
        assert!(s.song_click(SIZE, lx, ly, Button::Left));
        assert!(s.song().unwrap().automations[0].points().is_empty(), "clicked again: the point goes");
        s.undo();
        assert_eq!(s.song().unwrap().automations[0].points().len(), 1);

        // Its name opens its menu, which removes it.
        let (nx, ny) = find(&s, |hit| hit == SongHit::LaneName(0));
        assert!(s.song_click(SIZE, nx, ny, Button::Left));
        assert_eq!(menu(&s), ["remove:0"]);
        choose(&mut s, 0);
        assert!(s.song().unwrap().automations.is_empty());
    }
}
