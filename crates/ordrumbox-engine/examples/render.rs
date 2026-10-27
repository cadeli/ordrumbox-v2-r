//! Offline render of a pattern to a WAV file, through the same engine the app
//! plays with, timing how much faster than real time it runs.
//!
//! ```sh
//! cargo run --release -p ordrumbox-engine --example render -- <assets> <pattern-id> [seconds] [out.wav]
//! ```
//! `<assets>` is the `assets` folder of ordrumbox-v2.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::{Controller, SampleBank, SynthPresets, kits_from_json};

const RATE: u32 = 48_000;
const CALLBACK_FRAMES: usize = 512;

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn write_wav(path: &str, left: &[f32], right: &[f32]) -> std::io::Result<()> {
    let data_len = (left.len() * 4) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for (l, r) in left.iter().zip(right) {
        for s in [l, r] {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
    }
    std::fs::write(path, out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [assets, id, rest @ ..] = args.as_slice() else {
        eprintln!("usage: render <assets> <pattern-id> [seconds] [out.wav]");
        std::process::exit(2);
    };
    let seconds: f64 = rest.first().map_or(8.0, |s| s.parse().expect("seconds"));
    let assets = Path::new(assets);
    let data = assets.join("data");

    let (library, _) = Library::from_json(&read_json(&data.join("song.json")));
    let kits = kits_from_json(&read_json(&data.join("drumkits.json")));
    let scales = Scales::from_json(&read_json(&data.join("scales.json")));
    let synths = SynthPresets::from_json(&read_json(&data.join("generated_sounds.json")));
    let (bank, errors) = SampleBank::load(&kits, |url| std::fs::read(assets.join("kits").join(url)));
    if !errors.is_empty() {
        eprintln!("{} sample(s) not loaded, first: {}", errors.len(), errors[0]);
    }
    let Some(pattern) = library.pattern(id) else {
        let ids: Vec<&str> = library.patterns.iter().map(|p| p.id.as_str()).collect();
        eprintln!("no pattern {id}; patterns: {}", ids.join(" "));
        std::process::exit(1);
    };

    let (mut controller, mut audio) = Controller::new(RATE as f32, Arc::new(bank), scales, synths);
    controller.play(pattern);
    let frames = (seconds * f64::from(RATE)) as usize;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let started = Instant::now();
    for from in (0..frames).step_by(CALLBACK_FRAMES) {
        let to = (from + CALLBACK_FRAMES).min(frames);
        controller.update(pattern);
        audio.render(&mut left[from..to], &mut right[from..to]);
    }
    let elapsed = started.elapsed().as_secs_f64();
    let peak = left.iter().chain(&right).fold(0.0_f32, |m, s| m.max(s.abs()));
    let rms = (left.iter().chain(&right).map(|s| f64::from(*s).powi(2)).sum::<f64>() / (2 * frames) as f64).sqrt();
    println!(
        "{id}: {seconds} s rendered in {elapsed:.3} s ({:.0}x real time), peak {peak:.3}, rms {rms:.4}",
        seconds / elapsed
    );
    if let Some(out) = rest.get(1) {
        write_wav(out, &left, &right).unwrap_or_else(|e| panic!("{out}: {e}"));
        println!("wrote {out}");
    }
}
