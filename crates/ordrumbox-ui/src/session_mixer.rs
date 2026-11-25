//! The session's side of the mixer (F12): which track strips it shows, its
//! controls (built at every frame from the pattern, the master bus and the
//! returns), what they do, and the meters and the spectrum it draws.

use super::Session;
use super::Switch;
use super::session_automation_live::MixerMove;
use super::session_panel::{PanelLayout, number};
use super::session_types::Workspace;
use crate::canvas::{Canvas, fit};
use crate::control::{Format, Spec};
use crate::mixer::{self, MixerLayout, MixerView, TRACK_KNOBS};
use crate::output::{self, Param};
use crate::panel::{self, Id, Item, Kind, Rect};
use crate::scale::Scale;
use crate::view::Hit;
use crate::{frame, master, returns, theme};
use ordrumbox_core::{AutomationTarget, num};
use ordrumbox_engine::SCOPE_SIZE;
use ordrumbox_engine::strip_settings::delay_beats_used;
use std::cell::{Cell, RefCell};

/// A send's knob, and a track's pan.
const SEND: Spec = Spec::knob(0.0, 1.0, 0.01, 0.0);
const PAN: Spec = Spec::knob(-1.0, 1.0, 0.01, 0.0);
/// A track's fader: its volume.
const TRACK_FADER: Spec = Spec::fader(0.0, 1.0, 0.01, 1.0);
/// How much of the master's meter is kept from one frame to the next.
const METER_DECAY: f32 = 0.88;

/// The mixer's state: the first track strip shown, its scroll bar held or
/// not; the master's meter, the spectrum's analyser and its last bins.
#[derive(Debug, Default)]
pub(super) struct Mixer {
    first: usize,
    bar_held: bool,
    level: Cell<f32>,
    analyser: RefCell<crate::spectrum::Analyser>,
    spectrum: RefCell<Vec<u8>>,
}

impl Session {
    /// The mixer is shown (the kit editor opens over it).
    pub fn mixer_open(&self) -> bool {
        self.workspace == Workspace::Mixer && !self.kit_editor.is_open()
    }

    /// F12, View > Mixer: the mixer in the place of the grid, or back.
    pub fn toggle_mixer(&mut self) {
        let open = self.workspace == Workspace::Mixer;
        self.set_workspace(if open { Workspace::Grid } else { Workspace::Mixer });
    }

    /// The mixer in the main editor's region of a window of `size`.
    pub(super) fn mixer_layout(&self, size: (i32, i32, Scale)) -> MixerLayout {
        let scale = size.2;
        let body = self.body(size);
        let (left, top) = (frame::margin(scale), frame::body_top(scale));
        let area = Rect::new(left, top, (body.main_w - 2 * left).max(1), (body.main_bottom - top).max(1));
        let track_count = self.pattern().map_or(0, |p| p.tracks.len());
        MixerLayout::new(area, track_count, self.mixer.first, scale)
    }

    /// The mixer's controls: every strip's (its scroll bar is drawn by `mixer.rs`).
    pub(super) fn mixer_items(&self, layout: &mut PanelLayout, size: (i32, i32, Scale)) {
        let mixer = self.mixer_layout(size);
        let scale = size.2;
        self.track_strip_items(&mixer, scale, &mut layout.items);
        self.return_items(&mixer, scale, &mut layout.items);
        self.master_items(&mixer, scale, &mut layout.items);
    }

    fn track_strip_items(&self, mixer: &MixerLayout, scale: Scale, items: &mut Vec<Item>) {
        let Some(pattern) = self.pattern() else { return };
        let edited = self.editor_track();
        for strip in &mixer.tracks {
            let Some(track) = pattern.tracks.get(strip.track) else { continue };
            let t = strip.track;
            let color = if edited == Some(t) { theme::ACCENT } else { theme::text() };
            let name = Kind::Text { text: fit(&track.name, strip.name.w, scale), color };
            items.push(Item::new(Some(Id::MixerName(t)), strip.name, name));
            for (&(key, label), &rect) in TRACK_KNOBS.iter().zip(&strip.knobs) {
                let (spec, own) = match key {
                    "pan" => (PAN, track.pan),
                    "echoSend" => (SEND, track.echo_send),
                    _ => (SEND, track.reverb_send),
                };
                // While the song plays, the knob shows what its lane sets.
                let lane = AutomationTarget::from_track_key(key).and_then(|target| self.automated_now(t, target));
                let value = lane.unwrap_or(own);
                items.push(Item::new(
                    Some(Id::MixerKnob(t, key)),
                    rect,
                    knob(spec, value, label, Format::Fixed2.value(value, "")),
                ));
            }
            let (mute, solo) = self.shown_switch(t);
            for (key, label, on, rect) in [("mute", "M", mute, strip.mute), ("solo", "S", solo, strip.solo)] {
                items.push(Item::new(Some(Id::MixerToggle(t, key)), rect, Kind::Button { label: label.into(), on }));
            }
            let (value, lfo) = self.shown_track_volume(t, track.volume, TRACK_FADER);
            let kind =
                Kind::Value { spec: TRACK_FADER, value, label: None, text: Format::Fixed2.value(value, ""), lfo };
            items.push(Item::new(Some(Id::TrackVolume(t)), strip.fader, kind));
        }
    }

