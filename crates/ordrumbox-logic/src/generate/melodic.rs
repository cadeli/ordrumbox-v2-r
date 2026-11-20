//! Bass lines and chords/melodies (`bass_generator.js`, `melody_generator.js`).

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Pattern, Scales, Track};

use super::auto::Harmony;
use super::{
    Accent, Generator, Phrases, VelocitySpec, add_note, apply_loop_point, clamp, cycle, loop_point, num, nums, text,
};

/// What a groove variant (bass or melody) plays without its setting in the
/// table: its step density, the chance of a scale degree instead of a chord
/// interval, the widest such leap (semitones).
const GROOVE_DENSITY: f64 = 0.5;
const GROOVE_VARIATION: f64 = 0.3;
const GROOVE_MAX_LEAP: f64 = 7.0;
/// A bass groove without its settings: the chance its strong steps take the
/// first interval, the intervals (root, fifth), the beats' roots (the key's).
const BASS_STRONG_BEAT_WEIGHT: f64 = 0.75;
const BASS_STRONG_BEAT_INTERVALS: [f64; 2] = [0.0, 7.0];
const ROOT_PATTERN: [f64; 1] = [0.0];
/// A step-grid bass without its probabilities: each step's chance.
const STEP_GRID_PROBABILITIES: [f64; 4] = [0.7, 0.7, 0.6, 0.7];

fn is_true(value: &Value, key: &str) -> bool {
    value.get(key) == Some(&Value::Bool(true))
}

// ─── Bass ────────────────────────────────────────────────────────────────────

/// `generateNewBass`: a line in `harmony` (the variant's scale and register
/// when it names none).
pub(crate) fn bass(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    harmony: &Harmony,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("bass", scales, rnd);
    g.tone_threshold = 5;
    let name = g.resolve_variant(variant);
    let Some(config) = g.config(&name).or_else(|| g.config("basic")) else { return };
    let scale_name = harmony.scale.as_deref().or_else(|| text(config, "scaleName"));
    let tones = g.scale_steps(scale_name);
    let root = num(config, "rootNote").unwrap_or(0.0) + harmony.root;
    track.notes.clear();
    match text(config, "mode") {
        Some("stepGrid") => bass_step_grid(&mut g, track, &tones, config, density, root),
        Some("groove") => bass_groove(&mut g, track, &tones, config, density, root),
        Some("arpeggio") => bass_arpeggio(&mut g, track, &tones, config, density, root),
        _ => {
            let approach = nums(config, "approachNotes");
            let mut pitch = |g: &mut Generator<'_>, phrase: &Value, cached: &[f64]| {
                g.phrase_pitch(phrase, &tones, cached, root, approach.as_deref())
            };
            let accent = |_: &Value, step: i32| step % 4 == 0;
            let opts = Phrases {
                default_loop_beats: 2.0,
                pitch: Some(&mut pitch),
                accent: Some(&accent),
                ..Phrases::default()
            };
            g.phrases(track, config, density, opts);
        }
    }
    apply_loop_point(track, config);
}

/// `generateBassStepGridVariant`: one beat, the root then random tones, each
/// step with its probability (one row of `probabilitySets`).
fn bass_step_grid(g: &mut Generator<'_>, track: &mut Track, tones: &[f64], config: &Value, density: f64, root: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let beat = num(config, "beat").unwrap_or(0.0) as i32;
    // A row of the sets drawn (one draw, as the sets' own order), else the
    // single list, else the default; a row with a non-number is no row.
    let row = super::list(config, "probabilitySets")
        .and_then(|sets| g.pick(sets))
        .and_then(Value::as_array)
        .and_then(|row| row.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>());
    let probabilities = row
        .or_else(|| nums(config, "probabilities"))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| STEP_GRID_PROBABILITIES.to_vec());
    let mut pitches = vec![tones.first().copied().unwrap_or(0.0) + root];
    for _ in 1..spb {
        let tone = g.random_tone(tones);
        pitches.push(tone + root);
    }
    let accents = config.get("velocity").and_then(|v| nums(v, "accentPattern"));
    for step in 0..spb {
        if beat * spb + step >= loop_point {
            continue;
        }
        let probability = cycle(&probabilities, step as usize).unwrap_or(0.0);
        if g.random() < probability * density {
            let accent = accents.as_deref().and_then(|a| cycle(a, step as usize)).unwrap_or(0.0);
            let context = VelocitySpec { accent: Accent::By(accent), fixed: false, ..VelocitySpec::default() };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitches[step as usize], velocity);
        }
    }
}

