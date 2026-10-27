//! Bass lines and chords/melodies (`bass_generator.js`, `melody_generator.js`).

use ordrumbox_core::drum_taxonomy::{TrackType, detect_track_type};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Pattern, Scales, Track};

use super::auto::Harmony;
use super::{Accent, Gen, Phrases, Vel, add_note, apply_loop_point, clamp, cycle, loop_point, num, nums, text};

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
    let mut g = Gen::new("bass", scales, rnd);
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
            let mut pitch = |g: &mut Gen<'_>, phrase: &Value, cached: &[f64]| {
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
fn bass_step_grid(g: &mut Gen<'_>, track: &mut Track, tones: &[f64], config: &Value, density: f64, root: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let beat = num(config, "beat").unwrap_or(0.0) as i32;
    let probabilities: Vec<f64> = match super::list(config, "probabilitySets") {
        Some(sets) => g
            .pick(sets)
            .and_then(Value::as_array)
            .map(|row| row.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
            .unwrap_or_default(),
        None => nums(config, "probabilities").unwrap_or_else(|| vec![0.7, 0.7, 0.6, 0.7]),
    };
    let mut pitches = vec![tones.first().copied().unwrap_or(f64::NAN) + root];
    for _ in 1..spb {
        let tone = g.random_tone(tones);
        pitches.push(tone + root);
    }
    let accents = config.get("velocity").and_then(|v| nums(v, "accentPattern"));
    for step in 0..spb {
        if beat * spb + step >= loop_point {
            continue;
        }
        let probability = cycle(&probabilities, step as usize).unwrap_or(f64::NAN);
        if g.random() < probability * density {
            let accent = accents.as_deref().and_then(|a| cycle(a, step as usize)).unwrap_or(0.0);
            let context = Vel { accent: Accent::By(accent), fixed: false, ..Vel::default() };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitches[step as usize], velocity);
        }
    }
}

/// `generateBassGrooveVariant`: each beat on its root (`rootPattern`), the
/// other steps by chance, mostly strong intervals, sometimes a scale degree.
fn bass_groove(g: &mut Gen<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, root: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let roots = nums(config, "rootPattern").unwrap_or_default();
    let note_density = num(config, "density").unwrap_or(f64::NAN);
    let variation = num(config, "variation").unwrap_or(f64::NAN);
    let weight = num(config, "strongBeatWeight").unwrap_or(f64::NAN);
    let intervals = nums(config, "strongBeatIntervals").unwrap_or_default();
    let max_leap = num(config, "maxLeap").unwrap_or(f64::NAN);
    for beat in 0..track.beat_count {
        let root_pitch = cycle(&roots, beat as usize).unwrap_or(f64::NAN) + root;
        let mut last = root_pitch;
        for step in 0..spb {
            if beat * spb + step >= loop_point {
                continue;
            }
            let strong = step == 0;
            // `strong || Math.random() < density`: a NaN density keeps nothing.
            if !strong {
                let plays = g.random() < note_density * density;
                if !plays {
                    continue;
                }
            }
            let mut is_variation = false;
            let pitch = if strong || g.random() > variation {
                let interval = if g.random() < weight { intervals.first() } else { intervals.get(1) };
                root_pitch + interval.copied().unwrap_or(f64::NAN)
            } else {
                // `rootPitch + null` is the root in JS.
                let degree = g.pick(scale).copied().unwrap_or(0.0);
                let pitch = root_pitch + degree;
                is_variation = true;
                if (pitch - last).abs() > max_leap {
                    is_variation = false;
                    root_pitch
                } else {
                    pitch
                }
            };
            last = pitch;
            let context = Vel { variation: is_variation, ..Vel::accent(strong, false) };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitch, velocity);
        }
    }
}

