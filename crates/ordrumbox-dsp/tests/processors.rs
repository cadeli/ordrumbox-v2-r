//! The processors against the JS AudioWorklet processors, run in Node by
//! `tools/export_dsp_fixtures.mjs` on the same inputs, quantum by quantum.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::Path;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_dsp::synth::{Adsr, SynthBypass, SynthLfo};
use ordrumbox_dsp::{MasterBus, MasterSettings, Strip, StripSettings, SynthPatch, SynthVoice};

const QUANTUM: usize = 128;
/// Largest difference allowed with the JS output (math functions of V8 and of
/// the platform may differ in their last bit).
const TOLERANCE: f32 = 2e-5;
/// One frame kept out of this many in the Rust snapshots.
const SNAPSHOT_STRIDE: usize = 16;

struct Fixtures {
    full_frames: usize,
    stride: usize,
    cases: Vec<Value>,
    data: Vec<f32>,
}

fn fixtures() -> Fixtures {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let index = json::parse(&std::fs::read_to_string(dir.join("renders.json")).expect("renders.json")).expect("parses");
    let bytes = std::fs::read(dir.join("renders.bin")).expect("renders.bin");
    let data = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    let number = |key: &str| index.get(key).and_then(Value::as_f64).expect(key) as usize;
    Fixtures {
        full_frames: number("fullFrames"),
        stride: number("stride"),
        cases: index.get("cases").and_then(Value::as_array).expect("cases").to_vec(),
        data,
    }
}

fn num(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("{key}"))
}

/// The input of `burst()` in the export script.
fn burst(frames: usize, seed: u32, gain: f64) -> Vec<f32> {
    let mut s = seed;
    let mut env = 0.0_f64;
    (0..frames)
        .map(|i| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if i % 3000 == 0 {
                env = 1.0;
            }
            let noise = f64::from(s >> 8) / 16_777_216.0 * 2.0 - 1.0;
            let mut x = noise * env * 0.6;
            if i % 3000 == 0 {
                x += 0.9;
            }
            env *= 0.9993;
            (x * gain) as f32
        })
        .collect()
}

fn set_strip(settings: &mut StripSettings, key: &str, v: f32) {
    let field = match key {
        "cutoff" => &mut settings.cutoff,
        "q" => &mut settings.q,
        "filterMode" => &mut settings.filter_mode,
        "satType" => &mut settings.saturation_type,
        "satDrive" => &mut settings.saturation_drive,
        "satOut" => &mut settings.saturation_output,
        "satMix" => &mut settings.saturation_mix,
        "revRoom" => &mut settings.reverb_room,
        "revDamp" => &mut settings.reverb_damping,
        "revWidth" => &mut settings.reverb_width,
        "revMix" => &mut settings.reverb_mix,
        "dlyTimeL" => &mut settings.delay_time_l,
        "dlyTimeR" => &mut settings.delay_time_r,
        "dlyFb" => &mut settings.delay_feedback,
        "dlyMix" => &mut settings.delay_mix,
        "dlyMode" => &mut settings.delay_mode,
        "volume" => &mut settings.volume,
        "pan" => &mut settings.pan,
        _ => panic!("strip parameter {key}"),
    };
    *field = v;
}

fn set_master(settings: &mut MasterSettings, key: &str, v: f32) {
    let field = match key {
        "compThreshold" => &mut settings.threshold,
        "compRatio" => &mut settings.ratio,
        "compKnee" => &mut settings.knee,
        "compAttack" => &mut settings.attack,
        "compRelease" => &mut settings.release,
        "compMakeup" => &mut settings.makeup,
        "lowcut" => &mut settings.lowcut,
        "hicut" => &mut settings.hicut,
        "master" => &mut settings.master,
        "bypass" => &mut settings.bypass,
        "preGain" => &mut settings.pre_gain,
        _ => panic!("master parameter {key}"),
    };
    *field = v;
}

