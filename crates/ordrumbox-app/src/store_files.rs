//! The user files the store writes are idempotent, setting by setting: a
//! synth preset with every control of the synth editor set, the master and
//! update settings with every setting set, and the shipped library, kits and
//! presets are written, read back and written again; the second write is
//! the first one's bytes and every value is the one written.

use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::Library;
use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_engine::{MasterSettings, SynthPresets, kits_from_json};

use crate::master;
use crate::store;
use crate::synth_model as model;
use crate::update;

/// A folder of its own for test `name`, empty.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordrumbox-files-{name}-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    dir
}

fn asset(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data").join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The text settings a synth preset takes, beside its numeric controls.
const PRESET_TEXTS: [(&str, &str); 11] = [
    ("vco1.wave", "square"),
    ("vco2.wave", "triangle"),
    ("vco3.wave", "random"),
    ("filter.type", "bandpass"),
    ("noise.filterType", "lowpass"),
    ("lfo.target", "filter.freq"),
    ("lfo.wave", "sawtooth"),
    ("lfo.sync", "1/8T"),
    ("lfo2.target", "vco1.detune"),
    ("lfo2.wave", "square"),
    ("modEnvelope.target", "pitch"),
];

/// A preset with every control of the synth editor away from its default:
/// each knob at its maximum (its minimum when the default is the maximum),
/// each choice on another option, each card bypassed. The values set.
fn full_preset() -> (Object, Vec<(String, Value)>) {
    let defaults = model::defaults();
    let mut draft = defaults.clone();
    let mut values = Vec::new();
    for meta in model::metas() {
        let default = model::number(&defaults, meta.path).unwrap_or_else(|| panic!("{}: no default", meta.path));
        let value = if default == meta.max { meta.min } else { meta.max };
        values.push((meta.path.to_owned(), Value::from(value)));
    }
    for (path, text) in PRESET_TEXTS {
        assert_ne!(model::get(&defaults, path), Some(&Value::from(text)), "{path}: the test value is the default");
        values.push((path.to_owned(), Value::from(text)));
    }
    for (_, flag) in model::BYPASS_FLAGS {
        values.push((flag.to_owned(), Value::Bool(true)));
    }
    for (path, value) in &values {
        assert!(model::set(&mut draft, path, value.clone()), "{path}: no such group");
    }
    (draft, values)
}

#[test]
fn every_synth_control_survives_saving() {
    let dir = folder("synths");
    let path = dir.join("synth_presets.json");
    let (draft, values) = full_preset();
    let mut presets = SynthPresets::from_json(&json::parse(&asset("synth_presets.json")).unwrap());
    presets.set("FULL", Value::Object(draft.clone()));

    store::save_synths(&path, &presets).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    let loaded = store::load_synths(&path).unwrap().expect("saved");
    assert_eq!(loaded, presets);
    store::save_synths(&path, &loaded).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), first, "a second save changes the file");

    let Some(Value::Object(preset)) = loaded.get("FULL") else { panic!("FULL is not an object") };
    for (path, value) in &values {
        assert_eq!(model::get(preset, path), Some(value), "{path}");
    }
    // Opening it in the editor adds nothing: the preset holds every group.
    let mut hydrated = preset.clone();
    model::hydrate(&mut hydrated);
    assert_eq!(hydrated, *preset);
    fs::remove_dir_all(dir).unwrap();
}

/// The master settings with every setting away from its default, inside its range.
fn full_master() -> MasterSettings {
    let defaults = MasterSettings::default();
    let mut settings = MasterSettings { bypass: 1.0, ..defaults };
    for key in master::KEYS {
        let default = master::get(&defaults, key);
        let up = master::get(&master::set(defaults, key, default + 1.0).clamped(), key);
        let value =
            if up != default { up } else { master::get(&master::set(defaults, key, default - 1.0).clamped(), key) };
        assert_ne!(value, default, "{key}");
        settings = master::set(settings, key, value);
    }
    settings
}

#[test]
fn every_setting_survives_saving() {
    let dir = folder("settings");
    let path = dir.join("settings.json");
    // A key of another version of the app, kept.
    fs::create_dir_all(&dir).unwrap();
    fs::write(&path, r#"{"colorScheme":3}"#).unwrap();
    let master = full_master();
    let updates = update::Settings { check: false, last_check: Some(1_761_000_000) };

    let save = |master: &MasterSettings, updates: update::Settings| {
        store::save_setting(&path, "master", master::to_json(master)).unwrap();
        store::save_setting(&path, "updates", updates.to_json()).unwrap();
    };
    let load = || {
        let (master, issues) = master::from_json(&store::load_setting(&path, "master").unwrap().expect("master"));
        assert!(issues.is_empty(), "{issues:?}");
        let (updates, issues) =
            update::Settings::from_json(&store::load_setting(&path, "updates").unwrap().expect("updates"));
        assert!(issues.is_empty(), "{issues:?}");
        (master, updates)
    };
    save(&master, updates);
    let first = fs::read_to_string(&path).unwrap();
    let (read_master, read_updates) = load();
    assert_eq!((read_master, read_updates), (master, updates));
    for key in master::KEYS {
        assert_eq!(master::get(&read_master, key), master::get(&master, key), "{key}");
    }
    save(&read_master, read_updates);
    assert_eq!(fs::read_to_string(&path).unwrap(), first, "a second save changes the file");
    assert_eq!(store::load_setting(&path, "colorScheme"), Ok(Some(Value::from(3))));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_shipped_files_are_written_again_unchanged() {
    let dir = folder("shipped");

    let path = dir.join("library.json");
    let (library, _) = Library::parse(&asset("library.json")).unwrap();
    store::save(&path, &library).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    store::save(&path, &store::load(&path).unwrap().expect("saved")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), first, "library.json");

    let path = dir.join("kits.json");
    store::save_kits(&path, &kits_from_json(&json::parse(&asset("kits.json")).unwrap())).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    store::save_kits(&path, &store::load_kits(&path).unwrap().expect("saved")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), first, "kits.json");

    let path = dir.join("synth_presets.json");
    store::save_synths(&path, &SynthPresets::from_json(&json::parse(&asset("synth_presets.json")).unwrap())).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    store::save_synths(&path, &store::load_synths(&path).unwrap().expect("saved")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), first, "synth_presets.json");
    // The shipped presets are written as they are shipped.
    assert_eq!(json::parse(&first).unwrap(), json::parse(&asset("synth_presets.json")).unwrap());

    fs::remove_dir_all(dir).unwrap();
}
