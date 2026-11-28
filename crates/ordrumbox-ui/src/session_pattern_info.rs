//! A pattern's name, description and tags, typed in one form: the header's
//! INFO, one undo step for the three.

use super::Session;
use crate::panel::{Entry, Field, Id};
use ordrumbox_core::tags;

/// The form's labels, in the order of its fields.
const LABELS: [&str; 3] = ["Name", "Description", "Tags"];

impl Session {
    /// The form of the selected pattern: its name, description and tags
    /// (as `funk, bass`, the style first).
    pub(super) fn pattern_info_entry(&mut self) {
        let Some(pattern) = self.pattern() else {
            self.message = "No pattern selected".into();
            return;
        };
        let name = pattern.name.clone().unwrap_or_else(|| format!("Pattern {}", self.pattern_index));
        let texts = [name, pattern.description.clone(), tags::to_text(&pattern.tags)];
        let fields = LABELS.iter().zip(texts).map(|(label, text)| Field { label: (*label).into(), text }).collect();
        let at = self.chrome.widgets.pointer;
        let title = "Pattern info (tags: style first, then features):".to_owned();
        self.chrome.widgets.open_entry(Entry::form(Id::PatternInfo, title, fields, at));
    }

    /// A form confirmed with its fields' `texts`.
    pub(super) fn commit_form(&mut self, id: Id, texts: &[&str]) {
        if let (Id::PatternInfo, [name, description, tags]) = (id, texts) {
            self.commit_pattern_info(name, description, tags);
        }
    }

    /// The selected pattern's name (an empty one keeps the old), description
    /// and tags, one undo step when any changes.
    fn commit_pattern_info(&mut self, name: &str, description: &str, tag_text: &str) {
        let p = self.pattern_index;
        let Some(pattern) = self.pattern() else { return };
        let (name, description, wanted) = (name.trim(), description.trim(), tags::parse(tag_text));
        let rename = !name.is_empty() && pattern.name.as_deref() != Some(name);
        let describe = pattern.description != description;
        let retag = pattern.tags != wanted;
        if !(rename || describe || retag) {
            return;
        }
        let shown = if rename { name.to_owned() } else { pattern.name.clone().unwrap_or_default() };
        let revision = self.editor.revision();
        self.editor.batch(&format!("Edit pattern info \"{shown}\""), |editor| {
            if rename {
                editor.rename_pattern(p, name);
            }
            if describe {
                editor.set_pattern_description(p, description);
            }
            if retag {
                editor.set_pattern_tags(p, &wanted);
            }
        });
        if self.editor.revision() != revision {
            self.edited();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_panel::PanelKey;
    use super::super::session_song::tests::{SIZE, type_entry};
    use super::*;
    use crate::kits::KitSet;
    use crate::menu::Command;
    use ordrumbox_core::Library;

    fn session() -> Session {
        let (library, _) =
            Library::parse(r#"{"patterns":[{"name":"a","description":"old","tags":["rock"]}]}"#).unwrap();
        Session::new(library, KitSet::empty(), Vec::new())
    }

    #[test]
    fn the_info_form_edits_name_description_and_tags_in_one_step() {
        let mut s = session();
        s.run(Command::PatternInfo);
        let entry = s.chrome.widgets.entry().expect("the form");
        assert_eq!(entry.texts().collect::<Vec<_>>(), ["a", "old", "rock"]);
        // The name, then Tab to the description and the tags.
        *s.chrome.widgets.entry_mut().unwrap().text_mut().unwrap() = " Verse ".into();
        s.panel_key(SIZE, PanelKey::Tab, false, false);
        s.panel_text('!');
        s.panel_key(SIZE, PanelKey::Tab, false, false);
        type_entry(&mut s, "Funk, bass");
        let pattern = s.pattern().unwrap();
        assert_eq!(pattern.name.as_deref(), Some("Verse"));
        assert_eq!(pattern.description, "old!");
        assert_eq!(pattern.tags, ["funk", "bass"]);
        s.undo();
        let pattern = s.pattern().unwrap();
        assert_eq!((pattern.name.as_deref(), pattern.description.as_str()), (Some("a"), "old"), "one undo step");
        assert_eq!(pattern.tags, ["rock"]);
    }

    #[test]
    fn the_info_form_unchanged_or_cancelled_adds_no_step() {
        let mut s = session();
        s.run(Command::PatternInfo);
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.editor.history().next_undo(), None, "unchanged");
        s.run(Command::PatternInfo);
        s.panel_text('x');
        s.panel_key(SIZE, PanelKey::Escape, false, false);
        assert!(s.chrome.widgets.entry().is_none());
        assert_eq!(s.editor.history().next_undo(), None, "cancelled");
        // Up and Shift+Tab go round to the tags.
        s.run(Command::PatternInfo);
        s.panel_key(SIZE, PanelKey::Up, false, false);
        assert_eq!(s.chrome.widgets.entry().map(Entry::text), Some("rock"));
        s.panel_key(SIZE, PanelKey::Tab, true, false);
        assert_eq!(s.chrome.widgets.entry().map(Entry::text), Some("old"));
    }
}
