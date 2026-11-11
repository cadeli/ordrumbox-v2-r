//! Catalog lookups and the clips a song plays at a tick, as explicit tables.
//!
//! A row that changes is a change of behaviour (`DEVELOPMENT.md`, "Snapshots").

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::collections::HashSet;
use std::path::Path;

use ordrumbox_core::instruments::{Instrument, catalog};
use ordrumbox_core::json;
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Library, Song, song};

fn id(found: Option<&Instrument>) -> Option<&str> {
    found.map(|i| i.id.as_str())
}

/// A name, then what `by_id`, `by_name` and `from_file_name` find for it.
type Lookup = (&'static str, Option<&'static str>, Option<&'static str>, Option<&'static str>);

#[rustfmt::skip]
const LOOKUPS: &[Lookup] = &[
    ("KICK_01", None, Some("KICK"), Some("KICK")),
    ("SNARE", Some("SNARE"), Some("SNARE"), Some("SNARE")),
    ("RIM", None, Some("RIMSHOT"), Some("RIMSHOT")),
    ("CLAP", Some("CLAP"), Some("CLAP"), Some("CLAP")),
    ("COWBELL", Some("COWBELL"), Some("COWBELL"), Some("COWBELL")),
    ("CHH", Some("CHH"), Some("CHH"), Some("CHH")),
    ("OHH", Some("OHH"), Some("OHH"), Some("OHH")),
    ("CRASH", Some("CRASH"), Some("CRASH"), Some("CRASH")),
    ("KICK", Some("KICK"), Some("KICK"), Some("KICK")),
    ("TOM", Some("TOM"), Some("TOM"), Some("TOM")),
    ("CONGAS", Some("CONGAS"), Some("CONGAS"), Some("CONGAS")),
    ("LO_CONGAS", Some("LO_CONGAS"), Some("CONGAS"), Some("LO_CONGAS")),
    ("TAMBOURINE", Some("TAMBOURINE"), Some("TAMBOURINE"), Some("TAMBOURINE")),
    ("BASS", Some("BASS"), Some("BASS"), Some("BASS")),
    ("HIT", Some("HIT"), None, Some("HIT")),
    ("RIMSHOT", Some("RIMSHOT"), Some("HIT"), Some("RIMSHOT")),
    ("PIANO", Some("PIANO"), Some("PIANO"), Some("PIANO")),
    ("MTOM", Some("MTOM"), Some("MTOM"), Some("MTOM")),
    ("LO_TOM", Some("LO_TOM"), Some("LO_TOM"), Some("LO_TOM")),
    ("GUITAR", Some("GUITAR"), Some("GUITAR"), Some("GUITAR")),
    ("SYNTHLEAD", Some("SYNTHLEAD"), Some("SYNTHLEAD"), Some("SYNTHLEAD")),
    ("BRASS", Some("BRASS"), Some("BRASS"), Some("BRASS")),
    ("STRING", None, Some("STRINGS"), Some("ENSEMBLE")),
    ("ORGAN", Some("ORGAN"), Some("ORGAN"), Some("ORGAN")),
    ("RIDE", Some("RIDE"), Some("RIDE"), Some("RIDE")),
    ("SIDE", None, Some("RIMSHOT"), Some("RIMSHOT")),
    ("CRASH2", None, Some("CRASH"), Some("CRASH")),
    ("HI_TOM", Some("HI_TOM"), Some("HI_TOM"), Some("HI_TOM")),
    ("kick_01", None, Some("KICK"), Some("KICK")),
    ("Kick 2", None, Some("KICK"), Some("KICK")),
    ("BD", None, Some("KICK"), Some("KICK")),
    ("bd808", None, Some("KICK"), Some("KICK")),
    ("HH open", None, Some("CHH"), Some("OHH")),
    // A hat that says nothing of its openness is a closed one.
    ("hat", None, Some("CHH"), Some("CHH")),
    ("open-hat", None, Some("CHH"), Some("OHH")),
    ("Tambourine", Some("TAMBOURINE"), Some("TAMBOURINE"), Some("TAMBOURINE")),
    ("xyz", None, None, None),
    ("", None, None, None),
    ("e guitar", None, Some("GUITAR"), Some("GUITAR")),
    // A snare's rim is a rimshot.
    ("Snare Rim", None, Some("RIMSHOT"), Some("RIMSHOT")),
    // HI and LOW pick the variant, they do not find the instrument.
    ("LOW TOM", None, Some("TOM"), Some("LO_TOM")),
    // HI picks the variant.
    ("tom_hi", None, Some("HI_TOM"), Some("HI_TOM")),
    // The closest GM name: "Low Conga", not "Mute Hi Conga".
    ("conga low", None, Some("CONGAS"), Some("LO_CONGAS")),
    ("Shaker", Some("SHAKER"), Some("SHAKER"), Some("SHAKER")),
    ("piano chords", None, Some("PIANO"), Some("PIANO")),
    ("SUB BASS", None, Some("BASS"), Some("BASS")),
    ("clap_2", None, Some("CLAP"), Some("CLAP")),
    ("Maracas", Some("MARACAS"), Some("MARACAS"), Some("MARACAS")),
    ("woodblock", None, Some("RIMSHOT"), Some("HI_WOODBLOCK")),
    // BELL counts last: a GM "Ride Bell" is no percussion.
    ("COW BELL", None, Some("COWBELL"), Some("COWBELL")),
    ("Ride", Some("RIDE"), Some("RIDE"), Some("RIDE")),
    ("crash cymbal", None, Some("CRASH"), Some("CRASH")),
    ("china", None, Some("CRASH"), Some("CRASH")),
    ("FX riser", None, Some("SOUNDEFFECT"), Some("SOUNDEFFECT")),
    // VOX is read as ENSEMBLE.
    ("vox", None, None, Some("ENSEMBLE")),
    ("Électro kick", None, Some("KICK"), Some("KICK")),
    ("SAX", Some("SAX"), Some("SAX"), Some("SAX")),
    ("mt", None, Some("MTOM"), Some("MTOM")),
];

