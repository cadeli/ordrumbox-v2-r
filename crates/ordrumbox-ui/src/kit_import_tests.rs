//! End to end: a folder of samples imported as a kit, then the sample each
//! canonical track gets in it. The packs are file names as sample packs and
//! drum machines name them; the expectations are written by hand
//! (`README.md`, "Golden fixtures").

use std::fs;
use std::path::Path;

use ordrumbox_core::Track;
use ordrumbox_core::json;
use ordrumbox_engine::assign::{TrackSound, assign_track, substitute_for};
use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets, wav};

use crate::kits::KitSet;

/// Packs: folder name and file names.
const PACKS: [(&str, &[&str]); 5] = [
    (
        "roland",
        &[
            "BD.wav", "SD.wav", "CH.wav", "OH.wav", "CP.wav", "RS.wav", "CB.wav", "CY.wav", "LT.wav", "MT.wav",
            "HT.wav", "LC.wav", "MC.wav", "HC.wav", "MA.wav", "CL.wav",
        ],
    ),
    (
        "abbr",
        &[
            "kck_01.wav",
            "sn01.wav",
            "snr_03.wav",
            "hhc.wav",
            "hho.wav",
            "clp_01.wav",
            "rim01.wav",
            "crsh.wav",
            "rd01.wav",
            "ltom.wav",
            "htom.wav",
            "tom2.wav",
            "shkr.wav",
            "cowb.wav",
            "tamb.wav",
            "bass_c2.wav",
        ],
    ),
    (
        "ni",
        &[
            "Kick Basic 1.wav",
            "Snare Acoustic.wav",
            "Snare Rim.wav",
            "HH Closed 1.wav",
            "HH Open 2.wav",
            "HH Pedal.wav",
            "Clap Big.wav",
            "Shaker Hi.wav",
            "Tom High.wav",
            "Tom Low.wav",
            "Tom Mid.wav",
            "Crash Left.wav",
            "Ride Edge.wav",
            "Cowbell Hi.wav",
            "Claves.wav",
            "Castanets.wav",
        ],
    ),
    (
        "splice",
        &[
            "Kick_Punchy_02.wav",
            "Snare_Trap_05.wav",
            "Hat_Closed_03.wav",
            "OpenHat_02.wav",
            "Clap_Layered.wav",
            "Rim_Shot.wav",
            "Crash_01.wav",
            "Ride_Bell.wav",
            "Splash_01.wav",
            "Shaker_16th.wav",
            "Tamb_Loop.wav",
            "Conga_Hi.wav",
            "Tom_Floor.wav",
            "Tom_Rack_01.wav",
            "808_Bass_C.wav",
            "Bassline_Loop_124.wav",
            "Lead_Saw_C.wav",
            "Pad_Warm_C.wav",
        ],
    ),
    (
        "ableton",
        &[
            "Kick 808 Basic.wav",
            "Snare Brush.wav",
            "Hihat Closed 808.wav",
            "Hihat Open 909.wav",
            "Clap 808.wav",
            "Tom Lo 808.wav",
            "Tom Hi 808.wav",
            "Cymbal Crash 909.wav",
            "Cymbal Ride 909.wav",
            "Shaker Lofi.wav",
            "Perc Bongo.wav",
            "Rimshot 808.wav",
            "Cowbell 808.wav",
            "Tambourine Hit.wav",
        ],
    ),
];

/// The tracks every pack is played with.
const TRACKS: [&str; 18] = [
    "KICK", "SNARE", "CLAP", "CHH", "OHH", "TOM", "HI_TOM", "LO_TOM", "CRASH", "RIDE", "RIMSHOT", "COWBELL", "SHAKER",
    "TAMB", "CONGAS", "CLAVES", "BASS", "SYNTHPAD",
];

