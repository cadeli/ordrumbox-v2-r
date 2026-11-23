//! The handles of a sample's waveform, in the kit editor and in the track
//! editor's sample bar: a press near the start, the end, a loop point or the
//! end of the decay, dragged sideways, moves it. The start, the end and the
//! loop's points land on the nearest zero crossing, so that they do not
//! click. Kit edits, as the knobs: not undo steps.

use super::Session;
use crate::kits::{DECAY_RANGE, Field};
use crate::panel::{Kind, Rect};
use crate::sample_overlay::{Handle, Overlay, Shape};
use crate::scale::Scale;

/// How far from a handle a press still takes it, px at scale 1.
const HANDLE_REACH: i32 = 5;
/// How far a point looks for a zero crossing, seconds either side.
const SNAP_SECONDS: f64 = 0.003;

/// A handle of a waveform held by the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WaveDrag {
    kit: usize,
    sample: usize,
    handle: Handle,
    /// The waveform, on screen.
    wave: Rect,
}

/// A sample's waveform on screen: which sample, where.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Wave {
    kit: usize,
    sample: usize,
    rect: Rect,
}

impl Handle {
    /// The setting it moves.
    const fn field(self) -> Field {
        match self {
            Self::Start => Field::Start,
            Self::End => Field::End,
            Self::LoopStart => Field::LoopStart,
            Self::LoopEnd => Field::LoopEnd,
            Self::DecayEnd => Field::Decay,
        }
    }
}

impl Session {
    /// The waveform at `(x, y)`: the kit editor's, else the track editor's.
    fn wave_at(&self, size: (i32, i32, Scale), x: i32, y: i32) -> Option<Wave> {
        let (width, height, scale) = size;
        let layout = self.panel_layout(width, height, scale);
        if let Some(sample) = self.kit_editor.sample {
            let rect = self.kit_layout(size).envelope_rect(layout.details_top);
            if rect.contains(x, y) {
                return Some(Wave { kit: self.kit_index, sample, rect });
            }
        }
        let item = layout.items.iter().find(|i| matches!(i.kind, Kind::Envelope { .. }) && i.rect.contains(x, y))?;
        let url = self.editor_track().and_then(|t| self.track_sample_path(t))?;
        let (kit, sample) = self.kits.find_path(&url)?;
        Some(Wave { kit, sample, rect: item.rect })
    }

    /// The playback settings of sample `sample` of kit `kit`, its length known.
    fn wave_shape(&self, kit: usize, sample: usize) -> Option<Shape> {
        let instrument = self.kits.sample(kit, sample)?;
        let duration_ms = self.kits.buffer(&instrument.path)?.seconds() * 1000.0;
        Some(Shape::of(instrument, duration_ms)).filter(|shape| shape.duration_ms > 0.0)
    }

    /// The handle at `(x, y)`, and the waveform it is on.
    fn wave_handle_at(&self, size: (i32, i32, Scale), x: i32, y: i32) -> Option<(Wave, Handle)> {
        let wave = self.wave_at(size, x, y)?;
        let overlay = Overlay::new(&self.wave_shape(wave.kit, wave.sample)?)?;
        let span = f64::from((wave.rect.w - 1).max(1));
        let reach = f64::from(HANDLE_REACH * size.2) / span;
        let handle = overlay.handle_at(f64::from(x - wave.rect.x) / span, reach)?;
        Some((wave, handle))
    }

    /// A handle is under the pointer: the pointer shows a sideways drag.
    pub(super) fn is_over_wave_handle(&self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        self.wave_handle_at(size, x, y).is_some()
    }

    /// A press at `(x, y)`: on a handle, the drag of it starts. Whether it was one.
    pub(super) fn press_wave_handle(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some((wave, handle)) = self.wave_handle_at(size, x, y) else { return false };
        self.kit_editor.wave_drag = Some(WaveDrag { kit: wave.kit, sample: wave.sample, handle, wave: wave.rect });
        true
    }