/// The instruments `by_id` finds that are not drums.
const MELODIC: [&str; 7] = ["BASS", "BRASS", "GUITAR", "ORGAN", "PIANO", "SAX", "SYNTHLEAD"];

#[test]
fn lookups_find_the_listed_instruments() {
    let c = catalog();
    for &(name, by_id, by_name, from_file_name) in LOOKUPS {
        assert_eq!(id(c.by_id(name)), by_id, "by_id({name:?})");
        if let Some(found) = c.by_id(name) {
            assert_eq!(found.drum, !MELODIC.contains(&found.id.as_str()), "{name:?} is a drum");
        }
        assert_eq!(id(c.by_name(name)), by_name, "by_name({name:?})");
        assert_eq!(id(c.from_file_name(name)), from_file_name, "from_file_name({name:?})");
    }
}

/// The GM drum keys (channel 10) the catalog knows; every other key finds nothing.
const DRUM_KEYS: &[(i64, &str)] = &[
    (36, "KICK"),
    (37, "RIMSHOT"),
    (38, "SNARE"),
    (39, "CLAP"),
    (40, "SNARE"),
    (41, "LO_TOM"),
    (42, "CHH"),
    (43, "PERC"),
    (45, "LO_TOM"),
    (46, "OHH"),
    (47, "PERC"),
    (48, "MTOM"),
    (49, "CRASH"),
    (50, "HI_TOM"),
    (51, "RIDE"),
    (52, "PERC"),
    (53, "PERC"),
    (54, "TAMBOURINE"),
    (55, "PERC"),
    (56, "COWBELL"),
    (58, "PERC"),
    (60, "BONGOS"),
    (61, "LO_BONGOS"),
    (62, "CHI_CONGAS"),
    (63, "CONGAS"),
    (64, "LO_CONGAS"),
    (65, "HI_TIMBAL"),
    (66, "LO_TIMBAL"),
    (67, "PERC"),
    (68, "PERC"),
    (69, "CABA"),
    (70, "MARACAS"),
    (71, "WHISTLE"),
    (72, "PERC"),
    (73, "GUIRO"),
    (74, "GUIRO"),
    (75, "CLAVES"),
    (76, "HI_WOODBLOCK"),
    (77, "LO_WOODBLOCK"),
    (78, "CUICA"),
    (79, "CUICA"),
    (80, "TRIANGLE"),
    (81, "TRIANGLE"),
];

