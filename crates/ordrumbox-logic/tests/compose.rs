//! The drum lines of every style of `assets/data/styles.json`: what each
//! style promises (its required and forbidden steps, its meter, the exact
//! hits), how close the lines stay to its weights, and a snapshot of one
//! pattern per style (`tests/fixtures/compose_rust.txt`, written from this
//! code: `README.md`, "Golden fixtures").

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::drum_taxonomy::instrument_family;
use ordrumbox_core::json;
use ordrumbox_core::{Macros, Pattern, Rng, Scales, Style, Styles, Track};
use ordrumbox_logic::compose::{drum_line, track_settings};
use ordrumbox_logic::generate::{AutoGenerator, Options, generate_pattern};

fn styles() -> Styles {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/styles.json");
    let text = std::fs::read_to_string(path).expect("assets/data/styles.json");
    let (styles, issues) = Styles::from_json(&json::parse(&text).expect("styles.json parses"));
    assert_eq!(issues, [], "styles.json reads without an issue");
    styles
}

fn style<'s>(styles: &'s Styles, name: &str) -> &'s Style {
    styles.get(name).unwrap_or_else(|| panic!("no style {name}"))
}

/// The steps of the measure (`measure` steps long) a track plays, measure by measure.
fn measures(track: &Track, measure: usize) -> Vec<Vec<usize>> {
    let spb = track.steps_per_beat as usize;
    let total = track.beat_count as usize * spb;
    let mut out = vec![Vec::new(); total.div_ceil(measure)];
    for n in &track.notes {
        let a = n.beat as usize * spb + n.step as usize;
        out[a / measure].push(a % measure);
    }
    out.iter_mut().for_each(|m| m.sort_unstable());
    out
}

/// A line of part `name` of `style` on a track of two measures.
fn line(style: &Style, name: &str, partner: Option<&Track>, seed: u64) -> Track {
    let mut track = Track::new(name, style.beat_count * 2, style.steps_per_beat);
    drum_line(&mut track, style, Macros::default(), partner, 1.0, &mut Rng::new(seed));
    track
}

#[test]
fn the_fourteen_styles_and_their_aliases() {
    let styles = styles();
    let mut names = styles.names();
    names.sort_unstable();
    let expected = [
        "afrobeat",
        "bossa",
        "disco",
        "drumandbass",
        "funk",
        "hiphop",
        "house",
        "jazz",
        "reggae",
        "rock",
        "salsa",
        "samba",
        "techno",
        "trap",
    ];
    assert_eq!(names, expected);
    assert_eq!(style(&styles, "DnB").name, "drumandbass");
    assert_eq!(style(&styles, "ragga").name, "reggae");
}

#[test]
fn every_part_plays_its_hits_its_required_steps_and_never_a_forbidden_one() {
    let styles = styles();
    for style in &styles.list {
        let n = style.measure_steps();
        for (name, part) in &style.parts {
            for seed in 0..40 {
                let track = line(style, name, None, seed);
                let shown = track_settings(&track, style, Macros::default()).unwrap();
                let wanted = (shown.used.hits_per_measure as usize).min(shown.open_hits).max(part.required.len());
                for steps in measures(&track, n) {
                    let at = format!("{} {name} seed {seed}: {steps:?}", style.name);
                    assert_eq!(steps.len(), wanted, "{at}");
                    assert!(part.required.iter().all(|r| steps.contains(r)), "{at}");
                    assert!(!part.forbidden.iter().any(|f| steps.contains(f)), "{at}");
                }
            }
        }
    }
}

