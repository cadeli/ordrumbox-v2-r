//! The track editor's cartridges: their LED, their screen and their arrows.

use super::super::Session;
use crate::kits::KitSet;
use crate::panel::{Id, Step};
use ordrumbox_core::json;
use ordrumbox_core::{DelayType, FilterType, Groove, Library};
use ordrumbox_engine::SynthPresets;

/// A pattern of one KICK track, three synth presets.
fn session() -> Session {
    let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"}]}]}"#).unwrap();
    let mut s = Session::new(library, KitSet::empty(), Vec::new());
    let presets = json::parse(r#"{"a":{},"b":{},"c":{}}"#).unwrap();
    s.set_asset_synths(SynthPresets::from_json(&presets).unwrap());
    s
}

/// What the track plays: its synth on, its preset.
fn plays(s: &Session) -> (bool, Option<String>) {
    let track = &s.pattern().unwrap().tracks[0];
    (track.use_synth, track.synth_preset.clone())
}

/// The preset the screen shows.
fn shown(s: &Session) -> String {
    let view = s.track_view(0).unwrap();
    view.synths.values[view.synths.selected].clone()
}

#[test]
fn the_arrows_choose_and_the_led_plays() {
    let mut s = session();
    assert_eq!(s.track_view(0).unwrap().synths.values, ["a", "b", "c"]);
    assert_eq!(shown(&s), "a");
    // Off: the arrows only choose the preset.
    s.activate(Id::SynthStep(Step::Previous));
    assert_eq!(plays(&s), (false, Some("c".into())));
    s.activate(Id::SynthStep(Step::Next));
    assert_eq!(plays(&s), (false, Some("a".into())));
    s.select(Id::Synth, "b");
    assert_eq!((plays(&s), shown(&s)), ((false, Some("b".into())), "b".into()));
    // The LED plays the preset shown, and takes the track off auto-assign.
    s.activate(Id::SynthOn);
    assert_eq!(plays(&s), (true, Some("b".into())));
    assert!(!s.pattern().unwrap().tracks[0].auto_assign);
    // On: a step plays the next preset at once.
    s.activate(Id::SynthStep(Step::Next));
    assert_eq!(plays(&s), (true, Some("c".into())));
    // Off again: the sample back, the preset kept for the LED.
    s.activate(Id::SynthOn);
    assert_eq!((plays(&s), shown(&s)), ((false, Some("c".into())), "c".into()));
    s.undo();
    assert_eq!(plays(&s), (true, Some("c".into())));
}

#[test]
fn without_presets_the_led_says_so() {
    let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"}]}]}"#).unwrap();
    let mut s = Session::new(library, KitSet::empty(), Vec::new());
    s.activate(Id::SynthOn);
    assert_eq!(plays(&s), (false, None));
    assert_eq!(s.message, "No synth presets loaded.");
    s.activate(Id::SynthStep(Step::Next));
    assert_eq!(s.message, "No synth presets loaded.");
}

#[test]
fn an_effect_s_arrows_step_through_its_types() {
    let mut s = session();
    let reverb = |s: &Session| {
        let track = &s.pattern().unwrap().tracks[0];
        (track.reverb_type.clone(), track.reverb_mix)
    };
    // A new track's reverb has no type: its screen shows the one its LED
    // turns on; an arrow turns it on at the next type, as the filter's and an LFO's do.
    assert_eq!(reverb(&s), ("none".into(), 0.0));
    s.activate(Id::TrackSelectStep("reverbType", Step::Next));
    assert_eq!(reverb(&s), ("hall".into(), 0.5));
    s.activate(Id::TrackSelectStep("reverbType", Step::Previous));
    s.activate(Id::TrackSelectStep("reverbType", Step::Previous));
    assert_eq!(reverb(&s).0, "gated", "from the first type, the previous is the last");
    // Off, a type chosen turns it back on at its amount.
    s.activate(Id::FxPower(0));
    assert_eq!(reverb(&s), ("gated".into(), 0.0));
    s.select(Id::TrackSelect("reverbType"), "plate");
    assert_eq!(reverb(&s), ("plate".into(), 0.5));
}

#[test]
fn a_groove_is_picked_by_name_and_keeps_the_amount_set() {
    let mut s = session();
    let groove = |s: &Session| {
        let track = &s.pattern().unwrap().tracks[0];
        (track.groove.clone(), track.groove_amount)
    };
    s.select(Id::TrackSelect("groove"), "Swing 16th");
    assert_eq!(groove(&s), (Groove::swing(2), 0.5), "an amount, so that the pick is heard");
    s.set_value(Id::Loop("grooveAmount"), 0.2);
    s.select(Id::TrackSelect("groove"), "Accent");
    assert_eq!(groove(&s), (Groove::preset("Accent").unwrap(), 0.2), "the amount set is kept");
    s.select(Id::TrackSelect("groove"), "Custom");
    assert_eq!(groove(&s).0, Groove::preset("Accent").unwrap(), "Custom shows a table of its own, it is not a pick");
    s.select(Id::TrackSelect("groove"), "Straight");
    assert_eq!(groove(&s), (Groove::default(), 0.2));
}

#[test]
fn a_type_turns_on_an_effect_with_an_amount_but_no_type() {
    let mut s = session();
    let p = s.pattern_index;
    let updates = [("delayMix", json::Value::from(0.25)), ("delayType", json::Value::from("none"))];
    assert!(s.editor.update_track(p, 0, &updates, None, false));
    let delay = |s: &Session| {
        let track = &s.pattern().unwrap().tracks[0];
        (track.delay_type, track.delay_mix)
    };
    assert_eq!(delay(&s), (DelayType::None, 0.25));
    s.activate(Id::TrackSelectStep("delayType", Step::Next));
    assert_eq!(delay(&s), (DelayType::PingPong, 0.25), "the screen showed tape; its amount is kept");
}

#[test]
fn an_effect_s_led_gives_it_a_type() {
    let mut s = session();
    let reverb = |s: &Session| {
        let track = &s.pattern().unwrap().tracks[0];
        (track.reverb_type.clone(), track.reverb_mix)
    };
    s.activate(Id::FxPower(0));
    assert_eq!(reverb(&s), ("room".into(), 0.5));
    s.activate(Id::FxPower(0));
    assert_eq!(reverb(&s), ("room".into(), 0.0));
    // An amount without a type plays nothing: off, and on keeps the amount.
    s.select(Id::TrackSelect("reverbType"), "none");
    let p = s.pattern_index;
    assert!(s.editor.update_track(p, 0, &[("reverbMix", json::Value::from(0.25))], None, false));
    s.activate(Id::FxPower(0));
    assert_eq!(reverb(&s), ("room".into(), 0.25));
}

#[test]
fn an_lfo_s_arrows_step_through_its_waves() {
    let mut s = session();
    let lfo = |s: &Session| s.pattern().unwrap().tracks[0].filter_cutoff_lfo.clone();
    // Off: the screen shows sine, the wave its LED turns on; an arrow turns it on at the next.
    assert!(lfo(&s).is_none());
    s.activate(Id::LfoTypeStep(0, Step::Next));
    let wave = |s: &Session| lfo(s).and_then(|o| o.get("type").and_then(json::Value::as_str).map(str::to_owned));
    assert_eq!(wave(&s).as_deref(), Some("triangle"));
    s.activate(Id::LfoTypeStep(0, Step::Previous));
    s.activate(Id::LfoTypeStep(0, Step::Previous));
    assert_eq!(wave(&s).as_deref(), Some("random"), "from the first wave, the previous is the last");
    // Its LED turns it off.
    s.activate(Id::LfoPower(0));
    assert!(lfo(&s).is_none());
}

#[test]
fn the_filter_s_type_is_on_its_screen() {
    let mut s = session();
    let filter = |s: &Session| {
        let track = &s.pattern().unwrap().tracks[0];
        (track.filter_type, track.filter_cutoff)
    };
    let (bypass, cutoff) = filter(&s);
    assert_eq!(bypass, FilterType::Allpass);
    // Off, the screen shows lowpass; an arrow turns the filter on at the next
    // type, its cutoff up from 20 Hz as its LED would.
    s.activate(Id::TrackSelectStep("filterType", Step::Next));
    let (kind, on_cutoff) = filter(&s);
    assert_eq!(kind, FilterType::Highpass);
    assert!(on_cutoff > cutoff);
    s.activate(Id::TrackSelectStep("filterType", Step::Previous));
    s.activate(Id::TrackSelectStep("filterType", Step::Previous));
    assert_eq!(filter(&s).0, FilterType::Notch, "from the first type, the previous is the last");
    // The LED turns it off, then back on at the type it had.
    s.activate(Id::FxPower(5));
    assert_eq!(filter(&s).0, FilterType::Allpass);
    s.activate(Id::FxPower(5));
    assert_eq!(filter(&s).0, FilterType::Notch);
}

#[test]
fn an_arpeggio_s_spin_steps_through_its_directions() {
    let text = r#"{"patterns":[{"name":"p","tracks":[{"name":"BASS","notes":[{"beat":0,"step":0}]}]}]}"#;
    let (library, _) = Library::parse(text).unwrap();
    let mut s = Session::new(library, KitSet::empty(), Vec::new());
    let shown = |s: &Session| {
        let view = s.note_view(0);
        view.arp_direction.values[view.arp_direction.selected].clone()
    };
    let first = shown(&s);
    let step = |s: &mut Session, delta: i32| {
        let view = s.note_view(0);
        let choice = view.arp_direction;
        let count = choice.values.len() as i32;
        let at = usize::try_from((choice.selected as i32 + delta).rem_euclid(count)).unwrap();
        s.select(Id::NoteSelect("arpDirection"), &choice.values[at]);
    };
    step(&mut s, 1);
    let next = shown(&s);
    assert_ne!(next, first);
    assert_eq!(s.pattern().unwrap().tracks[0].notes[0].arp_direction.as_deref(), Some(next.as_str()));
    step(&mut s, -1);
    assert_eq!(shown(&s), first);
    step(&mut s, -1);
    let directions = s.note_view(0).arp_direction.values;
    assert_eq!(shown(&s), directions[directions.len() - 1], "from the first, the previous is the last");
}