/// The instrument of each GM program, eight a family.
const PROGRAMS: [&str; 128] = [
    "PIANO",
    "PIANO",
    "PIANO",
    "PIANO",
    "PIANO",
    "PIANO",
    "PIANO",
    "CLAVES",
    "CROMAPERC",
    "CROMAPERC",
    "CROMAPERC",
    "CROMAPERC",
    "CROMAPERC",
    "CROMAPERC",
    "COWBELL",
    "CROMAPERC",
    "ORGAN",
    "MELO",
    "ORGAN",
    "ORGAN",
    "ORGAN",
    "ORGAN",
    "ORGAN",
    "ORGAN",
    "GUITAR",
    "GUITAR",
    "GUITAR",
    "GUITAR",
    "GUITAR",
    "GUITAR",
    "GUITAR",
    "GUITAR",
    "BASS",
    "BASS",
    "BASS",
    "BASS",
    "BASS",
    "BASS",
    "BASS",
    "BASS",
    "STRINGS",
    "STRINGS",
    "STRINGS",
    "BASS",
    "STRINGS",
    "STRINGS",
    "STRINGS",
    "STRINGS",
    "ENSEMBLE",
    "ENSEMBLE",
    "STRINGS",
    "STRINGS",
    "ENSEMBLE",
    "ENSEMBLE",
    "ENSEMBLE",
    "ENSEMBLE",
    "BRASS",
    "BRASS",
    "BRASS",
    "BRASS",
    "BRASS",
    "LONGBRASS",
    "BRASS",
    "SHORTBRASS",
    "SAX",
    "SAX",
    "SAX",
    "SAX",
    "REED",
    "BRASS",
    "BASS",
    "REED",
    "PIPE",
    "PIPE",
    "PIPE",
    "PIPE",
    "PIPE",
    "PIPE",
    "WHISTLE",
    "PIPE",
    "SYNTHLEAD",
    "SYNTHLEAD",
    "SYNTHLEAD",
    "SYNTHLEAD",
    "SYNTHLEAD",
    "ENSEMBLE",
    "SYNTHLEAD",
    "BASS",
    "SYNTHPAD",
    "SYNTHPAD",
    "SYNTHLEAD",
    "ENSEMBLE",
    "SYNTHPAD",
    "SYNTHPAD",
    "SYNTHPAD",
    "SYNTHPAD",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "ETHNIC",
    "ETHNIC",
    "ETHNIC",
    "ETHNIC",
    "ETHNIC",
    "ETHNIC",
    "ETHNIC",
    "ETHNIC",
    "COWBELL",
    "COWBELL",
    "PERCUSSIVE",
    "RIMSHOT",
    "PERCUSSIVE",
    "MELO",
    "CASTENET",
    "SCRATCH",
    "GUITAR",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "SOUNDEFFECT",
    "HIT",
];

#[test]
fn midi_keys_and_programs_find_the_listed_instruments() {
    let c = catalog();
    for key in 0..128 {
        let want = DRUM_KEYS.iter().find(|(k, _)| *k == key).map(|(_, id)| *id);
        assert_eq!(id(c.from_midi(9, key)), want, "drum key {key}");
        assert_eq!(id(c.from_midi(0, key)), None, "key {key} off the drum channel");
    }
    for (program, want) in (0..).zip(PROGRAMS) {
        assert_eq!(id(c.from_midi_program(program)), Some(want), "program {program}");
    }
}

