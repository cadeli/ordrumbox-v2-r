//! The clip selected in the song panel: a click selects it (drawn with an
//! outline), DELETE removes it and Ctrl+D repeats it right after itself,
//! the copy then selected. It is known by its pattern and its first
//! measure, so that an edit, an undo or another song never leaves
//! another clip selected in its place.

use super::Session;

impl Session {
    /// The index of the clip selected in the song shown, if it is still there.
    pub(super) fn selected_clip(&self) -> Option<usize> {
        let (id, start) = self.song_panel.selected_clip.as_ref()?;
        self.song()?.clips.iter().position(|c| c.pattern_id == *id && c.start_measure == *start)
    }

    /// Clip `clip` of the song shown selected (`None`: none).
    pub(super) fn select_clip(&mut self, clip: Option<usize>) {
        let picked = clip.and_then(|c| self.song()?.clips.get(c)).map(|c| (c.pattern_id.clone(), c.start_measure));
        self.song_panel.selected_clip = picked;
    }

    /// DELETE: the clip selected removed, one undo step; whether there was one.
    pub fn remove_selected_clip(&mut self) -> bool {
        let Some(clip) = self.selected_clip() else { return false };
        self.undoable(|s| s.remove_clip(clip));
        self.song_panel.selected_clip = None;
        true
    }

    /// Ctrl+D: the clip selected repeated right after itself; whether there was one.
    pub fn repeat_selected_clip(&mut self) -> bool {
        let Some(clip) = self.selected_clip() else { return false };
        self.repeat_clip(clip);
        true
    }

    /// Clip `clip` repeated right after itself (its menu's Next, Ctrl+D), the
    /// copy selected, so that Ctrl+D again goes on.
    pub(super) fn repeat_clip(&mut self, clip: usize) {
        let Some(clip) = self.song().and_then(|song| song.clips.get(clip).cloned()) else { return };
        let label = self.pattern_label(&clip.pattern_id);
        let repeated = self.editor.repeat_pattern_at_measure(clip.start_measure, None);
        if let Ok(copy) = &repeated {
            self.song_panel.selected_clip = Some((copy.pattern_id.clone(), copy.start_measure));
        }
        let result = repeated.map(|_| {
            let at = f64::from(clip.start_measure) + clip.length_measures + 1.0;
            format!("\"{label}\" repeated at measure {}", ordrumbox_core::num::text(at))
        });
        self.song_edit_result(result);
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_song::tests::{SIZE, clips, find_hit, menu, session};
    use crate::song_view::SongHit;
    use crate::ui_input::Button;

    #[test]
    fn a_clip_clicked_is_selected_then_removed_or_repeated_by_its_keys() {
        let mut s = session();
        let (x, y) = find_hit(&s, |hit| hit == SongHit::Clip(0));
        s.song_click(SIZE, x, y, Button::Left);
        s.song_release(SIZE);
        assert_eq!(s.selected_clip(), Some(0));
        assert!(menu(&s).is_empty(), "a left click opens no menu");

        assert!(s.repeat_selected_clip());
        assert_eq!(clips(&s), [("b".into(), 0, 2.0), ("b".into(), 2, 2.0)]);
        assert_eq!(s.selected_clip(), Some(1), "the copy selected");
        assert!(s.remove_selected_clip());
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
        assert_eq!(s.selected_clip(), None);
        assert!(!s.remove_selected_clip(), "nothing selected");
        s.undo();
        assert_eq!(clips(&s).len(), 2, "one undo step");

        // The right button opens its menu, and selects it; the menu shows the keys.
        s.song_click(SIZE, x, y, Button::Right);
        assert_eq!(menu(&s), ["next:0", "delete:0"]);
        let shortcuts = s.chrome.widgets.dropdown().map(|d| d.shortcuts.clone()).unwrap_or_default();
        assert_eq!(shortcuts, crate::menu::shown_shortcuts(&["Ctrl+D", "Delete"]));
        assert_eq!(s.selected_clip(), Some(0));
    }

    #[test]
    fn the_name_under_the_pointer_is_marked() {
        let mut s = session();
        let (x, y) = find_hit(&s, |hit| hit == SongHit::RowName(0));
        assert!(s.song_hover(SIZE, x, y));
        let layout = s.song_layout(SIZE);
        let rows = s.song_rows();
        assert_eq!(s.song_view_data(&rows, &layout).hovered, Some(SongHit::RowName(0)));
        assert!(!s.song_hover(SIZE, x, y), "the same: nothing to redraw");
    }
}
