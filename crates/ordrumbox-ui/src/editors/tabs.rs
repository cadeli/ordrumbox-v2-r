//! The track editor's tabs (FX, sound, LFO, loop, generation) as rows.

use super::tables::{FX_DEFS, FX_KNOBS, FxDef, GENERATION_COMPOSE};
use super::tables::{
    GENERATION_ENGINE, GENERATION_GROOVY, GENERATION_MANUAL, GenerationRow, LFO_TARGETS, LOOP_PROPS, LfoTarget, Prop,
    fx_label, prop, track_select_options,
};
use super::values::{
    EditorState, flag, fx_on, has_lfo, index_of, lfo_wave_choice, loop_steps, number, select_choice, text_of,
};
use super::{Choice, ComposeView, TrackView};
use crate::control::{Format, Spec, Style};
use crate::panel::{GenerationTab, Id, Item, Kind, LfoParam, Rect, dim, label_width};
use crate::scale::Scale;
use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::{DelayType, Track, num};
use ordrumbox_engine::strip_settings::delay_beats_used;
use ordrumbox_logic::generate;

use super::layout::*;
use super::note_kit::shape_controls;

/// The FX tab: the effects one under the other, each a cartridge of its
/// own, as in the LFO tab: its LED, its name and its type in the label
/// column, then its knobs and its choices.
pub(super) fn fx_tab(rows: &mut Rows, view: &TrackView, state: &EditorState) {
    for (i, fx) in FX_DEFS.iter().enumerate() {
        fx_strip(rows, view, state, i as u8, fx);
    }
}

/// Slots of an effect strip's knob rows: the most controls a row has.
pub(super) const FX_SLOTS: i32 = 4;

/// Height of a strip cartridge's lines, its name's and its type's (at scale 1).
const CARTRIDGE_LINE_H: i32 = 14;

/// The y under the strip of effect `fx` started at `top`: its rows of knobs
/// and the gap to the next strip.
pub(super) fn fx_strip_bottom(top: i32, fx: &FxDef, scale: Scale) -> i32 {
    top + fx.controls.len() as i32 * knob_row_h(scale) + FX_STRIP_GAP * scale
}

pub(super) fn fx_strip(rows: &mut Rows, view: &TrackView, state: &EditorState, i: u8, fx: &FxDef) {
    let s = rows.scale;
    let on = fx_on(view.track, fx);
    let bottom = fx_strip_bottom(rows.y, fx, s);
    // Under its name: its type on a screen.
    let line = strip_cartridge(rows, (Id::FxPower(i), on), fx.label, fx.controls.len() as i32);
    if let Some(key) = fx.type_key() {
        let screen = (Id::TrackSelect(key), fx_choice(view, state, key).screen(on));
        rows.screen_arrows(line, screen, |step| Id::TrackSelectStep(key, step));
    }
    let (x, w) = (rows.x, rows.w);
    rows.x += label_width(s);
    rows.w -= label_width(s);
    for &row in fx.controls {
        fx_knob_row(rows, view, state, fx, row);
        rows.y += knob_row_h(s);
    }
    (rows.x, rows.w) = (x, w);
    rows.y = bottom;
}

/// One row of an effect strip's controls, at the current y: the knobs first,
/// then the choices, so every strip reads the same way.
fn fx_knob_row(rows: &mut Rows, view: &TrackView, state: &EditorState, fx: &FxDef, row: &[&'static str]) {
    let track = view.track;
    let type_key = fx.type_key();
    let knobs = row.iter().filter_map(|&key| FX_KNOBS.iter().find(|p| p.key == key));
    let choices = row.iter().filter(|&&key| Some(key) != type_key && !FX_KNOBS.iter().any(|p| p.key == key));
    let mut slot = 0;
    for p in knobs {
        let rect = rows.slot(slot, FX_SLOTS);
        slot += 1;
        // A slap is one echo: no feedback to set, its knob kept greyed out.
        if p.key == "delayFeedback" && track.delay_type == DelayType::Slap {
            rows.slot_choice(
                rect,
                p.label,
                Some(p.key),
                None,
                Kind::Disabled { label: "one echo".into(), style: Style::Knob },
            );
            continue;
        }
        let value = view.lfo.iter().find(|(k, _)| *k == p.key).map_or_else(|| number(track, p.key), |&(_, v)| v);
        let mut spec = value_spec(p, default_of(p.key, p.min), true);
        // Ranges over decades: a logarithmic knob.
        if matches!(p.format, Format::Freq | Format::FineMillis) {
            spec = spec.log_full_travel();
        }
        let text = p.format.value(value, "");
        let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text, lfo: has_lfo(track, p.key) };
        rows.items.push(Item::new(Some(Id::Fx(p.key)), rect, kind));
    }
    for &key in choices {
        let rect = rows.slot(slot, FX_SLOTS);
        slot += 1;
        let kind = match key {
            "sidechainSource" => view.sidechains.kind(),
            "filterSlope" => fx_choice(view, state, key).two_state(),
            _ => fx_choice(view, state, key).kind(),
        };
        rows.slot_choice(rect, fx_label(key), Some(key), Some(Id::TrackSelect(key)), kind);
    }
}