/// The `update` message as a patch.
fn patch_from_update(m: &Value) -> SynthPatch {
    let n = |key: &str| num(m, key);
    let b = |key: &str| m.get(key).and_then(Value::as_bool).unwrap_or_else(|| panic!("{key}"));
    let lfo = |i: u8| SynthLfo {
        target: n(&format!("lfo{i}Target")),
        wave: n(&format!("lfo{i}Wave")),
        freq: n(&format!("lfo{i}Freq")),
        depth: n(&format!("lfo{i}Depth")),
    };
    SynthPatch {
        osc_freq: [n("osc1Freq"), n("osc2Freq"), n("osc3Freq")],
        osc_gain: [n("osc1Gain"), n("osc2Gain"), n("osc3Gain")],
        osc_detune: [n("osc1Detune"), n("osc2Detune"), n("osc3Detune")],
        osc_wave: [n("osc1Wave"), n("osc2Wave"), n("osc3Wave")],
        noise_mix: n("noiseMix"),
        noise_filter_type: n("noiseFilterType"),
        noise_filter_freq: n("noiseFilterFreq"),
        noise_filter_q: n("noiseFilterQ"),
        filter_type: n("filterType"),
        filter_freq: n("filterFreq"),
        filter_q: n("filterQ"),
        // The JS voice has one filter stage.
        filter_slope: 12.0,
        drive: n("drive"),
        pitch_punch: n("pitchPunch"),
        sub_gain: n("subGain"),
        envelope: Adsr { attack: n("attack"), decay: n("decay"), sustain: n("sustain"), release: n("release") },
        volume: n("master"),
        pan: n("pan"),
        velocity: n("velocity"),
        lfo1: lfo(1),
        lfo2: lfo(2),
        filter_env_amount: n("filterEnvAmt"),
        fm_amount: n("fmAmount"),
        fm_algorithm: n("fmAlgo"),
        mod_envelope: Adsr {
            attack: n("modEnvAttack"),
            decay: n("modEnvDecay"),
            sustain: n("modEnvSustain"),
            release: n("modEnvRelease"),
        },
        mod_env_target: n("modEnvTarget"),
        mod_env_depth: n("modEnvDepth"),
        bypass: SynthBypass {
            noise: b("bypassNoise"),
            filter: b("bypassFilter"),
            env: b("bypassEnv"),
            lfo1: b("bypassLfo1"),
            lfo2: b("bypassLfo2"),
            fm: b("bypassFm"),
            mod_env: b("bypassModEnv"),
            filter_env: b("bypassFilterEnv"),
        },
    }
}

/// Parameter changes of an effect case: `(quantum, params)`.
fn changes(case: &Value) -> Vec<(usize, Object)> {
    case.get("changes")
        .and_then(Value::as_array)
        .expect("changes")
        .iter()
        .map(|c| (num(c, "quantum") as usize, c.get("params").and_then(Value::as_object).cloned().expect("params")))
        .collect()
}

fn render_effect(case: &Value, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let input = case.get("input").and_then(Value::as_array).expect("input");
    let seed = |i: usize| input[i].as_f64().expect("seed") as u32;
    let gain = input[2].as_f64().expect("gain");
    let (in_l, in_r) = (burst(frames, seed(0), gain), burst(frames, seed(1), gain));
    let rate = num(case, "sampleRate") as f32;
    let changes = changes(case);
    let (mut out_l, mut out_r) = (vec![0.0; frames], vec![0.0; frames]);

    match case.get("processor").and_then(Value::as_str) {
        Some("strip") => {
            let mut strip = Strip::new(rate);
            let mut settings = StripSettings::default();
            for (q, from) in (0..frames).step_by(QUANTUM).enumerate() {
                for (_, params) in changes.iter().filter(|(at, _)| *at == q) {
                    for (key, v) in params.iter() {
                        set_strip(&mut settings, key, v.as_f64().expect("number") as f32);
                    }
                }
                let to = (from + QUANTUM).min(frames);
                strip.process(&settings, &in_l[from..to], &in_r[from..to], &mut out_l[from..to], &mut out_r[from..to]);
            }
        }
        Some("master-bus") => {
            let mut bus = MasterBus::new(rate);
            let mut settings = MasterSettings::default();
            out_l.copy_from_slice(&in_l);
            out_r.copy_from_slice(&in_r);
            for (q, from) in (0..frames).step_by(QUANTUM).enumerate() {
                for (_, params) in changes.iter().filter(|(at, _)| *at == q) {
                    for (key, v) in params.iter() {
                        set_master(&mut settings, key, v.as_f64().expect("number") as f32);
                    }
                }
                let to = (from + QUANTUM).min(frames);
                bus.process(&settings, &mut out_l[from..to], &mut out_r[from..to]);
            }
        }
        other => panic!("processor {other:?}"),
    }
    (out_l, out_r)
}