/// What each track plays in each pack: the file of the first candidate and of the
/// last one ("-" when the track gets nothing), and whether it stands in for the
/// track's instrument.
const EXPECTED: &[(&str, &str, &str, &str, bool)] = &[
    ("roland", "KICK", "BD.wav", "BD.wav", false),
    ("roland", "SNARE", "SD.wav", "SD.wav", false),
    ("roland", "CLAP", "CP.wav", "CP.wav", false),
    ("roland", "CHH", "CH.wav", "CH.wav", false),
    ("roland", "OHH", "OH.wav", "OH.wav", false),
    ("roland", "TOM", "MT.wav", "MT.wav", false),
    ("roland", "HI_TOM", "HT.wav", "HT.wav", false),
    ("roland", "LO_TOM", "LT.wav", "LT.wav", false),
    ("roland", "CRASH", "CY.wav", "CY.wav", false),
    ("roland", "RIDE", "CY.wav", "CY.wav", true),
    ("roland", "RIMSHOT", "RS.wav", "RS.wav", false),
    ("roland", "COWBELL", "CB.wav", "CB.wav", false),
    ("roland", "SHAKER", "MA.wav", "MA.wav", true),
    ("roland", "TAMB", "MA.wav", "MA.wav", true),
    ("roland", "CONGAS", "MC.wav", "MC.wav", false),
    ("roland", "CLAVES", "CL.wav", "CL.wav", false),
    ("roland", "BASS", "synth BASS_ANALOG", "synth BASS_FM", false),
    ("roland", "SYNTHPAD", "synth SYNTHPAD_WARM", "synth SYNTHPAD_SWEEP", false),
    ("abbr", "KICK", "kck_01.wav", "kck_01.wav", false),
    ("abbr", "SNARE", "sn01.wav", "snr_03.wav", false),
    ("abbr", "CLAP", "clp_01.wav", "clp_01.wav", false),
    ("abbr", "CHH", "hhc.wav", "hhc.wav", false),
    ("abbr", "OHH", "hho.wav", "hho.wav", false),
    ("abbr", "TOM", "tom2.wav", "tom2.wav", false),
    ("abbr", "HI_TOM", "htom.wav", "htom.wav", false),
    ("abbr", "LO_TOM", "ltom.wav", "ltom.wav", false),
    ("abbr", "CRASH", "crsh.wav", "crsh.wav", false),
    ("abbr", "RIDE", "rd01.wav", "rd01.wav", false),
    ("abbr", "RIMSHOT", "rim01.wav", "rim01.wav", false),
    ("abbr", "COWBELL", "cowb.wav", "cowb.wav", false),
    ("abbr", "SHAKER", "shkr.wav", "shkr.wav", false),
    ("abbr", "TAMB", "tamb.wav", "tamb.wav", false),
    ("abbr", "CONGAS", "htom.wav", "htom.wav", true),
    ("abbr", "CLAVES", "rim01.wav", "rim01.wav", true),
    ("abbr", "BASS", "bass_c2.wav", "bass_c2.wav", false),
    ("abbr", "SYNTHPAD", "synth SYNTHPAD_WARM", "synth SYNTHPAD_SWEEP", false),
    ("ni", "KICK", "Kick Basic 1.wav", "Kick Basic 1.wav", false),
    ("ni", "SNARE", "Snare Acoustic.wav", "Snare Acoustic.wav", false),
    ("ni", "CLAP", "Clap Big.wav", "Clap Big.wav", false),
    ("ni", "CHH", "HH Closed 1.wav", "HH Closed 1.wav", false),
    ("ni", "OHH", "HH Open 2.wav", "HH Open 2.wav", false),
    ("ni", "TOM", "Tom Mid.wav", "Tom Mid.wav", false),
    ("ni", "HI_TOM", "Tom High.wav", "Tom High.wav", false),
    ("ni", "LO_TOM", "Tom Low.wav", "Tom Low.wav", false),
    ("ni", "CRASH", "Crash Left.wav", "Crash Left.wav", false),
    ("ni", "RIDE", "Ride Edge.wav", "Ride Edge.wav", false),
    ("ni", "RIMSHOT", "Snare Rim.wav", "Snare Rim.wav", false),
    ("ni", "COWBELL", "Cowbell Hi.wav", "Cowbell Hi.wav", false),
    ("ni", "SHAKER", "Shaker Hi.wav", "Shaker Hi.wav", false),
    ("ni", "TAMB", "Shaker Hi.wav", "Shaker Hi.wav", true),
    ("ni", "CONGAS", "Tom High.wav", "Tom High.wav", true),
    ("ni", "CLAVES", "Claves.wav", "Claves.wav", false),
    ("ni", "BASS", "synth BASS_ANALOG", "synth BASS_FM", false),
    ("ni", "SYNTHPAD", "synth SYNTHPAD_WARM", "synth SYNTHPAD_SWEEP", false),
    ("splice", "KICK", "Kick_Punchy_02.wav", "Kick_Punchy_02.wav", false),
    ("splice", "SNARE", "Snare_Trap_05.wav", "Snare_Trap_05.wav", false),
    ("splice", "CLAP", "Clap_Layered.wav", "Clap_Layered.wav", false),
    ("splice", "CHH", "Hat_Closed_03.wav", "Hat_Closed_03.wav", false),
    ("splice", "OHH", "OpenHat_02.wav", "OpenHat_02.wav", false),
    ("splice", "TOM", "Tom_Rack_01.wav", "Tom_Rack_01.wav", false),
    ("splice", "HI_TOM", "Tom_Rack_01.wav", "Tom_Rack_01.wav", false),
    ("splice", "LO_TOM", "Tom_Floor.wav", "Tom_Floor.wav", false),
    ("splice", "CRASH", "Crash_01.wav", "Splash_01.wav", false),
    ("splice", "RIDE", "Ride_Bell.wav", "Ride_Bell.wav", false),
    ("splice", "RIMSHOT", "Rim_Shot.wav", "Rim_Shot.wav", false),
    ("splice", "COWBELL", "Ride_Bell.wav", "Ride_Bell.wav", true),
    ("splice", "SHAKER", "Shaker_16th.wav", "Shaker_16th.wav", false),
    ("splice", "TAMB", "Tamb_Loop.wav", "Tamb_Loop.wav", false),
    ("splice", "CONGAS", "Conga_Hi.wav", "Conga_Hi.wav", false),
    ("splice", "CLAVES", "Rim_Shot.wav", "Rim_Shot.wav", true),
    ("splice", "BASS", "808_Bass_C.wav", "Bassline_Loop_124.wav", false),
    ("splice", "SYNTHPAD", "Pad_Warm_C.wav", "Pad_Warm_C.wav", false),
    ("ableton", "KICK", "Kick 808 Basic.wav", "Kick 808 Basic.wav", false),
    ("ableton", "SNARE", "Snare Brush.wav", "Snare Brush.wav", false),
    ("ableton", "CLAP", "Clap 808.wav", "Clap 808.wav", false),
    ("ableton", "CHH", "Hihat Closed 808.wav", "Hihat Closed 808.wav", false),
    ("ableton", "OHH", "Hihat Open 909.wav", "Hihat Open 909.wav", false),
    ("ableton", "TOM", "Tom Hi 808.wav", "Tom Hi 808.wav", false),
    ("ableton", "HI_TOM", "Tom Hi 808.wav", "Tom Hi 808.wav", false),
    ("ableton", "LO_TOM", "Tom Lo 808.wav", "Tom Lo 808.wav", false),
    ("ableton", "CRASH", "Cymbal Crash 909.wav", "Cymbal Crash 909.wav", false),
    ("ableton", "RIDE", "Cymbal Ride 909.wav", "Cymbal Ride 909.wav", false),
    ("ableton", "RIMSHOT", "Rimshot 808.wav", "Rimshot 808.wav", false),
    ("ableton", "COWBELL", "Cowbell 808.wav", "Cowbell 808.wav", false),
    ("ableton", "SHAKER", "Shaker Lofi.wav", "Shaker Lofi.wav", false),
    ("ableton", "TAMB", "Shaker Lofi.wav", "Shaker Lofi.wav", true),
    ("ableton", "CONGAS", "Perc Bongo.wav", "Perc Bongo.wav", true),
    ("ableton", "CLAVES", "Rimshot 808.wav", "Rimshot 808.wav", true),
    ("ableton", "BASS", "synth BASS_ANALOG", "synth BASS_FM", false),
    ("ableton", "SYNTHPAD", "synth SYNTHPAD_WARM", "synth SYNTHPAD_SWEEP", false),
];