/// `generateBassArpeggioVariant`: a contour over the scale from each beat's
/// root; off-beat notes may rest and fade across the beat.
fn bass_arpeggio(g: &mut Gen<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, root: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let roots = nums(config, "rootPattern").unwrap_or_else(|| vec![0.0]);
    let length = num(config, "phraseLength").unwrap_or(scale.len() as f64).max(2.0) as usize;
    let contour = text(config, "contour").unwrap_or("updown");
    let start = num(config, "startDegree").unwrap_or(0.0);
    let sequence = Gen::contour(scale, length, contour, start, &[0.0]);
    let average = num(config, "noteSpacing").unwrap_or(2.0).max(1.0) as i32;
    let jitter = num(config, "spacingJitter").unwrap_or(0.0).max(0.0) as i32;
    let rest_chance = clamp(num(config, "restChance").unwrap_or(0.15), 0.0, 1.0);
    let decay = clamp(num(config, "phraseDecay").unwrap_or(0.2), 0.0, 1.0);
    for beat in 0..track.beat_count {
        let root_pitch = cycle(&roots, beat as usize).unwrap_or(f64::NAN) + root;
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
                let context =
                    Vel { variation: index % sequence.len() != 0, scale: fade, ..Vel::accent(downbeat, false) };
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
    let bass = pattern.tracks.iter().find(|t| detect_track_type(&t.name) == TrackType::Bass)?;
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
    let mut g = Gen::new("melody", scales, rnd);
    let root = harmony.root;
    if variant == Some("break") {
        let Some(config) = g.config("break") else { return };
        track.notes.clear();
        let tones = g.scale_steps(harmony.scale.as_deref().or_else(|| text(config, "scaleName")));
        let bias = root + num(config, "octaveShift").unwrap_or(1.0) * 12.0;
        let mut pitch = |g: &mut Gen<'_>, phrase: &Value, _: &[f64]| g.phrase_pitch(phrase, &tones, &[], bias, None);
        let accent = |p: &Value, _: i32| is_true(p, "accent");
        let ghost = |p: &Value, _: i32| is_true(p, "ghost");
        let opts = Phrases {
            default_loop_beats: 2.0,
            pitch: Some(&mut pitch),
            accent: Some(&accent),
            ghost: Some(&ghost),
            stacking: false,
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
            let mut pitch = |g: &mut Gen<'_>, phrase: &Value, cached: &[f64]| {
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
            };
            g.phrases(track, config, density, opts);
        }
    }
    apply_loop_point(track, config);
}

/// `generateMelodyGrooveVariant`: strong steps on chord intervals, the
/// others by chance, sometimes a scale degree within `maxLeap`.
fn melody_groove(g: &mut Gen<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, bias: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let intervals = nums(config, "strongBeatIntervals").unwrap_or_else(|| vec![0.0, 4.0, 7.0]);
    let note_density = num(config, "density").unwrap_or(0.5);
    let variation = num(config, "variation").unwrap_or(0.3);
    let max_leap = num(config, "maxLeap").unwrap_or(7.0);
    for beat in 0..track.beat_count {
        let mut last = bias;
        for step in 0..spb {
            if beat * spb + step >= loop_point {
                continue;
            }
            let strong = step == 0 || step == spb / 2;
            // `strong || Math.random() < density`: a NaN density keeps nothing.
            if !strong {
                let plays = g.random() < note_density * density;
                if !plays {
                    continue;
                }
            }
            let pitch = if strong || g.random() > variation {
                bias + g.pick(&intervals).copied().unwrap_or(0.0)
            } else {
                let pitch = bias + g.pick(scale).copied().unwrap_or(0.0);
                if (pitch - last).abs() > max_leap { bias } else { pitch }
            };
            last = pitch;
            let context = Vel { variation: !strong, ..Vel::accent(strong, false) };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitch, velocity);
        }
    }
}

/// `generateMelodyArpeggioVariant`: a contour over the scale, every
/// `noteSpacing` steps, each note an arpeggio when the variant has one.
fn melody_arpeggio(g: &mut Gen<'_>, track: &mut Track, scale: &[f64], config: &Value, density: f64, bias: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let length = num(config, "phraseLength").unwrap_or(scale.len() as f64).max(2.0) as usize;
    let contour = text(config, "contour").unwrap_or("updown");
    let sequence = Gen::contour(scale, length, contour, 0.0, &[0.0, 3.0, 5.0, 7.0]);
    let average = num(config, "noteSpacing").unwrap_or(2.0).max(1.0) as i32;
    let jitter = num(config, "spacingJitter").unwrap_or(0.0).max(0.0) as i32;
    let arp = config.get("arp").filter(|a| !a.is_null());
    let count = num(config, "retriggerCount").unwrap_or(1.0);
    let rate = num(config, "rate").unwrap_or(1.0);
    for beat in 0..track.beat_count {
        let (mut step, mut index) = (0, 0usize);
        while step < spb {
            if beat * spb + step >= loop_point {
                break;
            }
            if g.random() < density {
                let degree = cycle(&sequence, index).unwrap_or(0.0);
                let context = Vel { variation: index % sequence.len() != 0, ..Vel::accent(step % 4 == 0, false) };
                let velocity = g.velocity(config.get("velocity"), context);
                let note = add_note(track, beat, step, bias + degree, velocity);
                if let Some(arp) = arp {
                    note.arp = arp.clone();
                    note.retrigger_count = count as i32;
                    note.rate = rate;
                }
            }
            index += 1;
            step += g.step_advance(average, jitter);
        }
    }
}