    /// The mouse at `x` with a handle held: the handle there, its point on
    /// the nearest zero crossing; whether to redraw.
    pub(super) fn wave_drag_move(&mut self, x: i32) -> bool {
        let Some(WaveDrag { kit, sample, handle, wave }) = self.kit_editor.wave_drag else { return false };
        let Some(shape) = self.wave_shape(kit, sample) else { return false };
        let fraction = f64::from(x - wave.x) / f64::from((wave.w - 1).max(1));
        let mut value = shape.dragged(handle, fraction);
        if handle != Handle::DecayEnd
            && let Some(buffer) = self.kits.sample(kit, sample).and_then(|s| self.kits.buffer(&s.path))
        {
            let snapped = buffer.nearest_zero_crossing(value / 1000.0, SNAP_SECONDS) * 1000.0;
            // Snapped, then kept within its bounds again.
            value = shape.dragged(handle, snapped / shape.duration_ms);
        }
        let field = handle.field();
        let value = if field == Field::Decay { value.clamp(DECAY_RANGE.0, DECAY_RANGE.1).round() } else { value };
        self.set_kit_sample_value(kit, sample, field, value);
        true
    }

    /// A handle is held.
    pub(super) fn is_wave_dragging(&self) -> bool {
        self.kit_editor.wave_drag.is_some()
    }

    /// Let go of the handle held.
    pub(super) fn release_wave_handle(&mut self) {
        self.kit_editor.wave_drag = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::test_support::SIZE;
    use crate::ui_input::{Button, PointerShape};
    use ordrumbox_core::Library;
    use ordrumbox_engine::SampleBuffer;
    use ordrumbox_engine::kit::{BankSample, Kit, KitSample, SampleBank};
    use std::sync::Arc;

    /// A kit of one 1 s sample, a square wave at 50 Hz (a zero crossing every 10 ms),
    /// on the track editor's track; the kit editor open.
    fn session() -> Session {
        let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"}]}]}"#).unwrap();
        let sample = KitSample { key: "KICK".into(), path: "k/kick.wav".into(), ..KitSample::default() };
        let kit = Kit { name: "k".into(), samples: vec![sample], ..Kit::default() };
        let square: Vec<f32> = (0..1000).map(|i| if (i / 10) % 2 == 0 { 0.5 } else { -0.5 }).collect();
        let buffer = Arc::new(SampleBuffer::mono(square, 1000));
        let bank = SampleBank { samples: vec![BankSample { kit: "k".into(), sample: kit.samples[0].clone(), buffer }] };
        let mut s = Session::new(library, KitSet::new(vec![kit], &bank, None, None, None), Vec::new());
        s.set_size(SIZE);
        s.kits_changed("kits loaded".into());
        s.toggle_kit_editor();
        s
    }

    /// The x of `fraction` of the kit editor's waveform, and its middle's y.
    fn at(s: &Session, fraction: f64) -> (i32, i32) {
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        let r = s.kit_layout(SIZE).envelope_rect(layout.details_top);
        (r.x + (fraction * f64::from(r.w - 1)).round() as i32, r.y + r.h / 2)
    }

    #[test]
    fn a_handle_dragged_moves_its_point_onto_a_zero_crossing() {
        let mut s = session();
        let (x, y) = at(&s, 1.0);
        assert_eq!(s.pointer_shape(SIZE, x, y), PointerShape::ResizeHorizontal, "over the end");
        assert!(s.mouse_down(SIZE, x, y, Button::Left, false));
        assert_eq!(s.pointer_shape(SIZE, 0, 0), PointerShape::ResizeHorizontal, "held");
        let (x, _) = at(&s, 0.62);
        s.mouse_move(SIZE, x, y, false);
        s.mouse_up();
        assert_eq!(s.kits.sample(0, 0).unwrap().end, Some(620.0), "on the crossing at 620 ms");
        assert!(s.kit_editor.wave_drag.is_none());
    }

    #[test]
    fn the_loop_points_have_handles_only_while_it_loops() {
        let mut s = session();
        s.set_kit_sample_value(0, 0, Field::LoopStart, 300.0);
        let (x, y) = at(&s, 0.3);
        assert!(!s.is_over_wave_handle(SIZE, x, y), "no loop");
        s.set_kit_sample_value(0, 0, Field::Loop, 1.0);
        assert!(s.is_over_wave_handle(SIZE, x, y));
        assert!(s.mouse_down(SIZE, x, y, Button::Left, false));
        let (x, _) = at(&s, 0.41);
        s.mouse_move(SIZE, x, y, false);
        s.mouse_up();
        assert_eq!(s.kits.sample(0, 0).unwrap().loop_start, Some(410.0), "on the crossing at 410 ms");
    }

    #[test]
    fn a_press_away_from_the_handles_is_no_drag() {
        let mut s = session();
        // Between the end of the decay (at 505 ms) and the end of the sample.
        let (x, y) = at(&s, 0.75);
        assert!(!s.press_wave_handle(SIZE, x, y));
        assert_eq!(s.pointer_shape(SIZE, x, y), PointerShape::Arrow);
    }
}