/// The choice of the effect setting `key` (a select of fixed options).
fn fx_choice(view: &TrackView, state: &EditorState, key: &str) -> Choice {
    let track = view.track;
    let mut choice = select_choice(track, key, state);
    if key == "delayBeats" {
        // Too long a delay plays at fewer beats: shown on the choice.
        let used = delay_beats_used(track.delay_beats, view.bpm);
        if used != track.delay_beats
            && let Some(label) = choice.labels.get_mut(choice.selected)
        {
            *label = format!("{label} ({} used)", num::text(used));
        }
    }
    choice
}

/// Pixels between two effect strips (at scale 1).
pub(super) const FX_STRIP_GAP: i32 = 4;

/// How far a strip's cartridge reaches out of the column (at scale 1).
const STRIP_FRAME_PAD: i32 = 3;

/// A strip's cartridge (an effect's, an LFO's): its frame round the strip's
/// `knob_rows` rows, in the gap between two strips, its LED (`led`, lit when `on`) and its name
/// in the label column. Returns the line under the name, where the strip
/// shows its type.
fn strip_cartridge(rows: &mut Rows, (led, on): (Id, bool), name: &str, knob_rows: i32) -> Rect {
    let s = rows.scale;
    let pad = STRIP_FRAME_PAD * s;
    let frame = Rect::new(rows.x - pad, rows.y - s.px(), rows.w + 2 * pad, knob_rows * knob_row_h(s) + 2 * s);
    let head = Rect::new(rows.x, rows.y + 2 * s, label_width(s) - 4 * s, CARTRIDGE_LINE_H * s);
    rows.cartridge_head(frame, head, (led, on), name);
    Rect::new(head.x, head.y + head.h + 4 * s, head.w, head.h)
}

pub(super) fn sound_tab(rows: &mut Rows, view: &TrackView) {
    let s = rows.scale;
    let track = view.track;
    for (key, label, on) in [("mono", "Mono", track.mono), ("legato", "Legato", track.legato)] {
        let r = rows.row();
        rows.label(r, label, None, false);
        let field = rows.field(r, 36 * s);
        rows.items.push(Item::new(Some(Id::Toggle(key)), field, Kind::Toggle { on }));
    }
    choke_select(rows, track);
    shape_controls(rows, &view.shape, (Id::SampleKnob, Id::SampleToggle));
    let r = rows.row();
    rows.items.push(Item::new(
        Some(Id::AutoAssign),
        Rect::new(r.x, r.y, 12 * s, r.h),
        Kind::Led { on: track.auto_assign },
    ));
    rows.items.push(Item::text(r.x + 14 * s, r.y + (r.h - 8 * s) / 2, "auto", dim(), s));
    let r = rows.row();
    rows.label(r, "Instrument", None, false);
    rows.items.push(Item::new(Some(Id::Instrument), rows.field(r, r.w), view.instruments.kind()));
    // The sample's choice, then the button that loads one from a file.
    let r = rows.row();
    rows.label(r, "Sample", None, false);
    let field = rows.field(r, r.w);
    let load = Rect::new(field.x + field.w - LOAD_W * s, field.y, LOAD_W * s, field.h);
    let choice_field = Rect::new(field.x, field.y, field.w - (LOAD_W + 4) * s, field.h);
    rows.items.push(Item::new(Some(Id::Sample), choice_field, view.samples.kind()));
    let button = Kind::Button { label: "File...".into(), on: false };
    rows.items.push(Item::new(Some(Id::LoadSample), load, button));
    synth_cartridge(rows, view);
}

