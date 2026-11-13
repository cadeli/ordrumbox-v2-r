//! What the generators read and draw from: the scales and styles of the
//! assets, what the library's tags teach, the style chosen, the random draws.

use ordrumbox_core::{Rng, ScaleList, Scales, Styles};
use ordrumbox_logic::Editor;
use ordrumbox_logic::tools::learned_styles;

/// The session's generation sources.
#[derive(Debug)]
pub(super) struct Generators {
    /// What the generators read (`scales.json`).
    pub(super) scales: Scales,
    /// The scale names, in file order (arpeggios).
    pub(super) scale_list: ScaleList,
    /// The styles of `styles.json`.
    pub(super) styles: Styles,
    /// Those styles with what the library's tags teach, and the library
    /// revision they were learned at.
    pub(super) learned_styles: (Styles, Option<u64>),
    /// The style B and D generate in (Generate menu); `None`: the pattern's, else a drawn one.
    pub(super) style: Option<String>,
    /// Random draws of the generators.
    pub(super) rng: Rng,
}

impl Default for Generators {
    fn default() -> Self {
        Self {
            scales: Scales::default(),
            scale_list: ScaleList::default(),
            styles: Styles::default(),
            learned_styles: (Styles::default(), None),
            style: None,
            rng: Rng::from_time(),
        }
    }
}

impl Generators {
    /// The styles the generation follows now: `styles.json` with what the
    /// library's tags teach, learned again when the library changed.
    pub(super) fn learn(&mut self, editor: &Editor) -> &Styles {
        learned_styles(&mut self.learned_styles, &self.styles, editor)
    }

    /// The styles the header and the Generate tab show: what the library taught,
    /// else (before the first frame learned it) the written ones.
    pub(super) fn shown_styles(&self) -> &Styles {
        if self.learned_styles.1.is_some() { &self.learned_styles.0 } else { &self.styles }
    }

    /// The loop start of pattern `p` (loop `pass` of the playback): new lines for
    /// its tracks in auto mode, in the styles the library teaches; whether any changed.
    pub(super) fn auto_loop(&mut self, editor: &mut Editor, p: usize, pass: u64) -> bool {
        let styles = self.learn(editor).clone();
        let rng = &mut self.rng;
        editor.auto_loop(p, pass, &self.scales, &styles, &mut || rng.next_f64())
    }
}
