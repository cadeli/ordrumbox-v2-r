//! File > Demos, in the session: the browser opened (also at the first
//! start), its keys and clicks, and the song or pattern chosen copied into
//! the library as one undo step, shown and played.

use ordrumbox_engine::Mode;
use ordrumbox_logic::demo_catalog::{DemoItem, copy_demo};

use super::Session;
use super::session_overlay::Overlay;
use super::session_types::{OverlayKey, PlayMode};
use crate::demo_browser::{DemoBrowser, DemoLayout, Outcome};
use crate::scale::Scale;

impl Session {
    /// File > Demos: the styles, as tiles.
    pub(super) fn open_demos(&mut self) {
        if self.demo_styles.is_empty() {
            self.message = "no demos: the assets folder has no data/demos/".into();
            return;
        }
        self.chrome.overlay = Some(Overlay::Demos(DemoBrowser::new()));
    }

    /// A key while the browser is open; whether it was.
    pub(super) fn demos_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::Demos(browser)) = &mut self.chrome.overlay else { return false };
        let outcome = browser.key(key, &self.demo_styles);
        let (width, height, scale) = self.chrome.panel_size;
        browser.follow(&DemoLayout::new(width, height, scale));
        self.demos_outcome(outcome);
        true
    }

    /// A click while the browser is open; whether it was.
    pub(super) fn demos_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> bool {
        let Some(Overlay::Demos(browser)) = &mut self.chrome.overlay else { return false };
        let layout = DemoLayout::new(width, height, scale);
        let outcome = browser.click(&layout, &self.demo_styles, x, y);
        browser.follow(&layout);
        self.demos_outcome(outcome);
        true
    }

    fn demos_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Browsing => {}
            Outcome::Close => self.chrome.overlay = None,
            Outcome::Load { style, item } => self.load_demo(style, item),
        }
    }

    /// `item` of style `style` copied into the library (one undo step; what
    /// the library already holds is kept), then shown and played.
    fn load_demo(&mut self, style: usize, item: DemoItem) {
        let Some(demos) = self.demo_styles.get(style) else { return };
        let name = demos.item_name(item).to_owned();
        let mut library = self.editor.library().clone();
        let Some(copied) = copy_demo(&mut library, demos.library(), item) else { return };
        self.chrome.overlay = None;
        if !copied.was_there {
            let selected_song = match copied.item {
                DemoItem::Song(s) => s,
                DemoItem::Pattern(_) => self.editor.selected_song(),
            };
            self.editor.replace_library(library, selected_song, &format!("Load the demo {name}"));
            self.after_history(None);
        }
        let added = if copied.was_there {
            format!("{name} is already in your library")
        } else {
            match copied.added_pattern_count {
                1 if matches!(item, DemoItem::Pattern(_)) => format!("{name} added to your library"),
                count => format!("{name} added to your library with {count} pattern(s)"),
            }
        };
        let played = match copied.item {
            DemoItem::Song(s) => self.play_loaded_song(s),
            DemoItem::Pattern(p) => self.play_loaded_pattern(p),
        };
        self.message = format!("{added}{played}");
    }

    /// Pattern `index` shown and played from its start; what to say of it.
    fn play_loaded_pattern(&mut self, index: usize) -> String {
        self.show_mode(PlayMode::Pattern);
        if self.playing() {
            self.playback.stop();
        }
        self.show_pattern(index);
        self.toggle(Mode::Pattern(index), 0);
        self.playing_text()
    }

    /// Song `index` selected, shown and played from its start; what to say of it.
    fn play_loaded_song(&mut self, index: usize) -> String {
        if self.playing() {
            self.playback.stop();
        }
        self.editor.set_selected_song(index);
        self.show_mode(PlayMode::Song);
        self.transport.song_cursor = 0;
        self.toggle_play_song();
        self.playing_text()
    }

    /// What to say after a demo was started: how to stop it and take it out.
    fn playing_text(&self) -> String {
        if self.playing() { ", playing (Space stops, Edit > Undo takes it out)".into() } else { String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Command;
    use ordrumbox_core::Library;
    use ordrumbox_logic::demo_catalog::DemoStyle;

    const STYLE: &str = r#"{"info":{"name":"Funk"},"patterns":[{"id":"a","name":"A","tracks":[{"name":"KICK"}]},
        {"id":"b","name":"B","tracks":[{"name":"KICK"}]}],
        "songs":[{"id":"s","name":"S","bpm":100,"clips":[{"patternId":"b","startMeasure":0,"measureCount":1}]}]}"#;

    fn session() -> Session {
        let library = Library::parse(r#"{"patterns":[{"id":"mine","name":"Mine"}]}"#).unwrap().0;
        let mut s = Session::new(library, crate::kits::KitSet::empty(), Vec::new());
        s.demo_styles = vec![DemoStyle::new(Library::parse(STYLE).unwrap().0, "funk")];
        s.set_size((1000, 700, Scale::ONE));
        s
    }

    #[test]
    fn a_pattern_is_loaded_as_one_undo_step_and_shown() {
        let mut s = session();
        s.run(Command::Demos);
        assert!(matches!(s.chrome.overlay, Some(Overlay::Demos(_))));
        s.overlay_key(OverlayKey::Enter);
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert!(s.chrome.overlay.is_none(), "the browser closes on a load");
        let ids: Vec<&str> = s.editor.library().patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["mine", "a"]);
        assert_eq!(s.pattern_index, 1);
        assert!(s.message.starts_with("A added to your library"), "{}", s.message);
        s.undo();
        assert_eq!(s.editor.library().patterns.len(), 1, "one undo step takes it out");
    }

    #[test]
    fn a_song_brings_its_patterns_and_is_selected() {
        let mut s = session();
        s.run(Command::Demos);
        s.overlay_key(OverlayKey::Enter);
        s.overlay_key(OverlayKey::Enter);
        let library = s.editor.library();
        assert_eq!((library.songs.len(), library.patterns.len()), (1, 2));
        assert_eq!(s.editor.selected_song(), 0);
        assert!(s.is_song_mode());
        assert!(s.message.contains("with 1 pattern(s)"), "{}", s.message);
        s.run(Command::Demos);
        s.overlay_key(OverlayKey::Enter);
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(s.editor.library().songs.len(), 1, "loaded twice, kept once");
        assert!(s.message.starts_with("S is already in your library"), "{}", s.message);
    }

    #[test]
    fn a_click_outside_closes_the_browser() {
        let mut s = session();
        s.run(Command::Demos);
        s.overlay_click(1000, 700, Scale::ONE, 1, 1);
        assert!(s.chrome.overlay.is_none());
    }
}
