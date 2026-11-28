//! A synth preset's tags in the window: the menus that choose a preset (the
//! track editor's synth screen, the synth editor's preset list), opened as
//! its instruments and its styles, each opening its presets; and the synth
//! editor's entry of the tags, `BASS, dub` (the instrument first).

use super::Session;
use super::session_panel_views::synth_choice;
use crate::menu_action::SynthPresetAction;
use crate::panel::{Dropdown, Id, Rect, SUBMENU_MARK};
use ordrumbox_engine::synth_preset::tags::Facet;

/// What the presets without an instrument are listed under.
const NO_INSTRUMENT: &str = "other";

impl Session {
    /// The preset menu of select `control` (`Id::Synth`, `Id::SynthPreset`)
    /// under `rect`: the instruments, then the styles, each with how many
    /// presets it has; the instrument of `current` highlighted.
    pub(super) fn synth_preset_menu(&mut self, control: Id, current: &str, rect: Rect) {
        let presets = &self.synths.presets;
        let instruments = presets.groups(Facet::Instrument);
        let at = instruments.iter().position(|(_, keys)| keys.contains(&current)).unwrap_or(0);
        let instrument_lines = instruments.iter().enumerate().map(|(i, (tag, keys))| {
            let label = format!("{} ({}){SUBMENU_MARK}", tag.unwrap_or(NO_INSTRUMENT), keys.len());
            (SynthPresetAction::Group(Facet::Instrument, i), label)
        });
        // The presets without a style are all under their instrument already.
        let styles = presets.groups(Facet::Style);
        let style_lines = styles.iter().enumerate().filter_map(|(i, (tag, keys))| {
            let label = format!("style: {} ({}){SUBMENU_MARK}", (*tag)?, keys.len());
            Some((SynthPresetAction::Group(Facet::Style, i), label))
        });
        let lines = instrument_lines.chain(style_lines);
        let mut menu = Dropdown::from_actions(Id::SynthPresetMenu, rect, lines, at);
        menu.control = Some(control);
        self.chrome.widgets.open_dropdown(menu);
    }

    /// A line of the preset menus picked in `menu`: a group opens its
    /// presets beside it, a preset is set as its select sets it.
    pub(super) fn synth_preset_menu_pick(&mut self, action: SynthPresetAction, menu: Dropdown) {
        let Some(control) = menu.control else { return };
        let (facet, index) = match action {
            SynthPresetAction::Preset(key) => return self.select(control, &key),
            SynthPresetAction::Group(facet, index) => (facet, index),
        };
        let groups = self.synths.presets.groups(facet);
        let Some((tag, keys)) = groups.get(index) else { return };
        let title = tag.unwrap_or(NO_INSTRUMENT).to_owned();
        let current = self.synth_preset_shown(control);
        let at = keys.iter().position(|key| Some(*key) == current.as_deref());
        let lines = keys.iter().map(|key| (SynthPresetAction::Preset((*key).to_owned()), (*key).to_owned()));
        let size = self.chrome.panel_size;
        let mut submenu = Dropdown::submenu(menu, size, Id::SynthPresetMenu, title, lines);
        submenu.control = Some(control);
        if let Some(at) = at {
            submenu.highlight(at);
        }
        self.chrome.widgets.open_dropdown(submenu);
    }

    /// The preset select `control` shows.
    fn synth_preset_shown(&self, control: Id) -> Option<String> {
        match control {
            Id::SynthPreset => self.synth_editor.key().map(str::to_owned),
            _ => {
                let choice = synth_choice(&self.synths.presets, self.track(self.editor_track()?)?);
                choice.values.get(choice.selected).cloned()
            }
        }
    }

    /// The tags entry of the preset edited, its tags as `BASS, dub`.
    pub(super) fn synth_tags_entry(&mut self) {
        let Some(key) = self.synth_editor.key() else { return };
        let tags = Facet::ALL.map(|facet| self.synth_editor.tag(facet).unwrap_or_default());
        let text = if tags[1].is_empty() { tags[0].to_owned() } else { tags.join(", ") };
        let title = format!("Tags of {key} (instrument, style):");
        self.show_entry(Id::SynthTags, None, title, text);
    }