    fn return_items(&self, mixer: &MixerLayout, scale: Scale, items: &mut Vec<Item>) {
        let (echo, reverb) = output::RETURNS.split_at(output::ECHO_CONTROLS);
        for ((strip, params), name) in mixer.returns.iter().zip([echo, reverb]).zip(["ECHO", "REVERB"]) {
            items.push(Item::text(strip.name.x, strip.name.y, name, theme::ACCENT, scale));
            let Some((level, knobs)) = params.split_last() else { continue };
            for (param, &rect) in knobs.iter().zip(&strip.knobs) {
                items.push(self.return_item(param, rect));
            }
            items.push(self.return_item(level, strip.fader));
        }
    }

    fn return_item(&self, param: &Param, rect: Rect) -> Item {
        let value = returns::SECTION.get(&self.output_panel.returns(), param.key);
        let kind = knob(param.spec, value, param.label, self.return_text(param, value));
        Item::new(Some(Id::Returns(param.key)), rect, kind)
    }

    fn master_items(&self, mixer: &MixerLayout, scale: Scale, items: &mut Vec<Item>) {
        let strip = &mixer.master;
        let settings = self.output_panel.master();
        items.push(Item::text(strip.name.x, strip.name.y, "MASTER", theme::ACCENT, scale));
        let item = |param: &Param, rect: Rect| {
            let value = master::SECTION.get(&settings, param.key);
            Item::new(
                Some(Id::Master(param.key)),
                rect,
                knob(param.spec, value, param.label, param.format.value(value, param.unit)),
            )
        };
        let knobs = [&output::PRE_GAIN].into_iter().chain(&output::COMPRESSOR).chain(&output::FILTERS);
        items.extend(knobs.zip(strip.knobs).map(|(param, rect)| item(param, rect)));
        items.push(item(&output::VOLUME, strip.fader));
        // The compressor's on/off button (lit: on), named above it.
        items.push(Item::text(strip.bypass.x, strip.bypass.y - 11 * scale, "Comp", panel::dim(), scale));
        let kind = Kind::Toggle { on: settings.bypass < 0.5 };
        items.push(Item::new(Some(Id::Master("compBypass")), strip.bypass, kind));
    }

    /// What a return's knob shows: its value, and for the echo's time the
    /// beats played when the tempo halves them to fit the echo.
    fn return_text(&self, param: &Param, value: f64) -> String {
        let text = param.format.value(value, param.unit);
        if param.key != "echoBeats" {
            return text;
        }
        let used = delay_beats_used(value, self.tempo());
        if used == value { text } else { format!("{} used", num::text(used)) }
    }

    /// A mixer's button or name clicked.
    pub(super) fn mixer_activate(&mut self, id: Id) {
        match id {
            Id::MixerName(t) => self.click(Hit::Name(t)),
            Id::MixerToggle(t, key) => {
                if let Some(switch) = Switch::from_key(key) {
                    self.switch_track(t, switch);
                }
            }
            Id::Master("compBypass") => {
                let bypass = if self.output_panel.master().bypass >= 0.5 { 0.0 } else { 1.0 };
                self.set_master_value("compBypass", bypass);
            }
            _ => {}
        }
    }

