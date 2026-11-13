//! The sample each canonical track gets in a kit: the kits the app ships with,
//! and folders of samples imported as kits, end to end. The packs are file
//! names as sample packs and drum machines name them; the expectations are
//! written by hand (`DEVELOPMENT.md`, "Instrument catalog").

use std::fs;
use std::path::Path;

use ordrumbox_core::Track;
use ordrumbox_core::json;
use ordrumbox_engine::assign::{TrackSound, assign_track, substitute_for};
use ordrumbox_engine::kit::BankSample;
use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets, kits_from_json, wav};

use crate::kits::KitSet;

#[path = "assign_tests/data.rs"]
mod data;
use data::*;
/// The synth presets the app starts with.
fn presets() -> SynthPresets {
    SynthPresets::from_json(&asset("synth_presets.json")).unwrap()
}

fn asset(name: &str) -> json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data").join(name);
    json::parse(&fs::read_to_string(path).unwrap()).unwrap()
}

/// The kits the app ships with, in one bank: a kit lacking an instrument borrows it from another.
fn shipped() -> (SampleBank, Vec<String>) {
    let kits = kits_from_json(&asset("kits.json")).unwrap().0;
    let samples = kits
        .iter()
        .flat_map(|kit| {
            kit.samples.iter().map(|sample| BankSample {
                kit: kit.name.clone(),
                sample: sample.clone(),
                buffer: SampleBuffer::mono(vec![0.0], 44_100).into(),
            })
        })
        .collect();
    (SampleBank { samples }, kits.iter().map(|k| k.name.clone()).collect())
}

/// The sample `track` gets in `kit`, shown as in [`EXPECTED`], and whether it stands in
/// for the track's instrument.
fn picked(bank: &SampleBank, synths: &SynthPresets, kit: &str, track: &str, pick: usize) -> (String, bool) {
    let track = Track { name: track.to_owned(), ..Track::default() };
    let mut choose = |n: usize| pick.min(n - 1);
    match assign_track(&track, bank, kit, synths, &mut choose) {
        TrackSound::Sample(i) => {
            let sample = &bank.samples[i].sample;
            let own = bank.samples[i].kit == kit;
            let shown = if own { sample.path.rsplit('/').next().unwrap_or_default() } else { &sample.path };
            (shown.to_owned(), substitute_for(&track.name, &sample.key).is_some())
        }
        TrackSound::None => ("-".to_owned(), false),
        TrackSound::Synth(key) => (format!("synth {key}"), false),
    }
}

fn import(dir: &Path, pack: &str, files: &[&str]) -> KitSet {
    let source = dir.join("sources").join(pack);
    fs::create_dir_all(&source).unwrap();
    for file in files {
        fs::write(source.join(file), wav::encode_float(&SampleBuffer::mono(vec![0.25; 4], 8000))).unwrap();
    }
    let mut kits = KitSet::new(Vec::new(), &SampleBank::default(), None, None, Some(dir.join("samples")));
    kits.import_folder(&source).unwrap();
    kits
}

#[test]
fn each_kit_plays_each_track_with_its_sample() {
    let dir = std::env::temp_dir().join(format!("ordrumbox-pack-import-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    let mut banks = Vec::new();
    for (pack, files) in PACKS {
        let (bank, errors) = import(&dir, pack, files).bank();
        assert!(errors.is_empty(), "{errors:?}");
        banks.push((pack, bank));
    }
    fs::remove_dir_all(dir).unwrap();
    let (shipped, kit_names) = shipped();
    assert_eq!(kit_names, SHIPPED);
    let synths = presets();
    // Every row that differs, written as the row would be, so a change shows whole.
    let mut changed = Vec::new();
    for &(kit, track, first, last, stands_in) in EXPECTED {
        let bank = banks.iter().find(|(pack, _)| *pack == kit).map_or(&shipped, |(_, bank)| bank);
        let (got_first, got_stands_in) = picked(bank, &synths, kit, track, 0);
        let (got_last, _) = picked(bank, &synths, kit, track, usize::MAX);
        if (got_first.as_str(), got_last.as_str(), got_stands_in) != (first, last, stands_in) {
            changed.push(format!("({kit:?}, {track:?}, {got_first:?}, {got_last:?}, {got_stands_in}),"));
        }
    }
    assert!(changed.is_empty(), "rows that changed:\n{}", changed.join("\n"));
    for kit in PACKS.iter().map(|(pack, _)| *pack).chain(SHIPPED) {
        for track in TRACKS {
            assert!(EXPECTED.iter().any(|row| (row.0, row.1) == (kit, track)), "no row for {track} in {kit}");
        }
    }
}