/// What a clip plays at a tick: pattern, clip index, repetition, tick in the pattern, pattern length.
type Source = (&'static str, usize, u64, u64, u64);

/// A tick of the transport ([`TICK`] a beat, a measure is `4 * TICK` = 384) and the clips playing.
type Expected = (u64, &'static [Source]);

fn demo_library() -> Library {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/song.json");
    Library::parse(&std::fs::read_to_string(path).unwrap()).unwrap().0
}

fn check_sources(library: &Library, song: &Song, expected: &[Expected]) {
    assert_eq!(TICK, 96, "the ticks of these tables are at 96 a beat");
    for &(tick, want) in expected {
        let got: Vec<(&str, usize, u64, u64, u64)> = song
            .active_clips_at(&library.patterns, tick, TICK)
            .iter()
            .map(|s| (library.patterns[s.pattern].id.as_str(), s.clip, s.loop_index, s.local_tick, s.pattern_ticks))
            .collect();
        assert_eq!(got, want, "{} at tick {tick}", song.id);
    }
}

#[test]
fn the_demo_song_plays_its_clips_in_turn() {
    let library = demo_library();
    // Clips: rock (8 beats) on measures 0–1, basrock 2, smrock 3, basrock 4, …,
    // funkfill 38, basrock 64–65; the song loops over 66 measures.
    #[rustfmt::skip]
    let expected: &[Expected] = &[
        (0, &[("rock", 0, 0, 0, 768)]),
        (767, &[("rock", 0, 0, 767, 768)]),
        (768, &[("basrock", 1, 0, 0, 384)]),
        (1151, &[("basrock", 1, 0, 383, 384)]),
        (1152, &[("smrock", 2, 0, 0, 384)]),
        (1535, &[("smrock", 2, 0, 383, 384)]),
        (1536, &[("basrock", 3, 0, 0, 384)]),
        (3840, &[("rock", 6, 0, 0, 768)]),
        (14592, &[("funkfill", 13, 0, 0, 384)]),
        (14975, &[("funkfill", 13, 0, 383, 384)]),
        (24959, &[("basrock", 23, 0, 383, 384)]),
        // Measure 66 wraps to measure 0; the repetitions count on the unwrapped transport.
        (25344, &[("rock", 0, 33, 0, 768)]),
        (26112, &[("basrock", 1, 66, 0, 384)]),
    ];
    check_sources(&library, &library.songs[0], expected);
}

#[test]
fn overlapping_and_fractional_clips() {
    let library = demo_library();
    let known: HashSet<String> = library.patterns.iter().map(|p| p.id.clone()).collect();
    let raw = json::parse(
        r#"[{"id": "hand", "name": "hand", "bpm": 100, "loopMeasureCount": 5, "clips": [
            {"pattern": "rock", "startMeasure": 0, "measureCount": 3},
            {"pattern": "disco", "startMeasure": 1, "measureCount": 1.5},
            {"pattern": "rock", "startMeasure": 3, "measureCount": 2},
            {"pattern": "nowhere", "startMeasure": 4, "measureCount": 1}]}]"#,
    )
    .unwrap();
    let (songs, dropped) = song::songs_from_json(&raw, &known, &mut Vec::new());
    assert_eq!(dropped.len(), 1, "the clip of an unknown pattern is dropped");
    // rock is 8 beats (768 ticks), disco 4 (384).
    #[rustfmt::skip]
    let expected: &[Expected] = &[
        (0, &[("rock", 0, 0, 0, 768)]),
        (383, &[("rock", 0, 0, 383, 768)]),
        (384, &[("rock", 0, 0, 384, 768), ("disco", 1, 0, 0, 384)]),
        (767, &[("rock", 0, 0, 767, 768), ("disco", 1, 0, 383, 384)]),
        (768, &[("rock", 0, 1, 0, 768), ("disco", 1, 1, 0, 384)]),
        (959, &[("rock", 0, 1, 191, 768), ("disco", 1, 1, 191, 384)]),
        // Half a measure later disco's 1.5 measures are over.
        (960, &[("rock", 0, 1, 192, 768)]),
        (1151, &[("rock", 0, 1, 383, 768)]),
        (1152, &[("rock", 2, 0, 0, 768)]),
        (1535, &[("rock", 2, 0, 383, 768)]),
        (1536, &[("rock", 2, 0, 384, 768)]),
        (1919, &[("rock", 2, 0, 767, 768)]),
        // The loop of 5 measures wraps; the repetitions go on counting.
        (1920, &[("rock", 0, 2, 384, 768)]),
        (2304, &[("rock", 0, 3, 0, 768), ("disco", 1, 5, 0, 384)]),
        (3840, &[("rock", 0, 5, 0, 768)]),
    ];
    check_sources(&library, &songs[0], expected);
}