    /// A control of track `t` that a lane can set (its fader, its pan, the
    /// track editor's volume, pan, cutoff and Q) at `value`, in the
    /// control's unit: written to its lane while the song plays and records
    /// automation, else set on the track (its lane taken back).
    pub(super) fn set_track_control(&mut self, t: usize, target: AutomationTarget, value: f64) {
        let moved = self.automate_from_mixer(t, target, value);
        if moved.is_handled() {
            return;
        }
        let desc = format!("{} on {}", target.label(), self.track_name(t));
        self.update_track(t, &[(target.track_key(), number(value))], &desc, true);
        if moved == MixerMove::TakenBack {
            self.say_taken_back(t, target);
        }
    }

    /// A send or the pan of track `t` at `value`.
    pub(super) fn set_mixer_knob(&mut self, t: usize, key: &'static str, value: f64) {
        if let Some(target) = AutomationTarget::from_track_key(key) {
            return self.set_track_control(t, target, value);
        }
        let label = TRACK_KNOBS.iter().find(|(k, _)| *k == key).map_or(key, |(_, label)| label);
        let description = format!("{label} on {}", self.track_name(t));
        self.update_track(t, &[(key, number(value))], &description, true);
    }

    /// The track strips one further on (`later`) or back, held at the ends.
    fn scroll_mixer(&mut self, later: bool) {
        let count = self.pattern().map_or(0, |p| p.tracks.len());
        let fits = self.mixer_layout(self.chrome.panel_size).fits;
        let last_first = count.saturating_sub(fits);
        let first = self.mixer.first.min(last_first);
        self.mixer.first = if later { (first + 1).min(last_first) } else { first.saturating_sub(1) };
    }

    /// A press on the track strips' scroll bar, when there is one: the thumb
    /// goes to `x`, and follows the mouse until it is released; whether it
    /// was the bar's.
    pub fn mixer_bar_press(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let mixer = self.mixer_layout(size);
        if !self.mixer_open() || !mixer.scrolls() || !mixer.scroll_bar.contains(x, y) {
            return false;
        }
        self.mixer.bar_held = true;
        self.mixer_bar_to(size, x);
        true
    }

    /// The held scroll bar moved to `x`; whether to redraw.
    pub(super) fn mixer_bar_to(&mut self, size: (i32, i32, Scale), x: i32) -> bool {
        let first = self.mixer_layout(size).first_at(x);
        let changed = first != self.mixer.first;
        self.mixer.first = first;
        changed
    }

    /// The track strips' scroll bar is held by the mouse.
    pub(super) const fn is_mixer_bar_held(&self) -> bool {
        self.mixer.bar_held
    }

    /// The mouse let go of the track strips' scroll bar.
    pub(super) const fn release_mixer_bar(&mut self) {
        self.mixer.bar_held = false;
    }

    /// Left, Right in the mixer: the track editor edits the track before or
    /// after, its strip scrolled into sight.
    pub fn mixer_select(&mut self, delta: isize) {
        let count = self.pattern().map_or(0, |p| p.tracks.len());
        let Some(last) = count.checked_sub(1) else { return };
        let t = self.editor_track().map_or(0, |t| t.saturating_add_signed(delta).min(last));
        self.click(Hit::Name(t));
        let fits = self.mixer_layout(self.chrome.panel_size).fits;
        self.mixer.first = self.mixer.first.clamp((t + 1).saturating_sub(fits), t);
    }

    /// The wheel over the mixer, off its controls: the track strips scroll; whether it was the mixer's.
    pub(super) fn mixer_wheel(&mut self, size: (i32, i32, Scale), x: i32, y: i32, lines: isize) -> bool {
        if !self.mixer_open() {
            return false;
        }
        let mixer = self.mixer_layout(size);
        let over = mixer.tracks.iter().any(|t| t.back.contains(x, y));
        if over {
            self.chrome.panel_size = size;
            for _ in 0..lines.unsigned_abs() {
                self.scroll_mixer(lines < 0);
            }
        }
        over
    }

    /// The mixer's strips, meters, curve and spectrum, under its controls.
    pub(super) fn draw_mixer(&self, canvas: &mut Canvas, scale: Scale, levels: &[f32]) {
        if !self.mixer_open() {
            return;
        }
        let layout = self.mixer_layout((canvas.width(), canvas.height(), scale));
        let master = self.master_meter();
        let view = MixerView { levels: levels.to_vec(), edited: self.editor_track(), master };
        mixer::draw(canvas, &view, &layout);
    }

