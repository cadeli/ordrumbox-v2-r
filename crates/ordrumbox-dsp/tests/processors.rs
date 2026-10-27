//! The processors against the JS AudioWorklet processors, run in Node by
//! `tools/export_dsp_fixtures.mjs` on the same inputs, quantum by quantum.

use std::path::Path;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_dsp::synth::{Adsr, SynthBypass, SynthLfo};
use ordrumbox_dsp::{MasterBus, MasterSettings, Strip, StripSettings, SynthPatch, SynthVoice};

const QUANTUM: usize = 128;
/// Largest difference allowed with the JS output (math functions of V8 and of
/// the platform may differ in their last bit).
const TOLERANCE: f32 = 2e-5;

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
    let data = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
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
        "satType" => &mut settings.sat_type,
        "satDrive" => &mut settings.sat_drive,
        "satOut" => &mut settings.sat_out,
        "satMix" => &mut settings.sat_mix,
        "revRoom" => &mut settings.rev_room,
        "revDamp" => &mut settings.rev_damp,
        "revWidth" => &mut settings.rev_width,
        "revMix" => &mut settings.rev_mix,
        "dlyTimeL" => &mut settings.dly_time_l,
        "dlyTimeR" => &mut settings.dly_time_r,
        "dlyFb" => &mut settings.dly_fb,
        "dlyMix" => &mut settings.dly_mix,
        "dlyMode" => &mut settings.dly_mode,
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
        drive: n("drive"),
        pitch_punch: n("pitchPunch"),
        sub_gain: n("subGain"),
        envelope: Adsr { attack: n("attack"), decay: n("decay"), sustain: n("sustain"), release: n("release") },
        master: n("master"),
        pan: n("pan"),
        velocity: n("velocity"),
        lfo1: lfo(1),
        lfo2: lfo(2),
        filter_env_amount: n("filterEnvAmt"),
        fm_amount: n("fmAmount"),
        fm_algo: n("fmAlgo"),
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