/// The synth presets the app starts with.
fn presets() -> SynthPresets {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data/synth_presets.json");
    SynthPresets::from_json(&json::parse(&fs::read_to_string(path).unwrap()).unwrap())
}

fn picked(bank: &SampleBank, synths: &SynthPresets, kit: &str, track: &str, pick: usize) -> (String, bool) {
    let track = Track { name: track.to_owned(), ..Track::default() };
    let mut choose = |n: usize| pick.min(n - 1);
    match assign_track(&track, bank, kit, synths, &mut choose) {
        TrackSound::Sample(i) => {
            let sample = &bank.samples[i].sample;
            let file = sample.path.rsplit('/').next().unwrap_or_default().to_owned();
            (file, substitute_for(&track.name, &sample.key).is_some())
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
fn an_imported_pack_plays_each_track_with_its_sample() {
    let dir = std::env::temp_dir().join(format!("ordrumbox-pack-import-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    let synths = presets();
    let mut got = Vec::new();
    for (pack, files) in PACKS {
        let (bank, errors) = import(&dir, pack, files).bank();
        assert!(errors.is_empty(), "{errors:?}");
        for track in TRACKS {
            let (first, stands_in) = picked(&bank, &synths, pack, track, 0);
            let (last, _) = picked(&bank, &synths, pack, track, usize::MAX);
            got.push((pack.to_owned(), track, first, last, stands_in));
        }
    }
    fs::remove_dir_all(dir).unwrap();
    assert_eq!(got.len(), EXPECTED.len());
    for (got, &(pack, track, first, last, stands_in)) in got.iter().zip(EXPECTED) {
        assert_eq!((got.0.as_str(), got.1), (pack, track), "the table follows PACKS and TRACKS");
        assert_eq!((got.2.as_str(), got.3.as_str(), got.4), (first, last, stands_in), "{track} in {pack}");
    }
}