#[test]
fn the_signature_figures_hold() {
    let styles = styles();
    for seed in 0..50 {
        // Reggae's one drop: the kick on the third beat, never on the first.
        let kick = line(style(&styles, "reggae"), "KICK", None, seed);
        assert_eq!(measures(&kick, 16), [vec![8], vec![8]]);
        // Salsa's 3-2 son clave, every time.
        let clave = line(style(&styles, "salsa"), "CLAVES", None, seed);
        assert_eq!(measures(&clave, 32), [vec![0, 6, 12, 20, 24], vec![0, 6, 12, 20, 24]]);
        // Four on the floor.
        let kick = line(style(&styles, "house"), "KICK", None, seed);
        assert!(measures(&kick, 16).iter().all(|m| m == &[0, 4, 8, 12]));
        // Trap: the snare on the third beat (half time).
        let snare = line(style(&styles, "trap"), "SNARE", None, seed);
        assert!(measures(&snare, 16).iter().all(|m| m == &[8]));
    }
    // Jazz and afrobeat are ternary: 3 steps a beat.
    for name in ["jazz", "afrobeat"] {
        let ride = line(style(&styles, name), "CHH", None, 1);
        assert_eq!(ride.steps_per_beat, 3, "{name}");
    }
}

#[test]
fn at_full_style_weight_the_steps_played_follow_the_weights() {
    let styles = styles();
    for name in ["hiphop", "funk", "drumandbass", "samba"] {
        let style = style(&styles, name);
        let (part_name, part) =
            style.parts.iter().find(|(_, p)| p.required.len() < p.hits_per_measure as usize).unwrap();
        let n = style.measure_steps();
        let mut counts = vec![0.0; n];
        for seed in 0..200 {
            let mut track = Track::new(part_name, style.beat_count, style.steps_per_beat);
            track.compose_style_weight = 100.0;
            drum_line(&mut track, style, Macros::default(), None, 1.0, &mut Rng::new(seed));
            for m in measures(&track, n) {
                m.into_iter().for_each(|s| counts[s] += 1.0);
            }
        }
        let r = correlation(&counts, &part.weights);
        assert!(r > 0.7, "{name} {part_name}: correlation {r}");
    }
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (ma, mb) = (mean(a), mean(b));
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

#[test]
fn a_part_coupled_to_avoid_its_partner_avoids_it() {
    let styles = styles();
    let style = style(&styles, "hiphop");
    for seed in 0..50 {
        let kick = line(style, "KICK", None, seed);
        let snare = line(style, "SNARE", Some(&kick), seed + 100);
        let kicks = measures(&kick, 16);
        for (m, steps) in measures(&snare, 16).iter().enumerate() {
            // The backbeat is required; the ghost notes keep off the kick.
            let extra: Vec<&usize> = steps.iter().filter(|s| **s != 4 && **s != 12).collect();
            assert!(extra.iter().all(|s| !kicks[m].contains(s)), "kick {:?} snare {steps:?}", kicks[m]);
        }
    }
}

// ─── Snapshot ────────────────────────────────────────────────────────────────

/// One pattern per style and seed, as this code generates it.
fn rust_snapshot() -> Vec<String> {
    let styles = styles();
    let scales = Scales::default();
    let mut lines = Vec::new();
    for style in &styles.list {
        for seed in [1u64, 2] {
            let mut rng = Rng::new(seed);
            let mut rnd = || rng.next_f64();
            let mut pattern = Pattern { beat_count: 4, ..Pattern::default() };
            let options = Options { genre: Some(style.name.clone()), ..Options::default() };
            generate_pattern(&mut AutoGenerator::new(&scales, &styles, &mut rnd), &mut pattern, &options);
            pattern.tracks.retain(|t| instrument_family(&t.name).is_drum());
            lines.push(format!("{} seed {seed} {}", style.name, pattern.to_json().to_json()));
        }
    }
    lines
}

fn snapshot_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compose_rust.txt")
}

/// Writes `compose_rust.txt`: `cargo test -p ordrumbox-logic --test compose
/// -- --ignored`, when the drum lines change on purpose.
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    std::fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn every_style_matches_its_snapshot() {
    let text = std::fs::read_to_string(snapshot_path()).expect("tests/fixtures/compose_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}
