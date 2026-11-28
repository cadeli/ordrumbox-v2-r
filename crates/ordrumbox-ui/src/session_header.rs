//! The header's widgets, on the metal and cards of `view/header.rs`: the
//! transport and tempo, the pattern (its list, its length, its name, notes
//! and tags), its editing, its generation (style, auto mode, the drum, bass
//! and chord lines, scale, key, macros), the kit and the views. Every
//! control that acts on the whole pattern is here; the menus keep them too.

use ordrumbox_core::steps::MAX_BEATS;
use ordrumbox_core::style::{self, Macros};
use ordrumbox_core::{Pattern, num, song};
use ordrumbox_logic::generate;

use super::Session;
use super::session_labels::style_label;
use super::session_panel::PanelLayout;
use super::session_types::Workspace;
use crate::canvas::fit;
use crate::control::Spec;
use crate::font;
use crate::menu::Command;
use crate::panel::hardware::CARD_HEAD;
use crate::panel::{Id, Item, Kind, Rect, Skin, ViewButton};
use crate::scale::Scale;
use crate::theme;
use crate::view::{self, HeaderCard, HeaderControls};

/// The pattern list offers every length up to this one, in beats...
const EVERY_BEAT_UP_TO: i32 = 16;
/// ...then every length this many beats apart, up to the longest.
const LONG_BEAT_STEP: usize = 4;

/// The pattern card's buttons: new, duplicate, and the form of its name,
/// description and tags.
const PATTERN_BUTTONS: [(&str, Command); 3] =
    [("NEW", Command::NewPattern), ("DUP", Command::DuplicatePattern), ("INFO", Command::PatternInfo)];
/// The edit card's buttons.
const EDIT_BUTTONS: [(&str, Command); 4] = [
    ("QUANT", Command::Quantize),
    ("HUMAN", Command::Humanize),
    ("CLEAR", Command::ClearPattern),
    ("COMPACT", Command::CompactTracks),
];
/// The generation's line buttons, lit while the lines they make are in auto mode.
const PART_BUTTONS: [(&str, Command); 3] =
    [("DRUMS", Command::GenerateDrums), ("BASS", Command::GenerateBass), ("CHORDS", Command::GenerateChords)];
/// The kit card's buttons: the sound assignment of Shift+G, H, J and K.
const KIT_BUTTONS: [(&str, Command); 4] = [
    ("RND", Command::SelectRandomDrumkit),
    ("SYNTH", Command::ConvertToSynth),
    ("ASSIGN", Command::AutoAssignAll),
    ("SHUF", Command::RandomSamples),
];
/// The semitones a pattern's key moves its generated lines up (0–11).
const KEYS: i32 = 12;
/// The space between a card's edge and its title (at scale 1).
const TITLE_PAD: i32 = 6;

impl Session {
    /// The header: its metal, its cards and their controls.
    pub(super) fn header_items(&self, layout: &mut PanelLayout, scale: Scale) {
        let c = view::header_controls(scale);
        layout.items.push(Item::new(None, c.metal, Kind::Panel).skinned(Skin::Metal));
        for card in HeaderCard::ALL {
            layout.items.push(Item::new(None, c.card(card), Kind::Card).skinned(Skin::Card));
        }
        self.card_titles(layout, &c, scale);
        let first = layout.items.len();
        self.header_selects(layout, &c);
        command_buttons(layout, &KIT_BUTTONS, c.kit_buttons);
        if let Some(pattern) = self.pattern() {
            self.transport_items(layout, &c);
            if self.workspace != Workspace::Song {
                self.pattern_items(layout, pattern, &c);
                self.generation_items(layout, pattern, &c);
            }
        } else {
            let at = c.card(HeaderCard::Transport);
            let y = c.play.y + (c.play.h - 8 * scale) / 2;
            layout.items.push(Item::text(at.x + TITLE_PAD * scale, y, "no pattern", theme::muted(), scale));
        }
        self.view_buttons(layout, &c);
        // The controls in the cards' look; the texts keep their own colours.
        for item in &mut layout.items[first..] {
            if !matches!(item.kind, Kind::Text { .. } | Kind::Mark { .. }) {
                item.skin = Skin::Card;
            }
        }
    }

