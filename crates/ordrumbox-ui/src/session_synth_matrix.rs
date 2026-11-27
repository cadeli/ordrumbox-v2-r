//! The session's side of the synth editor's matrix list (`synth_matrix.rs`):
//! the first line shown, its scroll bar held or not, and what the wheel and
//! a press on the bar do to them.

use crate::scale::Scale;
use crate::scroll_thumb;
use crate::synth_matrix::MatrixList;
use crate::ui_input::PointerShape;

use super::Session;

/// Where the matrix list is scrolled to, and whether its bar is held.
#[derive(Debug, Default)]
pub(super) struct MatrixScroll {
    pub(super) first: usize,
    bar_held: bool,
}

impl Session {
    /// The matrix list of the synth editor on a `size` window, when its tab is shown.
    fn synth_matrix_list(&self, size: (i32, i32, Scale)) -> Option<MatrixList> {
        if !self.synth_open() {
            return None;
        }
        self.synth_built(size)?.0.matrix
    }

    /// A press on the matrix's scroll bar, when it scrolls: the thumb goes
    /// to `y`, and follows the mouse until it is released; whether it was the bar's.
    pub fn synth_matrix_bar_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(list) = self.synth_matrix_list(size).filter(|l| l.scrolls() && l.bar.contains(x, y)) else {
            return false;
        };
        self.synth_editor.matrix.bar_held = true;
        self.synth_editor.matrix.first = list.first_at(y);
        true
    }

    /// The held bar moved to `y`; whether to redraw.
    pub(super) fn synth_matrix_bar_to(&mut self, size: (i32, i32, Scale), y: i32) -> bool {
        let Some(list) = self.synth_matrix_list(size) else { return false };
        let first = list.first_at(y);
        let changed = first != list.first;
        self.synth_editor.matrix.first = first;
        changed
    }

    /// The matrix's scroll bar is held by the mouse.
    pub(super) const fn is_synth_matrix_bar_held(&self) -> bool {
        self.synth_editor.matrix.bar_held
    }

    /// The mouse let go of the matrix's scroll bar.
    pub(super) const fn release_synth_matrix_bar(&mut self) {
        self.synth_editor.matrix.bar_held = false;
    }

    /// The wheel `lines` notches over the matrix card, off its controls: its
    /// lines scroll, held at the ends; whether it was the matrix's.
    pub(super) fn synth_matrix_wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, lines: isize) -> bool {
        let Some(list) = self.synth_matrix_list(size) else { return false };
        let over_card = self
            .synth_built(size)
            .is_some_and(|(built, _)| built.cards.iter().any(|c| c.group == "matrix" && c.rect.contains(x, y)));
        if !over_card {
            return false;
        }
        // The wheel turned up (`lines > 0`) goes back up the list.
        let first = if lines > 0 {
            list.first.saturating_sub(lines.unsigned_abs())
        } else {
            list.first.saturating_add(lines.unsigned_abs()).min(list.last_first())
        };
        self.synth_editor.matrix.first = first;
        true
    }

    /// Over the matrix's scroll bar, when it scrolls: the hand on its thumb.
    pub(super) fn synth_matrix_pointer_shape(&self, size: (i32, i32, Scale), x: i32, y: i32) -> PointerShape {
        match self.synth_matrix_list(size) {
            Some(list) if list.scrolls() && list.bar.contains(x, y) => scroll_thumb::hover_shape(list.thumb(), y),
            _ => PointerShape::Arrow,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::kits::KitSet;
    use crate::panel::Id;
    use crate::scale::Scale;
    use crate::session::Session;
    use crate::test_support::SIZE;
    use ordrumbox_core::Library;
    use ordrumbox_engine::SynthPresets;

    /// A session on the synth editor, preset "b" with eight routes.
    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"BASS","useSynth":true,"synthPreset":"b"}]}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        let routes: Vec<String> = (1..=8).map(|n| format!(r#""source{n}":"lfo1""#)).collect();
        let presets = format!(r#"{{"b":{{"matrix":{{{}}}}}}}"#, routes.join(","));
        s.set_asset_synths(SynthPresets::from_json(&ordrumbox_core::json::parse(&presets).unwrap()).unwrap());
        s.set_size(SIZE);
        s.toggle_synth();
        s
    }

    #[test]
    fn the_matrix_scrolls_with_the_wheel_and_its_bar() {
        let mut s = session();
        let list = s.synth_matrix_list(SIZE).expect("the matrix");
        assert!(list.scrolls() && list.total == 9, "eight routes and a free line");
        // The wheel on the bar, down past the end: held at the end.
        let (x, y) = (list.bar.x + 1, list.bar.y + 1);
        assert!(s.wheel(SIZE, x, y, -10, false, false));
        assert_eq!(s.synth_matrix_list(SIZE).unwrap().first, list.last_first());
        assert!(s.wheel(SIZE, x, y, 1, false, false));
        assert_eq!(s.synth_matrix_list(SIZE).unwrap().first, list.last_first() - 1);
        // The bar pressed at its top, dragged to its foot, let go.
        assert!(s.synth_matrix_bar_press(SIZE, x, list.bar.y));
        assert_eq!(s.synth_matrix_list(SIZE).unwrap().first, 0);
        assert!(s.mouse_move(SIZE, x, list.bar.y + list.bar.h, false));
        assert_eq!(s.synth_matrix_list(SIZE).unwrap().first, list.last_first());
        s.mouse_up();
        assert!(!s.is_synth_matrix_bar_held());
        assert!(!s.synth_matrix_bar_press(SIZE, list.bar.x - 20, list.bar.y), "off the bar");
    }

    #[test]
    fn a_target_list_offers_every_oscillator_width() {
        let s = session();
        let mut draft = s.synths.presets.get("b").unwrap().as_object().unwrap().clone();
        ordrumbox_engine::synth_model::hydrate(&mut draft);
        let fields = crate::synth_view::all_fields(&draft);
        let fi = fields.iter().position(|f| f == "matrix.target1").unwrap() as u16;
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        let spin = crate::panel::find(&layout.items, Id::SynthField(fi)).expect("the first line's target");
        let crate::panel::Kind::Spin { values, .. } = &spin.kind else { panic!("{:?}", spin.kind) };
        for vco in ["vco1", "vco2", "vco3"] {
            let at = |key: &str| values.iter().position(|v| *v == format!("{vco}.{key}"));
            assert_eq!(at("width"), at("detune").map(|i| i + 1), "{vco}'s width with its card: {values:?}");
        }
    }

    #[test]
    fn every_control_of_the_synth_editor_has_a_tooltip() {
        let s = session();
        let mut draft = s.synths.presets.get("b").unwrap().as_object().unwrap().clone();
        ordrumbox_engine::synth_model::hydrate(&mut draft);
        let paths = crate::synth_view::all_fields(&draft);
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        let fields: Vec<_> =
            layout.items.iter().filter_map(|i| i.id).filter(|id| matches!(id, Id::SynthField(_))).collect();
        assert!(fields.len() > 40, "{}", fields.len());
        for id in fields {
            let Id::SynthField(fi) = id else { continue };
            let tip = s.control_tip(id).unwrap_or_else(|| panic!("{}: no tooltip", paths[usize::from(fi)]));
            assert!(tip.iter().all(|line| !line.is_empty()), "{id:?}: {tip:?}");
        }
    }

    #[test]
    fn an_envelope_fader_says_its_value_in_its_tooltip() {
        let mut s = session();
        let mut draft = s.synths.presets.get("b").unwrap().as_object().unwrap().clone();
        ordrumbox_engine::synth_model::hydrate(&mut draft);
        let fields = crate::synth_view::all_fields(&draft);
        let fi = fields.iter().position(|f| f == "envelope.attack").unwrap() as u16;
        let tip = s.control_tip(Id::SynthField(fi)).expect("a tooltip");
        assert_eq!(tip.map(|line| line.into_owned()), ["Envelope Attack".to_owned(), "0 s".to_owned()]);
        let knob = fields.iter().position(|f| f == "fm.amount").unwrap() as u16;
        let tip = s.control_tip(Id::SynthField(knob)).expect("a knob says its value in its tooltip");
        assert_eq!(tip.map(|line| line.into_owned()), ["FM".to_owned(), "0".to_owned()]);
        let gain = fields.iter().position(|f| f == "vco1.gain").unwrap() as u16;
        let tip = s.control_tip(Id::SynthField(gain)).expect("an oscillator's knob says it in its tooltip");
        assert_eq!(tip[0], "VCO1 Gain");
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        let fader = crate::panel::find(&layout.items, Id::SynthField(fi)).unwrap().rect;
        assert!(s.control_hover((SIZE.0, SIZE.1, Scale::ONE), fader.x + fader.w / 2, fader.y + 10));
    }
}
