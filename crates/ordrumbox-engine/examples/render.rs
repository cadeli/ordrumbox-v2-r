//! Offline render of a pattern or a song to a WAV file, as the app exports
//! it, timing how much faster than real time it runs.
//!
//! ```sh
//! cargo run --release -p ordrumbox-engine --example render -- <assets> <pattern-id> [loops] [out.wav]
//! cargo run --release -p ordrumbox-engine --example render -- <assets> song:<song-id> [out.wav]
//! ```
//! `<assets>` is the `assets` folder of ordrumbox-v2.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::offline::{EXPORT_RATE, RenderSetup, render_pattern, render_song};
use ordrumbox_engine::{SampleBank, SynthPresets, kits_from_json, wav};

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [assets, what, rest @ ..] = args.as_slice() else {
        eprintln!("usage: render <assets> <pattern-id> [loops] [out.wav] | render <assets> song:<id> [out.wav]");
        std::process::exit(2);
    };
    let assets = Path::new(assets);
    let data = assets.join("data");
    let (library, _) = Library::from_json(&read_json(&data.join("song.json")));
    let kits = kits_from_json(&read_json(&data.join("drumkits.json")));
    let (bank, errors) = SampleBank::load(&kits, |url| std::fs::read(assets.join("kits").join(url)));
    if !errors.is_empty() {
        eprintln!("{} sample(s) not loaded, first: {}", errors.len(), errors[0]);
    }
    let setup = RenderSetup {
        bank: Arc::new(bank),
        scales: Scales::from_json(&read_json(&data.join("scales.json"))),
        synths: SynthPresets::from_json(&read_json(&data.join("generated_sounds.json"))),
        kit: kits.first().map(|k| k.name.clone()).unwrap_or_default(),
        sample_rate: EXPORT_RATE,
        seed: 1,
    };

    let started = Instant::now();
    let (rendered, out) = if let Some(id) = what.strip_prefix("song:") {
        let index = library.songs.iter().position(|s| s.id == id);
        (index.and_then(|i| render_song(&library, i, &setup)), rest.first())
    } else {
        let index = library.patterns.iter().position(|p| p.id == *what);
        let loops = rest.first().map_or(1, |l| l.parse().expect("loops"));
        (index.and_then(|i| render_pattern(&library, i, loops, &setup)), rest.get(1))
    };
    let Some((left, right)) = rendered else {
        let ids: Vec<&str> = library.patterns.iter().map(|p| p.id.as_str()).collect();
        let songs: Vec<String> = library.songs.iter().map(|s| format!("song:{}", s.id)).collect();
        eprintln!("nothing to render for {what}; patterns: {}; songs: {}", ids.join(" "), songs.join(" "));
        std::process::exit(1);
    };
    let elapsed = started.elapsed().as_secs_f64();
    let seconds = left.len() as f64 / f64::from(EXPORT_RATE);
    let peak = left.iter().chain(&right).fold(0.0_f32, |m, s| m.max(s.abs()));
    println!("{what}: {seconds:.2} s rendered in {elapsed:.3} s ({:.0}x real time), peak {peak:.3}", seconds / elapsed);
    if let Some(out) = out {
        std::fs::write(out, wav::encode(&left, &right, EXPORT_RATE)).unwrap_or_else(|e| panic!("{out}: {e}"));
        println!("wrote {out}");
    }
}