    /// The cards' titles in their bands. The pattern card's says SONG in
    /// song mode, and NEXT (blinking) while the pattern shown waits for the
    /// one playing to end its loop, framing the list.
    fn card_titles(&self, layout: &mut PanelLayout, c: &HeaderControls, s: Scale) {
        let waiting = self.waiting_blink().filter(|_| self.workspace != Workspace::Song);
        for card in HeaderCard::ALL {
            let r = c.card(card);
            let (title, color) = match card {
                HeaderCard::Pattern if self.workspace == Workspace::Song => (view::SONG_TITLE, theme::text()),
                HeaderCard::Pattern if waiting.is_some() => {
                    (view::WAITING_TITLE, if waiting == Some(true) { theme::LIVE } else { theme::muted() })
                }
                _ => (card.title(), theme::text()),
            };
            let y = r.y + (CARD_HEAD * s - 8 * s) / 2 + s.px();
            let title = fit(title, r.w - 2 * TITLE_PAD * s, s);
            layout.items.push(Item::text(r.x + TITLE_PAD * s, y, title, color, s));
        }
        if waiting == Some(true) {
            let r = c.pattern;
            let (t, o) = (s.px(), 2 * s.px());
            let sides = [
                Rect::new(r.x - o, r.y - o, r.w + 2 * o, t),
                Rect::new(r.x - o, r.y + r.h + o - t, r.w + 2 * o, t),
                Rect::new(r.x - o, r.y - o, t, r.h + 2 * o),
                Rect::new(r.x + r.w + o - t, r.y - o, t, r.h + 2 * o),
            ];
            for side in sides {
                layout.items.push(Item::new(None, side, Kind::Mark { color: theme::LIVE }));
            }
        }
    }

    /// The pattern (or, in song mode, the song) and kit lists.
    fn header_selects(&self, layout: &mut PanelLayout, c: &HeaderControls) {
        let library = self.editor.library();
        if self.workspace == Workspace::Song {
            self.song_select(layout, c.song());
        } else if !library.patterns.is_empty() {
            let labels = library
                .patterns
                .iter()
                .enumerate()
                .map(|(i, p)| p.name.clone().unwrap_or_else(|| format!("Pattern {i}")))
                .collect();
            let values = (0..library.patterns.len()).map(|i| i.to_string()).collect();
            let selected = self.pattern_index.min(library.patterns.len() - 1);
            layout.items.push(Item::new(Some(Id::PatternList), c.pattern, Kind::Select { values, labels, selected }));
        }
        if !self.kits.kits().is_empty() {
            let labels = self.kits.kits().iter().map(|k| k.name.clone()).collect();
            let values = (0..self.kits.kits().len()).map(|i| i.to_string()).collect();
            let selected = self.kit_index.min(self.kits.kits().len() - 1);
            layout.items.push(Item::new(Some(Id::KitList), c.kit, Kind::Select { values, labels, selected }));
        }
    }

    /// The view buttons (the main editor, then the kit editor): the one shown lit.
    fn view_buttons(&self, layout: &mut PanelLayout, c: &HeaderControls) {
        let kit_open = self.kit_editor.is_open();
        for (button, rect) in ViewButton::ALL.into_iter().zip(c.views) {
            let on = match button {
                // The song's slot has no button: the mode switch is its own.
                ViewButton::Song => continue,
                ViewButton::Kit => kit_open,
                _ => !kit_open && self.workspace.view_button() == button,
            };
            layout.items.push(Item::new(
                Some(Id::View(button)),
                rect,
                Kind::Button { label: button.label().into(), on },
            ));
        }
    }

    /// The mode switch, PLAY, REC (song mode) and the tempo knob.
    fn transport_items(&self, layout: &mut PanelLayout, c: &HeaderControls) {
        let song = self.is_song_mode();
        for (i, (rect, label)) in c.mode.iter().zip(view::MODES).enumerate() {
            let kind = Kind::Button { label: label.into(), on: song == (i == 1) };
            layout.items.push(Item::new(Some(Id::Mode(i as u8)), *rect, kind));
        }
        let kind = Kind::Transport { playing: self.playing() };
        layout.items.push(Item::new(Some(Id::Transport), c.play, kind));
        // REC in song mode; its slot stays empty in pattern mode.
        if song {
            let kind = Kind::Button { label: "REC".into(), on: self.is_recording_automation() };
            layout.items.push(Item::new(Some(Id::RecordAutomation), c.record, kind));
        }
        let spec = Spec::knob(song::MIN_BPM, song::MAX_BPM, 1.0, song::DEFAULT_BPM);
        let bpm = self.tempo();
        let kind = Kind::Value { spec, value: bpm, label: Some("BPM".into()), text: num::text(bpm), lfo: false };
        layout.items.push(Item::new(Some(Id::Bpm), c.bpm, kind));
    }

    /// The pattern's length, its buttons and the edit card's.
    fn pattern_items(&self, layout: &mut PanelLayout, pattern: &Pattern, c: &HeaderControls) {
        let (values, selected) = beat_choices(pattern.beat_count);
        let labels = values.iter().map(|v| format!("{v} beats")).collect();
        layout.items.push(Item::new(Some(Id::Beats), c.beats, Kind::Select { values, labels, selected }));
        command_buttons(layout, &PATTERN_BUTTONS, c.pattern_buttons);
        command_buttons(layout, &EDIT_BUTTONS, c.edit_buttons);
    }