    /// The tags entry confirmed with `text`: the preset's instrument and
    /// style, one undo step when they change; an unknown instrument is refused.
    pub(super) fn commit_synth_tags(&mut self, text: &str) {
        let (instrument, style) = text.split_once(',').unwrap_or((text, ""));
        let parsed = Facet::Instrument.parse(instrument).and_then(|i| Ok((i, Facet::Style.parse(style)?)));
        let (instrument, style) = match parsed {
            Ok(tags) => tags,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        let instrument_changed = self.synth_editor.set_tag(Facet::Instrument, instrument.as_deref());
        let style_changed = self.synth_editor.set_tag(Facet::Style, style.as_deref());
        if instrument_changed || style_changed {
            let group = instrument.as_deref().unwrap_or(NO_INSTRUMENT);
            self.commit_synth();
            self.message = format!("tags set: listed under {group}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, choose, menu, type_entry};
    use super::*;
    use crate::kits::KitSet;
    use crate::menu_action::MenuAction;
    use crate::panel;
    use crate::ui_input::Button;
    use crate::view::Cursor;
    use ordrumbox_core::{Library, json};
    use ordrumbox_engine::SynthPresets;

    /// The lines of a group's list of presets: these keys.
    fn presets(keys: &[&str]) -> Vec<MenuAction> {
        keys.iter().map(|key| SynthPresetAction::Preset((*key).into()).into()).collect()
    }

    /// Three presets: two basses, one dub; a lead without a style.
    fn session() -> Session {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        let presets = r#"{"LEAD":{"instrument":"SYNTHLEAD"},"SUB":{"instrument":"BASS","style":"dub"},
            "ACID":{"instrument":"BASS","style":"acid"}}"#;
        s.set_asset_synths(SynthPresets::from_json(&json::parse(presets).unwrap()).unwrap());
        s
    }

    #[test]
    fn the_preset_list_opens_by_instrument_and_by_style() {
        let mut s = session();
        s.toggle_synth();
        s.load_synth("SUB");
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        let list = panel::find(&layout.items, Id::SynthPreset).expect("the preset list").rect;
        s.mouse_down(SIZE, list.x + 2, list.y + 2, Button::Left, false);
        let open = s.chrome.widgets.dropdown().expect("the groups");
        assert_eq!(open.labels, ["BASS (2) >", "SYNTHLEAD (1) >", "style: acid (1) >", "style: dub (1) >"]);
        assert_eq!(open.at, 0, "the instrument of the preset edited");
        // BASS, then its presets, the one edited highlighted: ACID picked.
        choose(&mut s, 0);
        assert_eq!(menu(&s), presets(&["ACID", "SUB"]));
        assert_eq!(s.chrome.widgets.dropdown().map(|d| d.at), Some(1));
        s.pick(0);
        assert_eq!(s.synth_editor.key(), Some("ACID"));
    }

    #[test]
    fn the_track_s_synth_screen_opens_the_same_menus() {
        let mut s = session();
        let pattern = r#"{"patterns":[{"tracks":[{"name":"BASS","useSynth":true,"synthPreset":"SUB"}]}]}"#;
        s.editor = ordrumbox_logic::editor::Editor::new(Library::parse(pattern).unwrap().0);
        s.selection.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        s.synth_preset_menu(Id::Synth, "SUB", Rect::new(0, 0, 100, 14));
        s.pick(3);
        assert_eq!(menu(&s), presets(&["SUB"]), "the dub presets");
        s.pick(0);
        s.synth_preset_menu(Id::Synth, "SUB", Rect::new(0, 0, 100, 14));
        s.pick(0);
        assert_eq!(s.chrome.widgets.dropdown().map(|d| d.at), Some(1), "the track's preset");
        s.pick(0);
        assert_eq!(s.track(0).and_then(|t| t.synth_preset.as_deref()), Some("ACID"));
    }

    #[test]
    fn the_tags_entry_sets_the_tags_in_one_step() {
        let mut s = session();
        s.load_synth("LEAD");
        s.synth_tags_entry();
        assert_eq!(s.chrome.widgets.entry().map(|e| e.text()), Some("SYNTHLEAD"));
        type_entry(&mut s, " bass , Dub ");
        assert_eq!(s.message, "tags set: listed under BASS");
        let groups = s.synths.presets.groups(Facet::Style);
        assert_eq!(groups[1], (Some("dub"), vec!["LEAD", "SUB"]));
        s.undo();
        assert_eq!(s.synths.presets.groups(Facet::Instrument)[1], (Some("SYNTHLEAD"), vec!["LEAD"]), "one undo step");
        // An instrument the catalog does not hold: refused, nothing changes.
        s.synth_tags_entry();
        type_entry(&mut s, "kazoo, dub");
        assert!(s.message.contains("no instrument"), "{}", s.message);
        assert!(s.editor.history().next_undo().is_none());
    }
}
