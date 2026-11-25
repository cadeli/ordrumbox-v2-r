//! An option picked in the open list: a select's value is set, a menu's
//! action runs, each menu's by its own `*_menu_pick`.

use super::Session;
use super::session_confirm::Pending;
use crate::menu_action::MenuAction;
use crate::panel::{Choices, Dropdown, Id};

impl Session {
    /// Option `index` of the open list picked.
    pub(super) fn pick(&mut self, index: usize) {
        let Some(mut dropdown) = self.chrome.widgets.take_dropdown() else { return };
        // A menu that opens another stays shown beside it, this line highlighted.
        dropdown.at = index;
        match &dropdown.choices {
            Choices::Values(values) => {
                if let Some(value) = values.get(index).cloned() {
                    self.select(dropdown.id, &value);
                }
            }
            Choices::Actions(actions) => {
                if let Some(action) = actions.get(index).cloned() {
                    self.menu_pick(action, dropdown);
                }
            }
        }
    }

    /// `action`, picked in `menu`: the menu's own pick runs it.
    fn menu_pick(&mut self, action: MenuAction, menu: Dropdown) {
        match action {
            MenuAction::Song(action) => self.song_menu_pick(action),
            MenuAction::Loop(action) => self.loop_menu_pick(action),
            MenuAction::LoadProject(path) => self.ask(Pending::LoadProject(path)),
            MenuAction::Roll(action) => self.roll_menu_pick(action),
            MenuAction::Lane(action) => self.lane_menu_pick(action),
            MenuAction::Automation(action) => self.automation_menu_pick(action, menu),
            MenuAction::Grid(action) => self.grid_menu_pick(action, menu),
            MenuAction::Instrument(action) => match menu.id {
                Id::TrackNameMenu(target) => self.track_name_menu_pick(target, action, menu),
                Id::KitInstrumentMenu(sample) => self.kit_instrument_menu_pick(sample, action, menu),
                Id::InstrumentMenu => self.instrument_menu_pick(action, menu),
                _ => {}
            },
            MenuAction::Control(action) => self.control_menu_pick(action, menu.control),
            MenuAction::Pattern(action) => self.pattern_menu_pick(action, menu),
            MenuAction::SynthPreset(action) => self.synth_preset_menu_pick(action, menu),
        }
    }
}
