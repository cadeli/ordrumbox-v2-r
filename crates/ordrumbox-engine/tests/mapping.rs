//! What the engine makes of track and preset settings, against its Rust
//! snapshot `mapping_rust.txt`: the strip settings of every track setup of
//! `song.json` and of hand-made tracks, the LFO values of their LFOs over a
//! range of ticks, and the synth patch of every preset of the web version
//! (`generated_sounds.json`) and of busy hand-made ones (every LFO target and
//! wave, FM algorithm, modulation target, bypass). `write_the_snapshot`
//! rewrites it when one of these changes on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Library, lfo};
use ordrumbox_dsp::{StripSettings, SynthPatch};
use ordrumbox_engine::strip_settings::strip_settings;
use ordrumbox_engine::synth_preset::{SynthNote, synth_patch};

/// The tempo of the hand-made tracks.
const EDGE_BPM: f64 = 97.0;

/// A slow and a fast tempo the tracks of `song.json` are set up at, besides
/// the library's: a strip's delay is in beats, its length in frames.
const SLOW_BPM: f64 = 60.0;
const FAST_BPM: f64 = 172.0;

/// Tracks `song.json` does not reach, completed with the track defaults.
const EDGE_TRACKS: [&str; 15] = [
    r#"{"name":"A","filterType":"notch","filterFreq":1200,"filterQ":3}"#,
    r#"{"name":"B","filterType":"bandpass","filterFreq":600,"filterQ":9,"filterFreqLfo":{"freq":1},"filterQLfo":{"freq":1}}"#,
    r#"{"name":"C","saturationType":"hard","saturationAmount":0.6,"sat":true}"#,
    r#"{"name":"D","saturationType":"tape","saturationAmount":1.4,"sat":false}"#,
    r#"{"name":"E","reverbType":"gated","reverbAmount":0.7,"reverbOn":true}"#,
    r#"{"name":"F","reverbType":"spring","reverbAmount":0.5,"reverbOn":false}"#,
    r#"{"name":"G","reverbType":"plate","reverbAmount":0.3}"#,
    r#"{"name":"H","delayType":"pingpong","delayTime":0.75,"delayDepth":0.6,"delayOn":true}"#,
    r#"{"name":"I","delayType":"slap","delayTime":0.25,"delayDepth":0.3}"#,
    r#"{"name":"J","delayType":"none","delayTime":1,"delayDepth":0.9}"#,
    r#"{"name":"K","delayType":"tape","delayTime":2,"delayDepth":0.5,"delayOn":false}"#,
    r#"{"name":"L","velocity":1.7,"pan":-0.4,"mute":true}"#,
    r#"{"name":"M","velocity":0.6,"velocityLfo":{"freq":2},"panLfo":{"freq":1},"pan":0.5}"#,
    r#"{"name":"N","filterType":"weird","reverbType":"cathedral","delayType":"warp","saturationType":"fuzz","saturationAmount":0.2}"#,
    r#"{"name":"O","mute":false,"velocity":0.3}"#,
];

/// LFOs `song.json` does not reach: strings for numbers, every wave, the
/// `waveform` spelling, codes and unknown names, negative values, defaults.
const EDGE_LFOS: [&str; 9] = [
    r#"{"freq":"0.5","min":"-1","max":2,"phase":0.25,"type":"sawtooth"}"#,
    r#"{"freq":3,"min":0,"max":1,"phase":0,"type":"square"}"#,
    r#"{"freq":1.5,"min":-3,"max":3,"phase":0.1,"type":"random"}"#,
    r#"{"freq":1,"min":0,"max":1,"waveform":"triangle"}"#,
    r#"{"freq":1,"min":0,"max":1,"type":"2"}"#,
    r#"{"freq":1,"min":0,"max":1,"type":"bogus"}"#,
    r#"{"freq":-1,"min":0,"max":10,"phase":-0.3,"type":"sine"}"#,
    r#"{"min":0.2}"#,
    "{}",
];

/// The ticks the LFOs are read at: every 7/32 of a beat over 37.5 beats.
fn lfo_ticks() -> impl Iterator<Item = f64> {
    (0..1200).step_by(7).map(|t| f64::from(t) * f64::from(TICK) / 32.0)
}