/// Whether a case uses what a processor does differently from the JS one on
/// purpose. Such a case is checked against a Rust snapshot.
fn departs_from_js(case: &Value, fx: &Fixtures) -> bool {
    match case.get("processor").and_then(Value::as_str) {
        Some("synth-voice") => synth_departs(case),
        _ => effect_departs(case, fx),
    }
}

/// Whether a strip or master bus case uses what they do on purpose
/// (`strip.rs`, `master_bus.rs`): the reverb at another rate than 44.1 kHz
/// (its tunings follow the rate), the tape saturation, or a master output
/// the JS clips above the limit's knee.
fn effect_departs(case: &Value, fx: &Fixtures) -> bool {
    /// `LIMIT_KNEE` of the master bus.
    const LIMIT_KNEE: f32 = 0.9;
    let params: Vec<Object> = changes(case).into_iter().map(|(_, p)| p).collect();
    let set = |key: &str| params.iter().filter_map(|p| p.get(key).and_then(Value::as_f64)).collect::<Vec<_>>();
    match case.get("processor").and_then(Value::as_str) {
        Some("strip") => {
            let reverb = set("revMix").iter().any(|&m| m > 0.0) && num(case, "sampleRate") != 44_100.0;
            let tape = set("satType").contains(&2.0) && set("satMix").iter().any(|&m| m > 0.0);
            reverb || tape
        }
        _ => {
            let (offset, stored) = (num(case, "offset") as usize, num(case, "stored") as usize);
            fx.data[offset..offset + 2 * stored].iter().any(|x| x.abs() > LIMIT_KNEE)
        }
    }
}

/// Whether a synth case uses what the voice does differently on purpose
/// (`synth.rs`): the sub, the pitch punch or FM, set or reached by an LFO.
fn synth_departs(case: &Value) -> bool {
    /// LFO targets: sub gain, pitch punch, FM amount, FM algorithm.
    const DEPARTING_TARGETS: [f64; 4] = [19.0, 20.0, 24.0, 25.0];
    let messages = case.get("messages").and_then(Value::as_array).expect("messages");
    let update = messages.iter().find(|m| m.get("type").and_then(Value::as_str) == Some("update")).expect("update");
    let patch = patch_from_update(update);
    patch.sub_gain > 0.0
        || patch.pitch_punch > 0.0
        || patch.fm_amount > 0.0
        || [patch.lfo1.target, patch.lfo2.target].iter().any(|t| DEPARTING_TARGETS.contains(t))
}

/// The Rust snapshots: the synth voice's departing cases, and the strip's and
/// master bus's.
const SNAPSHOTS: [(&str, bool); 2] = [("synth_renders.bin", true), ("effect_renders.bin", false)];

fn snapshot_path(file: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(file)
}

/// The stored frames of every synth case (else effect case) that departs
/// from JS, left then right.
fn snapshot(fx: &Fixtures, synth: bool) -> Vec<(String, Vec<f32>)> {
    let of_kind = |case: &&Value| (case.get("processor").and_then(Value::as_str) == Some("synth-voice")) == synth;
    fx.cases
        .iter()
        .filter(of_kind)
        .filter(|case| departs_from_js(case, fx))
        .map(|case| {
            let frames = num(case, "frames") as usize;
            let (l, r) = if synth { render_synth(case, frames) } else { render_effect(case, frames) };
            let thin = |s: Vec<f32>| s.into_iter().step_by(SNAPSHOT_STRIDE);
            let name = case.get("name").and_then(Value::as_str).expect("name").to_owned();
            (name, thin(l).chain(thin(r)).collect())
        })
        .collect()
}