/// `generateBassGrooveVariant`: each beat on its root (`rootPattern`), the
/// other steps by chance, mostly strong intervals, sometimes a scale degree.
fn bass_groove(g: &mut Generator<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, root: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let roots = nums(config, "rootPattern").unwrap_or_else(|| ROOT_PATTERN.to_vec());
    let note_density = num(config, "density").unwrap_or(GROOVE_DENSITY);
    let variation = num(config, "variation").unwrap_or(GROOVE_VARIATION);
    let weight = num(config, "strongBeatWeight").unwrap_or(BASS_STRONG_BEAT_WEIGHT);
    let intervals = nums(config, "strongBeatIntervals").unwrap_or_else(|| BASS_STRONG_BEAT_INTERVALS.to_vec());
    let max_leap = num(config, "maxLeap").unwrap_or(GROOVE_MAX_LEAP);
    for beat in 0..track.beat_count {
        let root_pitch = cycle(&roots, beat as usize).unwrap_or(0.0) + root;
        let mut last = root_pitch;
        for step in 0..spb {
            if beat * spb + step >= loop_point {
                continue;
            }
            let strong = step == 0;
            if !step_plays(g, strong, note_density * density) {
                continue;
            }
            let mut is_variation = false;
            let pitch = if strong || g.random() > variation {
                let interval = if g.random() < weight { intervals.first() } else { intervals.get(1) };
                // A list of one interval: the second is the root.
                root_pitch + interval.copied().unwrap_or(0.0)
            } else {
                let (pitch, kept) = scale_step(g, scale, root_pitch, last, max_leap);
                is_variation = kept;
                pitch
            };
            last = pitch;
            let context = VelocitySpec { variation: is_variation, ..VelocitySpec::accent(strong, false) };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitch, velocity);
        }
    }
}

/// `generateBassArpeggioVariant`: a contour over the scale from each beat's
/// root; off-beat notes may rest and fade across the beat.
fn bass_arpeggio(g: &mut Generator<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, root: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let roots = nums(config, "rootPattern").unwrap_or_else(|| ROOT_PATTERN.to_vec());
    let length = num(config, "phraseLength").unwrap_or(scale.len() as f64).max(2.0) as usize;
    let contour = text(config, "contour").unwrap_or("updown");
    let start = num(config, "startDegree").unwrap_or(0.0);
    let sequence = Generator::contour(scale, length, contour, start, &[0.0]);
    let average = num(config, "noteSpacing").unwrap_or(2.0).max(1.0) as i32;
    let jitter = num(config, "spacingJitter").unwrap_or(0.0).max(0.0) as i32;
    let rest_chance = clamp(num(config, "restChance").unwrap_or(0.15), 0.0, 1.0);
    let decay = clamp(num(config, "phraseDecay").unwrap_or(0.2), 0.0, 1.0);
    for beat in 0..track.beat_count {
        let root_pitch = cycle(&roots, beat as usize).unwrap_or(0.0) + root;
        let (mut step, mut index) = (0, 0usize);
        while step < spb {
            if beat * spb + step >= loop_point {
                break;
            }
            let downbeat = step == 0;
            let rests = !downbeat && g.random() < rest_chance;
            if !rests && g.random() < density {
                let degree = cycle(&sequence, index).unwrap_or(0.0);
                let fade = if downbeat { 1.0 } else { 1.0 - decay * (f64::from(step) / f64::from(spb)) };
                let context = VelocitySpec {
                    variation: index % sequence.len() != 0,
                    scale: fade,
                    ..VelocitySpec::accent(downbeat, false)
                };
                let velocity = g.velocity(config.get("velocity"), context);
                add_note(track, beat, step, root_pitch + degree, velocity);
            }
            index += 1;
            step += g.step_advance(average, jitter);
        }
    }
}

// ─── Chords and melodies ─────────────────────────────────────────────────────

/// `#extractBassRootPitch`: the most frequent pitch of the pattern's bass
/// track (the first to reach the top count), `None` without bass notes.
fn bass_root(pattern: &Pattern) -> Option<f64> {
    let bass = pattern.tracks.iter().find(|t| instrument_family(&t.name) == InstrumentFamily::Bass)?;
    let mut counts: Vec<(f64, usize)> = Vec::new();
    let (mut best, mut best_count) = (bass.notes.first()?.pitch, 0);
    for note in &bass.notes {
        let count = match counts.iter_mut().find(|(p, _)| *p == note.pitch) {
            Some((_, c)) => {
                *c += 1;
                *c
            }
            None => {
                counts.push((note.pitch, 1));
                1
            }
        };
        if count > best_count {
            best_count = count;
            best = note.pitch;
        }
    }
    Some(best)
}