/// The Sound tab's choke group: off, or the group whose tracks cut each other.
fn choke_select(rows: &mut Rows, track: &Track) {
    let key = "chokeGroup";
    let r = rows.row();
    rows.label(r, "Choke", None, false);
    let (values, labels) = track_select_options(key);
    let selected = index_of(&values, &text_of(track, key));
    let choice = Choice { values, labels, selected };
    rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 72 * rows.scale), choice.kind()));
}

/// Width of the Sound tab's File... button (at scale 1): its label and a margin.
pub(super) const LOAD_W: i32 = 48;

/// The synth cartridge's margin around its row (at scale 1).
const CARTRIDGE_PAD: i32 = 4;

/// The Sound tab's synth, a cartridge of its own: its LED (on: the track
/// plays the synth instead of its sample), its name, then the preset on a
/// one-line screen between the arrows that step through the presets.
fn synth_cartridge(rows: &mut Rows, view: &TrackView) {
    let s = rows.scale;
    let pad = CARTRIDGE_PAD * s;
    let frame = Rect::new(rows.x, rows.y, rows.w, row_h(s) + 2 * pad);
    let r = Rect::new(frame.x + pad, frame.y + pad, frame.w - 2 * pad, row_h(s));
    let on = view.track.use_synth;
    rows.cartridge_head(frame, r, (Id::SynthOn, on), "SYNTH");
    // The screen and its arrows line up with the fields of the rows above.
    let x = frame.x + label_width(s);
    let line = Rect::new(x, r.y + s.px(), r.x + r.w - x, r.h - 2 * s);
    rows.screen_arrows(line, (Id::Synth, view.synths.screen(on)), Id::SynthStep);
    rows.y = frame.y + frame.h + 2 * s;
}

/// The LFO tab: the five targets one under the other, each a cartridge of
/// its own, as in the FX tab: its LED and its name, its wave on a screen
/// under them, then its knobs.
pub(super) fn lfo_tab(rows: &mut Rows, view: &TrackView) {
    for (i, target) in LFO_TARGETS.iter().enumerate() {
        lfo_strip(rows, view.track, i as u8, target);
    }
}

/// Slots of an LFO strip's knob row: frequency, min, max, phase.
pub(super) const LFO_SLOTS: i32 = 4;

pub(super) fn lfo_strip(rows: &mut Rows, track: &Track, i: u8, target: &LfoTarget) {
    let s = rows.scale;
    let lfo = track.field(target.lfo_key).and_then(|v| v.as_object().cloned());
    // Off: the target's own defaults; on: what the LFO plays.
    let get = |key: &str, fallback: f64| lfo.as_ref().map_or(fallback, |o| ordrumbox_core::lfo::setting(o, key));
    let on = lfo.is_some();
    let line = strip_cartridge(rows, (Id::LfoPower(i), on), target.label, 1);
    let choice = lfo_wave_choice(track, target);
    rows.screen_arrows(line, (Id::LfoType(i), choice.screen(on)), |step| Id::LfoTypeStep(i, step));

    // The knobs right of the label column; min and max: the target's range,
    // its own end the default of each.
    let (x, w) = (rows.x, rows.w);
    rows.x += label_width(s);
    rows.w -= label_width(s);
    let range = |default: f64| {
        let spec = Spec::knob(target.min, target.max, target.step, default);
        if target.key == "filterCutoff" { spec.log_full_travel() } else { spec }
    };
    let knobs = [
        (LfoParam::Freq, Spec::knob(0.1, 2.0, 0.1, 1.0), get("freq", 1.0)),
        (LfoParam::Min, range(target.min), get("min", target.min)),
        (LfoParam::Max, range(target.max), get("max", target.max)),
        (LfoParam::Phase, Spec::knob(0.0, 1.0, 0.01, 0.0), get("phase", 0.0)),
    ];
    for (slot, (param, spec, value)) in (0..).zip(knobs) {
        let text = crate::control::fmt(value);
        let kind = Kind::Value { spec, value, label: Some(param.label().to_owned()), text, lfo: false };
        rows.items.push(Item::new(Some(Id::Lfo(i, param)), rows.slot(slot, LFO_SLOTS), kind));
    }
    (rows.x, rows.w) = (x, w);
    rows.y += knob_row_h(s) + FX_STRIP_GAP * s;
}