/// Writes the snapshots from the processors as they are: `cargo test -p
/// ordrumbox-dsp --test processors -- --ignored`, when their sound changes on
/// purpose (`README.md`, "Golden fixtures").
#[test]
#[ignore = "writes the snapshots"]
fn write_the_snapshots() {
    let fx = fixtures();
    for (file, synth) in SNAPSHOTS {
        let bytes: Vec<u8> = snapshot(&fx, synth).iter().flat_map(|(_, s)| s).flat_map(|x| x.to_le_bytes()).collect();
        std::fs::write(snapshot_path(file), bytes).expect("written");
    }
}

#[test]
fn departing_cases_match_their_snapshots() {
    let fx = fixtures();
    for (file, synth) in SNAPSHOTS {
        check_snapshot(file, &snapshot(&fx, synth));
    }
}

fn check_snapshot(file: &str, cases: &[(String, Vec<f32>)]) {
    let bytes = std::fs::read(snapshot_path(file)).expect(file);
    let stored: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    assert_eq!(stored.len(), cases.iter().map(|(_, s)| s.len()).sum::<usize>());
    let mut rest = stored.as_slice();
    for (name, samples) in cases {
        let (expected, after) = rest.split_at(samples.len());
        rest = after;
        for (got, want) in samples.iter().zip(expected) {
            assert!((got - want).abs() <= TOLERANCE, "{name}: {got} instead of {want}");
        }
    }
}

/// The messages WorkletSynthVoice posts (update, trigger, update with the
/// same values, release) as one start and one release.
fn render_synth(case: &Value, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let rate = num(case, "sampleRate") as f32;
    let messages = case.get("messages").and_then(Value::as_array).expect("messages");
    let of_type =
        |kind: &'static str| messages.iter().filter(move |m| m.get("type").and_then(Value::as_str) == Some(kind));
    let patch = patch_from_update(of_type("update").next_back().expect("update"));
    for update in of_type("update") {
        assert_eq!(patch_from_update(update), patch, "updates differ");
    }
    let mut voice = SynthVoice::default();
    voice.start(&patch, 0, num(of_type("trigger").next().expect("trigger"), "startTime"), 1);
    voice.release_at(num(of_type("release").next().expect("release"), "releaseTime"));
    assert_eq!(messages.len(), 4, "update, trigger, update, release");

    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    for from in (0..frames).step_by(QUANTUM) {
        let to = (from + QUANTUM).min(frames);
        voice.render(rate, from as u64, &mut l[from..to], &mut r[from..to]);
    }
    (l, r)
}

#[test]
fn processors_match_the_js_worklets() {
    let fx = fixtures();
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for case in &fx.cases {
        let name = case.get("name").and_then(Value::as_str).expect("name");
        let processor = case.get("processor").and_then(Value::as_str).expect("processor");
        if departs_from_js(case, &fx) {
            continue;
        }
        let frames = num(case, "frames") as usize;
        let stored = num(case, "stored") as usize;
        let offset = num(case, "offset") as usize;
        let (l, r) = if processor == "synth-voice" { render_synth(case, frames) } else { render_effect(case, frames) };

        let kept: Vec<usize> = (0..frames).filter(|&i| i < fx.full_frames || i % fx.stride == 0).collect();
        assert_eq!(kept.len(), stored, "{name}: stored frames");
        let expected_l = &fx.data[offset..offset + stored];
        let expected_r = &fx.data[offset + stored..offset + 2 * stored];
        let mut worst = (0.0_f32, 0, 'L');
        for (k, &i) in kept.iter().enumerate() {
            for (got, want, side) in [(l[i], expected_l[k], 'L'), (r[i], expected_r[k], 'R')] {
                let error = (got - want).abs();
                if error > worst.0 || error.is_nan() {
                    worst = (error, i, side);
                }
            }
        }
        report.push(format!("{processor:12} {name:16} max error {:.1e} (frame {} {})", worst.0, worst.1, worst.2));
        if worst.0.is_nan() || worst.0 > TOLERANCE {
            failures.push(name.to_owned());
        }
    }
    println!("{}", report.join("\n"));
    assert!(failures.is_empty(), "differ from JS: {failures:?}\n{}", report.join("\n"));
}