    /// The generation card: the pattern's style, ♫, AUTO, the line buttons,
    /// its scale and key, the macros.
    fn generation_items(&self, layout: &mut PanelLayout, pattern: &Pattern, c: &HeaderControls) {
        let styles = self.shown_styles();
        let style = pattern.extra.get(generate::STYLE_KEY).and_then(|g| g.as_str()).unwrap_or_default();
        let values: Vec<String> =
            std::iter::once(String::new()).chain(styles.list.iter().map(|s| s.name.clone())).collect();
        let labels: Vec<String> =
            std::iter::once("random".to_owned()).chain(styles.list.iter().map(style_label)).collect();
        let selected = styles.get(style).and_then(|st| values.iter().position(|v| *v == st.name)).unwrap_or(0);
        layout.items.push(Item::new(Some(Id::StyleList), c.style, Kind::Select { values, labels, selected }));
        let button = Kind::Button { label: font::REGENERATE.into(), on: false };
        layout.items.push(Item::new(Some(Id::GeneratePattern), c.generate, button));
        let button = Kind::Button { label: "AUTO".into(), on: self.pattern_auto() };
        layout.items.push(Item::new(Some(Id::AutoAll), c.auto, button));
        for (((label, command), rect), on) in PART_BUTTONS.iter().zip(c.parts).zip(self.generation()) {
            layout.items.push(Item::new(
                Some(Id::Command(*command)),
                rect,
                Kind::Button { label: (*label).into(), on },
            ));
        }
        layout.items.push(Item::new(Some(Id::PatternScale), c.scale, scale_spin(pattern)));
        layout.items.push(Item::new(Some(Id::PatternKey), c.key, key_spin(pattern)));
        let macros = Macros::of(pattern);
        for (rect, (key, label)) in c.macros.iter().zip(view::MACROS) {
            let value = match key {
                style::ENERGY => macros.energy,
                style::COMPLEXITY => macros.complexity,
                _ => macros.variation,
            };
            let spec = Spec::knob(0.0, 100.0, 1.0, style::MACRO_NEUTRAL);
            let kind = Kind::Value { spec, value, label: Some(label.into()), text: num::text(value), lfo: false };
            layout.items.push(Item::new(Some(Id::Macro(key)), *rect, kind));
        }
    }
}

/// Buttons that run the menus' `buttons`, one in each of `rects`.
fn command_buttons<const N: usize>(layout: &mut PanelLayout, buttons: &[(&str, Command); N], rects: [Rect; N]) {
    for ((label, command), rect) in buttons.iter().zip(rects) {
        let kind = Kind::Button { label: (*label).into(), on: false };
        layout.items.push(Item::new(Some(Id::Command(*command)), rect, kind));
    }
}

/// The scale spin: the scales a pattern is generated in, its own selected
/// (else the first, the sections' natural minor).
fn scale_spin(pattern: &Pattern) -> Kind {
    let names = generate::scale_names();
    let current = generate::pattern_scale(pattern);
    let selected = current.and_then(|s| names.iter().position(|n| *n == s)).unwrap_or(0);
    let labels = names.iter().map(|n| short_scale(n).to_owned()).collect();
    Kind::spin(names.iter().map(|n| (*n).to_owned()).collect(), labels, selected)
}

/// A scale's name short enough for the spin's screen.
fn short_scale(name: &str) -> &str {
    match name {
        "natural minor" => "minor",
        "pentatonic minor" => "penta minor",
        "pentatonic major" => "penta major",
        "blues scale" => "blues",
        "harmonic minor" => "harm. minor",
        other => other,
    }
}

/// The key spin: the semitones the generated lines move up, 0–11.
fn key_spin(pattern: &Pattern) -> Kind {
    // A whole number of semitones from the file; rounding is meant.
    let current = (generate::pattern_key(pattern).round() as i32).rem_euclid(KEYS);
    let values: Vec<String> = (0..KEYS).map(|k| k.to_string()).collect();
    let labels = (0..KEYS).map(|k| format!("key +{k}")).collect();
    Kind::spin(values, labels, usize::try_from(current).unwrap_or(0))
}

/// The lengths the beats list offers, and the index of `current`, added when it
/// is none of them (set by the keys).
fn beat_choices(current: i32) -> (Vec<String>, usize) {
    let long = (EVERY_BEAT_UP_TO..=MAX_BEATS).step_by(LONG_BEAT_STEP).skip(1);
    let mut beats: Vec<i32> = (1..=EVERY_BEAT_UP_TO).chain(long).collect();
    let selected = beats.binary_search(&current).unwrap_or_else(|at| {
        beats.insert(at, current);
        at
    });
    (beats.iter().map(i32::to_string).collect(), selected)
}