    /// The master's settings, its level and the spectrum of the last samples
    /// played (the meter and the analyser keep their smoothing between frames).
    fn master_meter(&self) -> (ordrumbox_engine::MasterSettings, f32, Vec<u8>) {
        let mut peak = 0.0f32;
        if let Some(controller) = self.playback.controller() {
            let mut samples = [0.0f32; SCOPE_SIZE];
            controller.scope(&mut samples);
            peak = samples.iter().fold(0.0, |m, v| m.max(v.abs()));
            *self.mixer.spectrum.borrow_mut() = self.mixer.analyser.borrow_mut().bytes(&samples);
        }
        let level = (self.mixer.level.get() * METER_DECAY).max(peak);
        self.mixer.level.set(level);
        (self.output_panel.master(), level, self.mixer.spectrum.borrow().clone())
    }
}

/// A tall knob of the mixer: its label and value under it.
fn knob(spec: Spec, value: f64, label: &str, text: String) -> Kind {
    Kind::Value { spec, value, label: Some(label.into()), text, lfo: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::test_support::{SIZE, click};
    use ordrumbox_core::Library;

    /// A session on a pattern of `count` tracks, the mixer shown.
    fn mixer_session(count: usize) -> Session {
        let tracks: Vec<String> = (0..count).map(|i| format!(r#"{{"name":"T{i}","notes":[{{}}]}}"#)).collect();
        let json = format!(r#"{{"patterns":[{{"name":"p","beatCount":4,"tracks":[{}]}}]}}"#, tracks.join(","));
        let (library, _) = Library::parse(&json).unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        s.toggle_mixer();
        assert!(s.mixer_open());
        s
    }

    fn rect_of(s: &Session, id: Id) -> Rect {
        panel::find(&s.panel_layout(SIZE.0, SIZE.1, SIZE.2).items, id).map(|i| i.rect).unwrap()
    }

    /// The wheel `lines` notches on control `id`.
    fn wheel_on(s: &mut Session, id: Id, lines: isize) {
        let r = rect_of(s, id);
        assert!(s.wheel(SIZE, r.x + r.w / 2, r.y + 10, lines, false, false));
    }

    #[test]
    fn a_track_strip_sets_its_sends_pan_volume_mute_and_solo() {
        let mut s = mixer_session(3);
        wheel_on(&mut s, Id::MixerKnob(1, "echoSend"), 10);
        wheel_on(&mut s, Id::MixerKnob(1, "reverbSend"), 5);
        wheel_on(&mut s, Id::MixerKnob(1, "pan"), -20);
        let track = s.track(1).unwrap();
        assert!((track.echo_send - 0.1).abs() < 1e-9, "{}", track.echo_send);
        assert!((track.reverb_send - 0.05).abs() < 1e-9);
        assert!((track.pan + 0.2).abs() < 1e-9);
        assert_eq!(s.editor.history().next_undo(), Some("Pan on T1"));
        // The fader: pressed at the top of its groove, the volume at its top.
        let fader = rect_of(&s, Id::TrackVolume(2));
        let (top, _) = panel::fader_bar(fader, SIZE.2);
        s.set_value(Id::TrackVolume(2), 0.3);
        click(&mut s, fader.x + fader.w / 2, top);
        s.mouse_up();
        assert_eq!(s.track(2).unwrap().volume, 1.0);
        let mute = rect_of(&s, Id::MixerToggle(0, "mute"));
        click(&mut s, mute.x + 2, mute.y + 2);
        let solo = rect_of(&s, Id::MixerToggle(2, "solo"));
        click(&mut s, solo.x + 2, solo.y + 2);
        assert!(s.track(0).unwrap().mute && s.track(2).unwrap().solo);
        // A name: the track editor edits that track.
        let name = rect_of(&s, Id::MixerName(2));
        click(&mut s, name.x + 2, name.y + 2);
        assert_eq!(s.editor_track(), Some(2));
    }

    #[test]
    fn the_returns_and_the_master_are_set_and_saved() {
        let mut s = mixer_session(1);
        wheel_on(&mut s, Id::Returns("echoFeedback"), -10);
        assert!((s.output_panel.returns().echo_feedback - 0.45).abs() < 1e-6);
        wheel_on(&mut s, Id::Master("volume"), -10);
        assert!((s.output_panel.master().master - 0.9).abs() < 1e-6);
        assert!(s.saving.master.pending());
        let bypass = rect_of(&s, Id::Master("compBypass"));
        click(&mut s, bypass.x + 2, bypass.y + 2);
        assert_eq!(s.output_panel.master().bypass, 1.0);
        // The levels are faders.
        let level = panel::find(&s.panel_layout(SIZE.0, SIZE.1, SIZE.2).items, Id::Returns("reverbLevel")).cloned();
        assert!(
            matches!(level.map(|i| i.kind), Some(Kind::Value { spec, .. }) if spec.style == crate::control::Style::Fader)
        );
    }

    #[test]
    fn the_track_strips_scroll_and_the_rest_stays_put() {
        let mut s = mixer_session(20);
        let fits = s.mixer_layout(SIZE).fits;
        assert!(fits < 20);
        let master = rect_of(&s, Id::Master("volume"));
        let first = rect_of(&s, Id::MixerName(0));
        // The scroll bar: its thumb dragged to the end and back, a press off it jumps there.
        let layout = s.mixer_layout(SIZE);
        let (bar, (start, len)) = (layout.scroll_bar, layout.thumb());
        assert!(layout.scrolls());
        assert!(s.mixer_bar_press(SIZE, start + len / 2, bar.y + 2));
        assert_eq!(s.pointer_shape(SIZE, 0, 0), crate::ui_input::PointerShape::Grabbing);
        assert!(s.mouse_move(SIZE, bar.x + bar.w + 50, bar.y, false));
        assert_eq!(s.mixer_layout(SIZE).tracks.last().map(|t| t.track), Some(19));
        s.mouse_move(SIZE, bar.x - 50, bar.y, false);
        s.mouse_up();
        assert!(!s.is_mixer_bar_held());
        assert_eq!(rect_of(&s, Id::MixerName(0)), first);
        assert_eq!(s.pointer_shape(SIZE, start + len / 2, bar.y + 2), crate::ui_input::PointerShape::Grab);
        assert!(s.mixer_bar_press(SIZE, bar.x + bar.w - 1, bar.y + 2));
        s.mouse_up();
        assert_eq!(s.mixer_layout(SIZE).tracks.last().map(|t| t.track), Some(19));
        // Not past the bar, nor with every strip shown.
        assert!(!s.mixer_bar_press(SIZE, bar.x + 2, bar.y - 4));
        s.mixer.first = 0;
        // The wheel on a strip's background scrolls on, held at the end.
        let strip = s.mixer_layout(SIZE).tracks[0].back;
        assert!(s.wheel(SIZE, strip.x + 2, strip.y + strip.h - 2, -100, false, false));
        assert_eq!(rect_of(&s, Id::MixerName(19)).x, s.mixer_layout(SIZE).tracks[fits - 1].back.x + 4);
        assert_eq!(rect_of(&s, Id::Master("volume")), master);
        // Left goes back to the first track, its strip scrolled into sight.
        s.mixer_select(-30);
        assert_eq!(s.editor_track(), Some(0));
        assert_eq!(rect_of(&s, Id::MixerName(0)), first);
        s.mixer_select(fits as isize + 2);
        assert!(s.mixer_layout(SIZE).tracks.iter().any(|t| t.track == fits + 2));
    }

    #[test]
    fn nothing_moves_when_a_track_is_muted_selected_or_moved_by_a_host() {
        let mut s = mixer_session(4);
        let rects = |s: &Session| -> Vec<Rect> {
            s.panel_layout(SIZE.0, SIZE.1, SIZE.2).items.iter().filter(|i| i.id.is_some()).map(|i| i.rect).collect()
        };
        let before = rects(&s);
        s.toggle_row(1, "mute");
        s.click(Hit::Name(3));
        s.set_value(Id::MixerKnob(0, "pan"), 1.0);
        assert_eq!(rects(&s), before);
    }

    #[test]
    fn the_echo_time_shows_the_beats_used_when_the_tempo_halves_them() {
        let time = &output::RETURNS[0];
        let s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        assert_eq!(s.return_text(time, 0.75), "0.75 beat");
        let slow = Session::new(Library { bpm: 60.0, ..Library::default() }, KitSet::empty(), Vec::new());
        assert_eq!(slow.return_text(time, 4.0), "2 used");
    }

    #[test]
    fn escape_and_f12_go_back_to_the_grid() {
        let mut s = mixer_session(1);
        s.run(crate::menu::Command::Mixer);
        assert_eq!(s.workspace, Workspace::Grid);
        s.show_view(4);
        assert!(s.mixer_open());
    }
}
