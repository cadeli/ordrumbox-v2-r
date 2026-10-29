//! Saving the kits and the synth presets is idempotent: a file holding every
//! field is saved, read back and saved again; the second save writes the
//! first one's text and every value is the one written. The first save may
//! normalize (the JS app's keys); the saves after it never change the file.

use std::path::Path;

use ordrumbox_core::json::{self, Value};
use ordrumbox_engine::{Kit, KitSample, SynthPresets, kits_from_json, kits_to_json};

fn parse(text: &str) -> Value {
    json::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn asset(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data").join(name);
    parse(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// The file text, as `store::save_kits` writes it.
fn kits_text(kits: &[Kit]) -> String {
    kits_to_json(kits).to_json_pretty(2) + "\n"
}

/// Read `value`, save, read and save again: the same text, the same kits.
/// The kits read back from the first save.
fn kits_saved_twice(value: &Value) -> Vec<Kit> {
    let kits = kits_from_json(value);
    let first = kits_text(&kits);
    let reloaded = kits_from_json(&parse(&first));
    assert_eq!(kits_text(&reloaded), first, "a second save changes the file");
    assert_eq!(reloaded, kits, "the file does not hold the whole kits");
    reloaded
}

#[test]
fn every_kit_field_survives_saving() {
    let kits = kits_saved_twice(&parse(
        r#"[{"name":"studio","info":"a kit","desc":"recorded live","custom":{"x":1},"samples":[
              {"displayName":"kick","key":"KICK","path":"studio/kick.wav","peakDb":-3.5,"rootMidi":36,
               "decay":800,"gainDb":-2.5,"transpose":-1.5,"someFutureKey":[1,2]},
              {"displayName":"snare","key":"SNARE","path":"studio/snare.wav"}]},
            {"name":"empty","samples":[]}]"#,
    ));
    assert_eq!(kits.len(), 2);
    let kit = &kits[0];
    assert_eq!(kit.name, "studio");
    for (key, value) in [("info", r#""a kit""#), ("desc", r#""recorded live""#), ("custom", r#"{"x":1}"#)] {
        assert_eq!(kit.extra.get(key), Some(&parse(value)), "kit {key}");
    }
    let kick = &kit.samples[0];
    let expected = KitSample {
        key: "KICK".into(),
        display_name: "kick".into(),
        path: "studio/kick.wav".into(),
        decay: Some(800.0),
        root_midi: Some(36.0),
        peak_db: Some(-3.5),
        gain_db: -2.5,
        transpose: -1.5,
        extra: kick.extra.clone(),
    };
    assert_eq!(*kick, expected);
    assert_eq!(kick.extra.get("someFutureKey"), Some(&parse("[1,2]")), "an unknown key is kept");
    // Fields left unset stay unset: no `null`, no 0 written for them.
    let snare = &kit.samples[1];
    assert_eq!(
        (snare.decay, snare.root_midi, snare.peak_db, snare.gain_db, snare.transpose),
        (None, None, None, 0.0, 0.0)
    );
}

#[test]
fn the_js_kit_keys_are_saved_under_their_current_name() {
    let web = parse(
        r#"[{"name":"studio","infos":"a kit","instruments":[
              {"display_name":"kick","key":"KICK","url":"studio/kick.wav","tune":-1.5}]}]"#,
    );
    let current = parse(
        r#"[{"name":"studio","info":"a kit","samples":[
              {"displayName":"kick","key":"KICK","path":"studio/kick.wav","transpose":-1.5}]}]"#,
    );
    let from_web = kits_saved_twice(&web);
    assert_eq!(from_web, kits_saved_twice(&current));
    let text = kits_text(&from_web);
    for key in ["infos", "instruments", "display_name", "url", "tune"] {
        assert!(!text.contains(&format!("\"{key}\"")), "{key} written back");
    }
}

#[test]
fn the_shipped_kits_are_saved_as_they_were_read() {
    let kits = kits_saved_twice(&asset("kits.json"));
    assert!(!kits.is_empty());
}

#[test]
fn the_synth_presets_are_saved_as_they_were_read() {
    // Presets are kept as written: every key of every preset, in its order.
    let value = asset("synth_presets.json");
    let presets = SynthPresets::from_json(&value);
    assert!(!presets.is_empty());
    let first = presets.to_json().to_json_pretty(2) + "\n";
    assert_eq!(parse(&first), value, "a preset value changed");
    let reloaded = SynthPresets::from_json(&parse(&first));
    assert_eq!(reloaded, presets);
    assert_eq!(reloaded.to_json().to_json_pretty(2) + "\n", first);
}