/// `generateNewMelody` for track `t` of `pattern`: chords that avoid
/// doubling the bass root (an octave up), grooves and arpeggios in `harmony`.
pub(crate) fn melody(
    pattern: &mut Pattern,
    t: usize,
    variant: Option<&str>,
    density: f64,
    harmony: &Harmony,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let bass_root = bass_root(pattern);
    let Some(track) = pattern.tracks.get_mut(t) else { return };
    let mut g = Generator::new("melody", scales, rnd);
    let root = harmony.root;
    if variant == Some("break") {
        let Some(config) = g.config("break") else { return };
        track.notes.clear();
        let tones = g.scale_steps(harmony.scale.as_deref().or_else(|| text(config, "scaleName")));
        let bias = root + num(config, "octaveShift").unwrap_or(1.0) * 12.0;
        let mut pitch =
            |g: &mut Generator<'_>, phrase: &Value, _: &[f64]| g.phrase_pitch(phrase, &tones, &[], bias, None);
        let accent = |p: &Value, _: i32| is_true(p, "accent");
        let ghost = |p: &Value, _: i32| is_true(p, "ghost");
        let opts = Phrases {
            default_loop_beats: 2.0,
            pitch: Some(&mut pitch),
            accent: Some(&accent),
            ghost: Some(&ghost),
            stacking: false,
            keep_accents: false,
        };
        g.phrases(track, config, density, opts);
        apply_loop_point(track, config);
        return;
    }
    if matches!(variant, Some("intro" | "outro")) {
        return; // the notes stay as they are
    }
    let name = g.resolve_variant(variant);
    let Some(config) = g.config(&name).or_else(|| g.config("sparse")) else { return };
    let tones = g.scale_steps(harmony.scale.as_deref().or_else(|| text(config, "scaleName")));
    let bias = root + num(config, "octaveShift").unwrap_or(1.0) * 12.0;
    track.notes.clear();
    match text(config, "mode") {
        Some("groove") => melody_groove(&mut g, track, &tones, config, density, bias),
        Some("arpeggio") => melody_arpeggio(&mut g, track, &tones, config, density, bias),
        _ => {
            // `#resolveChordPitch`: a chord note on the bass root's pitch class goes an octave up.
            let mut pitch = |g: &mut Generator<'_>, phrase: &Value, cached: &[f64]| {
                let base = g.phrase_pitch(phrase, &tones, cached, 0.0, None);
                match bass_root {
                    Some(bass) if base.rem_euclid(12.0) == bass.rem_euclid(12.0) => base + 12.0 + bias,
                    _ => base + bias,
                }
            };
            let accent = |_: &Value, step: i32| step % 2 == 0;
            let opts = Phrases {
                default_loop_beats: 2.0,
                pitch: Some(&mut pitch),
                accent: Some(&accent),
                ghost: None,
                stacking: true,
                keep_accents: false,
            };
            g.phrases(track, config, density, opts);
        }
    }
    apply_loop_point(track, config);
}

/// Whether a step plays: a strong one always does, the others by chance
/// (`chance`; a NaN one keeps nothing).
fn step_plays(g: &mut Generator<'_>, strong: bool, chance: f64) -> bool {
    strong || g.random() < chance
}

/// A scale degree off `root_pitch`: the pitch (the root again when it
/// leaps more than `max_leap` from `last`), and whether the degree was kept.
/// A scale with no degree gives the root.
fn scale_step(g: &mut Generator<'_>, scale: &[f64], root_pitch: f64, last: f64, max_leap: f64) -> (f64, bool) {
    let pitch = root_pitch + g.pick(scale).copied().unwrap_or(0.0);
    if (pitch - last).abs() > max_leap { (root_pitch, false) } else { (pitch, true) }
}

/// `generateMelodyGrooveVariant`: strong steps on chord intervals, the
/// others by chance, sometimes a scale degree within `maxLeap`.
fn melody_groove(g: &mut Generator<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, bias: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let intervals = nums(config, "strongBeatIntervals").unwrap_or_else(|| vec![0.0, 4.0, 7.0]);
    let note_density = num(config, "density").unwrap_or(GROOVE_DENSITY);
    let variation = num(config, "variation").unwrap_or(GROOVE_VARIATION);
    let max_leap = num(config, "maxLeap").unwrap_or(GROOVE_MAX_LEAP);
    for beat in 0..track.beat_count {
        let mut last = bias;
        for step in 0..spb {
            if beat * spb + step >= loop_point {
                continue;
            }
            let strong = step == 0 || step == spb / 2;
            if !step_plays(g, strong, note_density * density) {
                continue;
            }
            let pitch = if strong || g.random() > variation {
                bias + g.pick(&intervals).copied().unwrap_or(0.0)
            } else {
                scale_step(g, scale, bias, last, max_leap).0
            };
            last = pitch;
            let context = VelocitySpec { variation: !strong, ..VelocitySpec::accent(strong, false) };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitch, velocity);
        }
    }
}