/// The loop tab of `track`, its groove amount at the value `moved` gives it
/// (a song lane's, while it plays) when it gives one.
pub(super) fn loop_tab(rows: &mut Rows, track: &Track, moved: &[(&'static str, f64)], state: &EditorState) {
    let spb = track.steps_per_beat;
    let max_steps = f64::from(track.beat_count * spb);
    for p in &LOOP_PROPS {
        let default = default_of(p.key, p.min);
        let (value, spec, format) = match p.key {
            "stepsPerBeat" => (f64::from(spb), value_spec(p, default, false), p.format),
            // The loop length ends at the track's length.
            "loopLength" => (
                loop_steps(track).min(max_steps),
                value_spec(&Prop { max: max_steps, ..*p }, p.min, false),
                Format::LoopLength(spb),
            ),
            key => (moved_value(moved, key).unwrap_or(track.groove_amount), value_spec(p, default, false), p.format),
        };
        if p.key == "grooveAmount" {
            groove_select(rows, track, state);
        }
        let is_moved = moved_value(moved, p.key).is_some();
        rows.slider(Id::Loop(p.key), spec, value, p.label, format.value(value, ""), is_moved);
    }
}

/// The generation tab of a track (`moved`: values a song lane gives its sliders while it plays).
pub(super) fn generation_tab(
    rows: &mut Rows,
    (track, moved): (&Track, &[(&'static str, f64)]),
    compose: Option<&ComposeView>,
    state: &EditorState,
) {
    let s = rows.scale;
    // The simple view: auto, lock, a new line, three settings.
    generation_toggle(rows, ("autoMode", "Auto"), track.auto_mode, state);
    generation_toggle(rows, ("composeLock", "Lock"), track.compose_lock, state);
    let r = rows.row();
    rows.label(r, "New line", None, false);
    let button = Kind::Button { label: "\u{21bb}".into(), on: false };
    rows.items.push(Item::new(Some(Id::Regenerate), rows.field(r, 36 * s), button));
    let melodic = instrument_family(&track.name).is_melodic();
    // The part above Advanced takes the compose view's height whatever the
    // track: Advanced stays put when the sound changes.
    let simple_bottom = simple_part_bottom(rows.y, s);
    match compose {
        Some(view) => {
            rows.text(&format!("{} / {}  (* = style)", view.style, view.shown.part), dim());
            for p in GENERATION_COMPOSE.iter().filter(|p| SIMPLE_COMPOSE.contains(&p.key)) {
                compose_slider(rows, track, view, p, state);
            }
        }
        None if melodic => {
            variant_select(rows, track, state);
            generation_slider(rows, (track, moved), &GENERATION_DENSITY, state);
        }
        None => rows.text("no part for this track in the pattern's style", dim()),
    }
    rows.y = rows.y.max(simple_bottom);
    // Everything else, folded.
    let r = rows.row();
    let label = if state.generation_advanced { "Advanced \u{25be}" } else { "Advanced \u{25b8}" };
    let button = Kind::Button { label: label.into(), on: state.generation_advanced };
    rows.items.push(Item::new(Some(Id::GenerationAdvanced), Rect::new(r.x, r.y + 2 * s, 120 * s, r.h - 4 * s), button));
    if state.generation_advanced {
        advanced_generation(rows, (track, moved), compose, state);
    }
}

/// A yes/no setting `key` of the generation tab, shown as `label`.
fn generation_toggle(rows: &mut Rows, (key, label): (&'static str, &str), on: bool, state: &EditorState) {
    let r = rows.row();
    rows.label(r, label, Some(key), state.selected_prop == Some(key));
    rows.items.push(Item::new(Some(Id::Toggle(key)), rows.field(r, 36 * rows.scale), Kind::Toggle { on }));
}

/// The unfolded Advanced part of the generation tab: its tabs (Groovy and
/// Engine, or Manual; Compose) and the settings of the one shown.
fn advanced_generation(
    rows: &mut Rows,
    (track, moved): (&Track, &[(&'static str, f64)]),
    compose: Option<&ComposeView>,
    state: &EditorState,
) {
    let s = rows.scale;
    let compose_tab = compose.is_some() && state.generation_tab == GenerationTab::Compose;
    // Three slots whatever shows: Compose stays put when Auto turns Engine off.
    let tabs = if track.auto_mode {
        [
            Some((
                Id::GenerationTab(GenerationTab::Groovy),
                None,
                "Groovy",
                !compose_tab && state.generation_tab == GenerationTab::Groovy,
                false,
            )),
            Some((
                Id::GenerationTab(GenerationTab::Engine),
                None,
                "Engine",
                !compose_tab && state.generation_tab == GenerationTab::Engine,
                false,
            )),
            None,
        ]
    } else {
        [Some((Id::GenerationTab(GenerationTab::Groovy), None, "Manual", !compose_tab, false)), None, None]
    };
    let [first, second, _] = tabs;
    let compose_slot =
        compose.map(|_| (Id::GenerationTab(GenerationTab::Compose), None, "Compose", compose_tab, false));
    rows.tabs(&[first, second, compose_slot]);
    if let (true, Some(view)) = (compose_tab, compose) {
        for p in &GENERATION_COMPOSE {
            compose_slider(rows, track, view, p, state);
        }
        return;
    }
    if !track.auto_mode {
        for p in &GENERATION_MANUAL {
            generation_slider(rows, (track, moved), p, state);
        }
        return;
    }
    for row in if state.generation_tab == GenerationTab::Groovy { &GENERATION_GROOVY } else { &GENERATION_ENGINE } {
        match row {
            // A drum line comes from its style (Compose); a melodic one has them above.
            GenerationRow::Select("autoVariant", _) | GenerationRow::Slider(Prop { key: "autoDensity", .. }) => {}
            GenerationRow::Slider(p) => generation_slider(rows, (track, moved), p, state),
            GenerationRow::Toggle(key, label) => generation_toggle(rows, (key, label), flag(track, key), state),
            GenerationRow::Select(key, label) => {
                let r = rows.row();
                rows.label(r, label, Some(key), state.selected_prop == Some(key));
                let values = track_select_options(key).0;
                let selected = index_of(&values, &text_of(track, key));
                let choice = Choice { values, labels: Vec::new(), selected };
                rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * s), choice.kind()));
            }
        }
    }
}

/// The y under the Generate tab's simple part (from `top`): its tallest, a style's
/// line and its compose sliders.
pub(super) fn simple_part_bottom(top: i32, scale: Scale) -> i32 {
    let mut rows = Rows { x: 0, w: 0, y: top, scale, items: Vec::new() };
    rows.y += 12 * scale;
    for _ in SIMPLE_COMPOSE {
        rows.row();
    }
    rows.y
}

/// The rows of the Generate tab's tallest sub-tab.
pub(super) const GENERATION_ROWS: usize =
    max_len(&[GENERATION_GROOVY.len(), GENERATION_ENGINE.len(), GENERATION_COMPOSE.len(), GENERATION_MANUAL.len()]);

pub(super) const fn max_len(lens: &[usize]) -> usize {
    let mut max = 0;
    let mut i = 0;
    while i < lens.len() {
        if lens[i] > max {
            max = lens[i];
        }
        i += 1;
    }
    max
}

/// The y under the tallest tab (from `top`, under the main tabs): the Gen
/// tab's Advanced settings, or the FX tab's strips. The note editor starts
/// there whatever the tab, so it never moves.
pub(super) fn tallest_tab_bottom(top: i32, scale: Scale) -> i32 {
    let mut rows = Rows { x: 0, w: 0, y: top, scale, items: Vec::new() };
    // Auto, Lock, New line.
    for _ in 0..3 {
        rows.row();
    }
    rows.y = simple_part_bottom(rows.y, scale);
    rows.row(); // Advanced
    rows.tabs(&[]);
    for _ in 0..GENERATION_ROWS {
        rows.row();
    }
    let fx_bottom = FX_DEFS.iter().fold(top, |y, fx| fx_strip_bottom(y, fx, scale));
    rows.y.max(fx_bottom)
}

/// The compose settings of the simple view.
pub(super) const SIMPLE_COMPOSE: [&str; 3] = ["composeHitsPerMeasure", "composeComplexity", "composeVariation"];

/// A melodic track's density (the simple view).
pub(super) const GENERATION_DENSITY: Prop = prop("autoDensity", "Density", -1.0, 1.0, 0.01, Format::Density);

pub(super) fn generation_slider(
    rows: &mut Rows,
    (track, moved): (&Track, &[(&'static str, f64)]),
    p: &Prop,
    state: &EditorState,
) {
    let s = rows.scale;
    let shown = moved_value(moved, p.key);
    let is_moved = shown.is_some();
    let value = match (shown, p.key) {
        (Some(value), _) => value,
        // Unset until the generator picks them: the amounts used meanwhile.
        (None, "autoHitVariation") => track.auto_hit_amount(),
        (None, "autoNoteVariation") => track.auto_note_amount(),
        (None, key) => number(track, key),
    };
    let spec = value_spec(p, default_of(p.key, p.min), false);
    let r = rows.row();
    if state.selected_prop == Some(p.key) {
        rows.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
    }
    let text = p.format.value(value, "");
    let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text, lfo: is_moved };
    rows.items.push(Item::new(Some(Id::Generation(p.key)), r, kind));
}

/// The value `moved` gives the setting `key`, if it gives one.
fn moved_value(moved: &[(&'static str, f64)], key: &str) -> Option<f64> {
    moved.iter().find(|(k, _)| *k == key).map(|&(_, value)| value)
}

/// The loop tab's groove: a preset of its table (`Custom` for one of the file's own).
fn groove_select(rows: &mut Rows, track: &Track, state: &EditorState) {
    let key = "groove";
    let r = rows.row();
    rows.label(r, "Groove", Some(key), state.selected_prop == Some(key));
    let choice = select_choice(track, key, state);
    rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * rows.scale), choice.kind()));
}

/// The variant a melodic track plays (`""`: its style's).
pub(super) fn variant_select(rows: &mut Rows, track: &Track, state: &EditorState) {
    let key = "autoVariant";
    let r = rows.row();
    rows.label(r, "Variant", Some(key), state.selected_prop == Some(key));
    let values = variant_options(track);
    let selected = index_of(&values, &text_of(track, key));
    let choice = Choice { values, labels: Vec::new(), selected };
    rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * rows.scale), choice.kind()));
}

/// The variants a melodic track can play (`""`: its style's).
pub fn variant_options(track: &Track) -> Vec<String> {
    let kind = if instrument_family(&track.name) == InstrumentFamily::Bass { "bass" } else { "melody" };
    std::iter::once(String::new()).chain(generate::variant_names(kind).into_iter().map(str::to_owned)).collect()
}

/// A setting of `settings` by its key.
pub(super) fn compose_value(key: &str, settings: &ordrumbox_logic::compose::Settings) -> f64 {
    match key {
        "composeHitsPerMeasure" => settings.hits_per_measure,
        "composeComplexity" => settings.complexity,
        "composeSyncopation" => settings.syncopation,
        "composeCoupling" => settings.coupling,
        "composeVariation" => settings.variation,
        "composeStyleWeight" => settings.style_weight,
        _ => 0.0,
    }
}

/// One compose slider: the track's value, else the style's (its text ends
/// in `*`; a double-click gives a value back to the style); `a→b` when the
/// pattern's macros make it play `b`; the hits show the most a measure holds
/// when asked for more.
pub(super) fn compose_slider(rows: &mut Rows, track: &Track, view: &ComposeView, p: &Prop, state: &EditorState) {
    let s = rows.scale;
    let own_key = matches!(p.key, "composeStyleWeight" | "composeSeed");
    let own = track.field(p.key).and_then(|v| v.as_f64());
    let value =
        if own_key { number(track, p.key) } else { own.unwrap_or_else(|| compose_value(p.key, &view.shown.base)) };
    let mut text = p.format.value(value, "");
    if !own_key {
        let used = compose_value(p.key, &view.shown.used);
        let used = if p.key == "composeHitsPerMeasure" { used.min(view.shown.open_hits as f64) } else { used };
        if (used - value).abs() >= 0.5 {
            text = format!("{text}\u{2192}{}", num::text(used.round()));
        }
        if own.is_none() {
            text.push('*');
        }
    }
    let r = rows.row();
    if state.selected_prop == Some(p.key) {
        rows.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
    }
    let spec = value_spec(p, default_of(p.key, p.min), false);
    let spec = if own_key { spec } else { spec.unset_by_default() };
    let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text, lfo: false };
    rows.items.push(Item::new(Some(Id::Compose(p.key)), r, kind));
}