/// The targets an LFO of the busy patches modulates, one patch each.
const LFO_TARGETS: [&str; 33] = [
    "FLT",
    "VCO1",
    "VCO2",
    "VCO3",
    "masterVolume",
    "vco1.gain",
    "vco1.detune",
    "vco1.octave",
    "vco2.gain",
    "vco2.detune",
    "vco2.octave",
    "vco3.gain",
    "vco3.detune",
    "vco3.octave",
    "filter.freq",
    "filter.filterEnvelopeAmount",
    "filter.Q",
    "noise.mix",
    "subGain",
    "pitchPunch",
    "filter.drive",
    "noise.filterFreq",
    "noise.filterQ",
    "fm.amount",
    "fm.algo",
    "envelope.attack",
    "envelope.decay",
    "envelope.sustain",
    "envelope.release",
    "modEnvelope.attack",
    "modEnvelope.decay",
    "modEnvelope.sustain",
    "modEnvelope.release",
];
const WAVES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];
const MOD_TARGETS: [&str; 5] = ["off", "filter", "pitch", "fm", "shape"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn parse(text: &str) -> Value {
    json::parse(text).expect("parses")
}

fn strip_fields(s: &StripSettings) -> [(&'static str, f32); 20] {
    [
        ("cutoff", s.cutoff),
        ("q", s.q),
        ("filterMode", s.filter_mode),
        ("satType", s.saturation_type),
        ("satDrive", s.saturation_drive),
        ("satOut", s.saturation_output),
        ("satMix", s.saturation_mix),
        ("revRoom", s.reverb_room),
        ("revDamp", s.reverb_damping),
        ("revWidth", s.reverb_width),
        ("revOn", s.reverb_on),
        ("revMix", s.reverb_mix),
        ("dlyTimeL", s.delay_time_l),
        ("dlyTimeR", s.delay_time_r),
        ("dlyFb", s.delay_feedback),
        ("dlyOn", s.delay_on),
        ("dlyMix", s.delay_mix),
        ("dlyMode", s.delay_mode),
        ("volume", s.volume),
        ("pan", s.pan),
    ]
}

/// One line a strip setup: where it was first met, its tempo, and the
/// strip's parameters; every hand-made track, and each distinct setup of
/// `song.json` at the library's tempo, [`SLOW_BPM`] and [`FAST_BPM`].
fn strip_lines(library: &Library, lines: &mut Vec<String>) {
    let mut cases = Vec::new();
    for pattern in &library.patterns {
        let name = pattern.name.as_deref().unwrap_or("?");
        for track in &pattern.tracks {
            for bpm in [library.bpm, SLOW_BPM, FAST_BPM] {
                cases.push((format!("{name}/{}", track.name), track.clone(), bpm, false));
            }
        }
    }
    for text in EDGE_TRACKS {
        let (edge, report) = Library::parse(&format!(r#"{{"patterns":[{{"tracks":[{text}]}}]}}"#)).unwrap();
        assert!(report.issues.is_empty(), "{text}: {:#?}", report.issues);
        let track = edge.patterns[0].tracks[0].clone();
        cases.push((format!("edge {}", track.name), track, EDGE_BPM, true));
    }
    let mut seen = HashSet::new();
    for (at, track, bpm, edge) in cases {
        let mut settings = format!("@{bpm}:");
        for (key, value) in strip_fields(&strip_settings(&track, bpm)) {
            write!(settings, " {key}={value}").unwrap();
        }
        if seen.insert(settings.clone()) || edge {
            lines.push(format!("strip {at} {settings}"));
        }
    }
}

/// One line a distinct LFO: its JSON and its values at `lfo_ticks()`.
fn lfo_lines(library: &Library, lines: &mut Vec<String>) {
    let mut lfos: Vec<Object> = Vec::new();
    for track in library.patterns.iter().flat_map(|p| &p.tracks) {
        for lfo in
            [&track.volume_lfo, &track.pan_lfo, &track.transpose_lfo, &track.filter_cutoff_lfo, &track.filter_q_lfo]
                .into_iter()
                .flatten()
        {
            if !lfos.contains(lfo) {
                lfos.push(lfo.clone());
            }
        }
    }
    lfos.extend(EDGE_LFOS.iter().map(|text| parse(text).as_object().cloned().unwrap()));
    for lfo_object in &lfos {
        let mut line = format!("lfo {}:", Value::Object(lfo_object.clone()).to_json());
        for tick in lfo_ticks() {
            write!(line, " {}", lfo::value(lfo_object, tick)).unwrap();
        }
        lines.push(line);
    }
}

/// A patch's fields (booleans as 0/1).
fn patch_fields(p: &SynthPatch) -> Vec<(String, f64)> {
    let mut fields = Vec::new();
    let mut add = |key: &str, v: f64| fields.push((key.to_owned(), v));
    for i in 0..3 {
        add(&format!("osc{}Freq", i + 1), p.osc_freq[i]);
        add(&format!("osc{}Gain", i + 1), p.osc_gain[i]);
        add(&format!("osc{}Detune", i + 1), p.osc_detune[i]);
        add(&format!("osc{}Wave", i + 1), p.osc_wave[i]);
    }
    add("noiseMix", p.noise_mix);
    add("noiseFilterType", p.noise_filter_type);
    add("noiseFilterFreq", p.noise_filter_freq);
    add("noiseFilterQ", p.noise_filter_q);
    add("filterType", p.filter_type);
    add("filterFreq", p.filter_freq);
    add("filterQ", p.filter_q);
    add("drive", p.drive);
    add("pitchPunch", p.pitch_punch);
    add("subGain", p.sub_gain);
    add("attack", p.envelope.attack);
    add("decay", p.envelope.decay);
    add("sustain", p.envelope.sustain);
    add("release", p.envelope.release);
    add("master", p.volume);
    add("pan", p.pan);
    add("velocity", p.velocity);
    for (i, l) in [(1, p.lfo1), (2, p.lfo2)] {
        add(&format!("lfo{i}Target"), l.target);
        add(&format!("lfo{i}Wave"), l.wave);
        add(&format!("lfo{i}Freq"), l.freq);
        add(&format!("lfo{i}Depth"), l.depth);
    }
    add("filterEnvAmt", p.filter_env_amount);
    add("fmAmount", p.fm_amount);
    add("fmAlgo", p.fm_algorithm);
    add("modEnvAttack", p.mod_envelope.attack);
    add("modEnvDecay", p.mod_envelope.decay);
    add("modEnvSustain", p.mod_envelope.sustain);
    add("modEnvRelease", p.mod_envelope.release);
    add("modEnvTarget", p.mod_env_target);
    add("modEnvDepth", p.mod_env_depth);
    let b = p.bypass;
    for (key, on) in [
        ("bypassNoise", b.noise),
        ("bypassFilter", b.filter),
        ("bypassEnv", b.env),
        ("bypassLfo1", b.lfo1),
        ("bypassLfo2", b.lfo2),
        ("bypassFm", b.fm),
        ("bypassModEnv", b.mod_env),
        ("bypassFilterEnv", b.filter_env),
    ] {
        add(key, f64::from(u8::from(on)));
    }
    fields
}

/// The web version's presets (`generated_sounds.json`), in file order.
fn web_presets() -> Object {
    let text = fs::read_to_string(fixtures().join("generated_sounds.json")).expect("generated_sounds.json");
    parse(&text).as_object().cloned().expect("an object")
}

/// `BASS1` with a third oscillator, noise, and the LFOs, FM, modulation
/// target and other keys given: every path of the patch at once.
fn busy(bass1: &Value, lfo: Value, lfo2: Value, fm: Value, mod_target: &str, extra: &str) -> Value {
    let mut preset = bass1.as_object().cloned().expect("an object");
    preset.insert("vco3", parse(r#"{"gain":0.3,"octave":1,"detune":-7,"wave":"triangle"}"#));
    preset.insert("noise", parse(r#"{"mix":0.2,"filterType":"bandpass","filterFreq":3000,"filterQ":2}"#));
    preset.insert("lfo", lfo);
    preset.insert("lfo2", lfo2);
    preset.insert("fm", fm);
    let mod_envelope =
        format!(r#"{{"attack":0.01,"decay":0.05,"sustain":0.3,"release":0.05,"target":"{mod_target}"}}"#);
    preset.insert("modEnvelope", parse(&mod_envelope));
    for (key, value) in parse(extra).as_object().expect("an object").iter() {
        preset.insert(key, value.clone());
    }
    Value::Object(preset)
}

/// The synth cases: name, preset, note.
fn synth_cases() -> Vec<(String, Value, SynthNote)> {
    let presets = web_presets();
    let preset = |key: &str| presets.get(key).cloned().unwrap_or_else(|| panic!("{key}"));
    let note = |ratio, velocity, pan, bpm| SynthNote { ratio, velocity, pan, bpm };
    let mut cases: Vec<_> =
        presets.iter().map(|(key, p)| (key.to_owned(), p.clone(), note(1.0, 0.8, 0.0, 240.0))).collect();
    cases.push(("PIANO_variant".into(), preset("PIANO"), note(1.5, 0.4, 0.5, 180.0)));
    cases.push(("BASS1_low".into(), preset("BASS1"), note(0.5, 1.0, -0.7, 300.0)));
    let bass1 = preset("BASS1");
    let busy_note = note(1.25, 0.9, 0.2, 200.0);
    for (i, target) in LFO_TARGETS.iter().enumerate() {
        let lfo =
            format!(r#"{{"target":"{target}","wave":"{}","freq":{},"depth":0.3,"sync":"off"}}"#, WAVES[i % 5], 3 + i);
        let sync = if i % 3 == 0 { "1/16" } else { "off" };
        let lfo2 = format!(
            r#"{{"target":"{}","wave":"{}","freq":0,"depth":0.5,"sync":"{sync}"}}"#,
            LFO_TARGETS[(i + 7) % LFO_TARGETS.len()],
            WAVES[(i + 2) % 5]
        );
        let fm = format!(r#"{{"amount":{},"algo":{}}}"#, (i % 4) as f64 * 0.1, i % 5);
        let preset = busy(&bass1, parse(&lfo), parse(&lfo2), parse(&fm), MOD_TARGETS[i % 5], "{}");
        cases.push((format!("busy_{i}"), preset, busy_note));
    }
    let bypassed = busy(
        &bass1,
        parse(r#"{"target":"FLT","wave":"sine","freq":2,"depth":0.5}"#),
        Value::Null,
        parse(r#"{"amount":0.3,"algo":1}"#),
        "pitch",
        r#"{"bypassNoise":true,"bypassFilter":true,"bypassLfo1":true,"bypassFm":true,"bypassModEnv":true,"bypassFilterEnv":true}"#,
    );
    cases.push(("bypassed".into(), bypassed, busy_note));
    let no_env = busy(
        &bass1,
        Value::Null,
        Value::Null,
        Value::Null,
        "off",
        r#"{"bypassEnv":true,"filter":{"type":"notch","freq":9000,"Q":0.5}}"#,
    );
    cases.push(("no_env".into(), no_env, busy_note));
    cases
}

/// One line a synth case: its name, note and patch.
fn synth_lines(lines: &mut Vec<String>) {
    for (name, preset, note) in synth_cases() {
        let mut line =
            format!("synth {name} ratio={} velocity={} pan={} bpm={}:", note.ratio, note.velocity, note.pan, note.bpm);
        for (key, value) in patch_fields(&synth_patch(&preset, note)) {
            write!(line, " {key}={value}").unwrap();
        }
        lines.push(line);
    }
}

fn rust_snapshot() -> Vec<String> {
    let text =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../ordrumbox-core/tests/fixtures/song.json"))
            .expect("song.json");
    let (library, _) = Library::from_json(&parse(&text));
    let mut lines = Vec::new();
    strip_lines(&library, &mut lines);
    lfo_lines(&library, &mut lines);
    synth_lines(&mut lines);
    lines
}

fn snapshot_path() -> PathBuf {
    fixtures().join("mapping_rust.txt")
}

/// Writes `mapping_rust.txt` from the code as it is: `cargo test -p
/// ordrumbox-engine --test mapping -- --ignored`, when the strip settings,
/// the LFOs or the synth patches change on purpose (`DEVELOPMENT.md`,
/// "Snapshots").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn mapping_matches_its_snapshot() {
    let text = fs::read_to_string(snapshot_path()).expect("tests/fixtures/mapping_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}

#[test]
fn the_snapshot_covers_every_case() {
    let lines = rust_snapshot();
    let count = |prefix: &str| lines.iter().filter(|l| l.starts_with(prefix)).count();
    assert!(count("strip ") > 100, "{}", count("strip "));
    assert_eq!(count("strip edge "), EDGE_TRACKS.len());
    assert!(count("lfo ") > 30, "{}", count("lfo "));
    assert_eq!(count("synth "), 16 + 2 + LFO_TARGETS.len() + 2);
}
