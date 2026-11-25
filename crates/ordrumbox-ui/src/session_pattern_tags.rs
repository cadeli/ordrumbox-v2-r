//! A pattern's tags in the window: their line on the song screen, edited as
//! `funk, bass` (the style first), and the header's PATTERN list, opened as
//! a menu of styles, each opening its patterns.

use super::Session;
use crate::menu_action::PatternAction;
use crate::panel::{Dropdown, Id, Rect, SUBMENU_MARK};
use ordrumbox_core::tags;

/// What the patterns without a style are listed under.
const NO_STYLE: &str = "other";

impl Session {
    /// The tags entry of the selected pattern, its tags as `funk, bass`.
    pub(super) fn pattern_tags_entry(&mut self) {
        let Some(pattern) = self.pattern() else { return };
        let text = tags::to_text(&pattern.tags);
        self.show_entry(Id::PatternTags, None, "Pattern tags (style first, then features):".into(), text);
    }

    /// The tags entry confirmed with `text`: the selected pattern's tags, one
    /// undo step when they change.
    pub(super) fn commit_pattern_tags(&mut self, text: &str) {
        let wanted = tags::parse(text);
        let Some(pattern) = self.pattern() else { return };
        if pattern.tags == wanted {
            return;
        }
        if self.editor.set_pattern_tags(self.pattern_index, &wanted) {
            self.edited();
            let style = self.pattern().and_then(|p| p.style_tag()).unwrap_or(NO_STYLE);
            self.message = format!("tags set: listed under {style}");
        }
    }

    /// The header's PATTERN list under `rect`: the styles, each with how
    /// many patterns it has, the selected pattern's highlighted.
    pub(super) fn pattern_style_menu(&mut self, rect: Rect) {
        let groups = tags::by_style(&self.editor.library().patterns);
        let at = groups.iter().position(|(_, indices)| indices.contains(&self.pattern_index)).unwrap_or(0);
        let lines = groups.iter().enumerate().map(|(i, (style, indices))| {
            let label = format!("{} ({}){SUBMENU_MARK}", style.unwrap_or(NO_STYLE), indices.len());
            (PatternAction::Style(i), label)
        });
        self.chrome.widgets.open_dropdown(Dropdown::from_actions(Id::PatternMenu, rect, lines, at));
    }

    /// A line of the PATTERN menus picked in `menu`: a style opens its
    /// patterns beside it, a pattern is shown (as the list did).
    pub(super) fn pattern_menu_pick(&mut self, action: PatternAction, menu: Dropdown) {
        let group = match action {
            PatternAction::Pattern(index) => return self.show_pattern(index),
            PatternAction::Style(group) => group,
        };
        let patterns = &self.editor.library().patterns;
        let groups = tags::by_style(patterns);
        let Some((style, indices)) = groups.get(group) else { return };
        let title = style.unwrap_or(NO_STYLE).to_owned();
        let lines = indices
            .iter()
            .map(|&i| (PatternAction::Pattern(i), patterns[i].name.clone().unwrap_or_else(|| format!("Pattern {i}"))));
        let at = indices.iter().position(|&i| i == self.pattern_index);
        let mut submenu = Dropdown::submenu(menu, self.chrome.panel_size, Id::PatternMenu, title, lines);
        if let Some(at) = at {
            submenu.highlight(at);
        }
        self.chrome.widgets.open_dropdown(submenu);
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, choose, menu, type_entry};
    use super::*;
    use crate::kits::KitSet;
    use crate::menu::Command;
    use crate::panel;
    use crate::ui_input::Button;
    use ordrumbox_core::Library;

    /// Three patterns: `a` rock, `b` with a bass only, `c` funk.
    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"a","tags":["rock"]},{"name":"b","tags":["bass"]},{"name":"c","tags":["funk","bass"]}]}"#,
        )
        .unwrap();
        Session::new(library, KitSet::empty(), Vec::new())
    }

    #[test]
    fn the_tags_line_edits_the_pattern_s_tags_in_one_step() {
        let mut s = session();
        s.toggle_song_view();
        let line = s.song_layout(SIZE).tags_line();
        s.song_click(SIZE, line.x + line.w / 2, line.y + 2, Button::Left);
        assert_eq!(s.chrome.widgets.entry().map(|e| e.text.as_str()), Some("rock"));
        type_entry(&mut s, " Disco, BASS,disco ");
        assert_eq!(s.pattern().unwrap().tags, ["disco", "bass"]);
        assert_eq!(s.message, "tags set: listed under disco");
        s.undo();
        assert_eq!(s.pattern().unwrap().tags, ["rock"], "one undo step");
        // The same tags written again: no step.
        s.run(Command::PatternTags);
        type_entry(&mut s, "rock");
        assert_eq!(s.editor.history().next_undo(), None);
    }

    #[test]
    fn the_pattern_list_opens_by_style() {
        let mut s = session();
        s.show_pattern(2);
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        let list = panel::find(&layout.items, Id::PatternList).expect("the header's list").rect;
        s.mouse_down(SIZE, list.x + 2, list.y + 2, Button::Left, false);
        let open = s.chrome.widgets.dropdown().expect("the styles");
        assert_eq!(open.labels, ["funk (1) >", "rock (1) >", "other (1) >"]);
        assert_eq!(open.at, 0, "the style of the pattern shown");
        // rock, then its pattern: `a` shown.
        choose(&mut s, 1);
        assert_eq!(menu(&s), [PatternAction::Pattern(0).into()]);
        choose(&mut s, 0);
        assert_eq!(s.pattern_index, 0);
    }
}