/// `generateMelodyArpeggioVariant`: a contour over the scale, every
/// `noteSpacing` steps, each note an arpeggio when the variant has one.
fn melody_arpeggio(g: &mut Generator<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, bias: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let length = num(config, "phraseLength").unwrap_or(scale.len() as f64).max(2.0) as usize;
    let contour = text(config, "contour").unwrap_or("updown");
    let sequence = Generator::contour(scale, length, contour, 0.0, &[0.0, 3.0, 5.0, 7.0]);
    let average = num(config, "noteSpacing").unwrap_or(2.0).max(1.0) as i32;
    let jitter = num(config, "spacingJitter").unwrap_or(0.0).max(0.0) as i32;
    let arp = config.get("arp").filter(|a| !a.is_null());
    let count = num(config, "retriggerCount").unwrap_or(1.0);
    let rate = num(config, "retriggerRate").unwrap_or(1.0);
    for beat in 0..track.beat_count {
        let (mut step, mut index) = (0, 0usize);
        while step < spb {
            if beat * spb + step >= loop_point {
                break;
            }
            if g.random() < density {
                let degree = cycle(&sequence, index).unwrap_or(0.0);
                let context = VelocitySpec {
                    variation: index % sequence.len() != 0,
                    ..VelocitySpec::accent(step % 4 == 0, false)
                };
                let velocity = g.velocity(config.get("velocity"), context);
                let note = add_note(track, beat, step, bias + degree, velocity);
                if let Some(arp) = arp {
                    note.arp = arp.clone();
                    note.retrigger_count = count as i32;
                    note.retrigger_rate = rate;
                }
            }
            index += 1;
            step += g.step_advance(average, jitter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{list, nums, table};
    use ordrumbox_core::json::Value;

    /// The numbers each bass mode reads: the table holds them all, so its
    /// defaults never stand in for a typing mistake in `generators.json`.
    #[test]
    fn the_bass_table_holds_what_each_mode_reads() {
        let numbers: [(&str, &[&str]); 2] =
            [("groove", &["density", "variation", "strongBeatWeight", "maxLeap"]), ("arpeggio", &[])];
        let lists: [(&str, &[&str]); 3] = [
            ("groove", &["rootPattern", "strongBeatIntervals"]),
            ("arpeggio", &["rootPattern"]),
            ("stepGrid", &["probabilities"]),
        ];
        for (name, config) in table("bass").iter() {
            let mode = config.get("mode").and_then(Value::as_str).unwrap_or("phrases");
            for (_, keys) in numbers.iter().filter(|(m, _)| *m == mode) {
                for key in *keys {
                    assert!(config.get(key).and_then(Value::as_f64).is_some(), "bass.{name}.{key}");
                }
            }
            for (_, keys) in lists.iter().filter(|(m, _)| *m == mode) {
                for key in *keys {
                    assert!(nums(config, key).is_some_and(|v| !v.is_empty()), "bass.{name}.{key}");
                }
            }
            for row in list(config, "probabilitySets").unwrap_or_default() {
                assert!(row.as_array().is_some_and(|r| r.iter().all(|v| v.as_f64().is_some())), "bass.{name}");
            }
        }
    }

    #[test]
    fn the_random_table_holds_its_numbers() {
        let config = table("random");
        for key in ["densityMin", "densitySpread", "pitchMin", "pitchRange", "velocityMin", "velocitySpread"] {
            assert!(config.get(key).and_then(Value::as_f64).is_some(), "random.{key}");
        }
    }

    #[test]
    fn a_list_with_a_non_number_is_no_list() {
        let config = ordrumbox_core::json::parse(r#"{"a":[1,2],"b":[1,"x"],"c":3}"#).unwrap();
        assert_eq!(nums(&config, "a"), Some(vec![1.0, 2.0]));
        assert_eq!(nums(&config, "b"), None);
        assert_eq!(nums(&config, "c"), None);
        assert_eq!(nums(&config, "d"), None);
    }
}
