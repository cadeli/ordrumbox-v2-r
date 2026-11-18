//! The Sound tab's synth cartridge: its LED, its screen and its arrows.

use super::super::Session;
use crate::kits::KitSet;
use crate::panel::{Id, Step};
use ordrumbox_core::Library;
use ordrumbox_core::json;
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
