//! Drum and percussion lines (`kick_generator.js`, `snare_generator.js`,
//! `hat_generator.js`, `clap_generator.js`, `cowbell_generator.js`,
//! `perc_generator.js`).

use ordrumbox_core::json::Value;
use ordrumbox_core::{Scales, Track, js};

use super::{
    Accent, Generator, Grid, Phrases, VelocitySpec, add_note, apply_loop_point, cycle, loop_point, num, nums, text,
    with_steps_per_beat,
};

fn is_true(value: &Value, key: &str) -> bool {
    value.get(key) == Some(&Value::Bool(true))
}

/// The phrases of a drum variant: accents and ghosts as the phrase says.
fn drum_phrases(g: &mut Generator<'_>, track: &mut Track, config: &Value, density: f64) {
    let accent = |p: &Value, _: i32| is_true(p, "accent");
    let ghost = |p: &Value, _: i32| is_true(p, "ghost");
    let mut zero = |_: &mut Generator<'_>, _: &Value, _: &[f64]| 0.0;
    let opts = Phrases {
        default_loop_beats: 2.0,
        pitch: Some(&mut zero),
        accent: Some(&accent),
        ghost: Some(&ghost),
        stacking: false,
    };
    g.phrases(track, config, density, opts);
}

/// `generateNewKick`.
pub(crate) fn kick(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("kick", scales, rnd);
    // A random kick is never the `break` one.
    let name = match variant {
        Some(v) if !v.is_empty() && g.configs.contains_key(v) => v.to_owned(),
        _ => g.random_variant(&["break"]),
    };
    let Some(config) = g.config(&name).or_else(|| g.config("basic")) else { return };
    track.notes.clear();
    if text(config, "mode") == Some("grid") {
        let accent = |_: i32, step: i32| step == 0;
        let ghost = |_: i32, step: i32| step != 0;
        let opts = Grid { default_loop_beats: 1.0, accent: Some(&accent), ghost: Some(&ghost), pitch: None };
        g.grid(track, config, density, opts);
    } else {
        drum_phrases(&mut g, track, config, density);
    }
    apply_loop_point(track, config);
}

/// `generateNewClap`.
pub(crate) fn clap(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("clap", scales, rnd);
    let name = g.resolve_variant(variant);
    let Some(config) = g.config(&name).or_else(|| g.config("sparse")) else { return };
    track.notes.clear();
    if text(config, "mode") == Some("grid") {
        g.grid(track, config, density, Grid::beats(2.0));
    } else {
        drum_phrases(&mut g, track, config, density);
    }
    apply_loop_point(track, config);
}

/// `generateNewCowbell`.
pub(crate) fn cowbell(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("cowbell", scales, rnd);
    let name = g.resolve_variant(variant);
    let Some(config) = g.config(&name).or_else(|| g.config("basic")) else { return };
    track.notes.clear();
    if text(config, "mode") == Some("grid") {
        g.grid(track, config, density, Grid::beats(1.0));
    } else {
        drum_phrases(&mut g, track, config, density);
    }
    apply_loop_point(track, config);
}

// ─── Snare ───────────────────────────────────────────────────────────────────

/// `generateNewSnare`.
pub(crate) fn snare(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("snare", scales, rnd);
    let name = g.resolve_variant(variant);
    let Some(config) = g.config(&name).or_else(|| g.config("basic")) else { return };
    track.notes.clear();
    match text(config, "mode") {
        Some("grid") => g.grid(track, config, density, Grid::beats(2.0)),
        Some("fill") => snare_fill(&mut g, track, config, density),
        Some("roll") => snare_roll(&mut g, track, config),
        Some("breakCrescendo") => break_crescendo(&mut g, track, config),
        _ => drum_phrases(&mut g, track, config, density),
    }
    apply_loop_point(track, config);
}

/// `Math.max(1, Math.round(1 + (count - 1) * progress))`: the hits of a ratchet.
fn ratchet(count: f64, progress: f64) -> i32 {
    js::round(1.0 + (count - 1.0) * progress).max(1.0) as i32
}

/// `generateSnareRollVariant`: the last beat, louder and with more hits step after step.
fn snare_roll(g: &mut Generator<'_>, track: &mut Track, config: &Value) {
    let loop_point = loop_point(track, config, 1.0);
    let spb = track.steps_per_beat;
    let last_beat = (track.beat_count - 1).max(0);
    let last_step = (spb - 1).max(0);
    let count = num(config, "retriggerCount").unwrap_or(4.0);
    let rate = num(config, "retriggerRate").unwrap_or(1.0);
    let (min, max) = (num(config, "minVelocity").unwrap_or(0.3), num(config, "maxVelocity").unwrap_or(1.0));
    for step in 0..spb {
        if last_beat * spb + step >= loop_point {
            continue;
        }
        let progress = if last_step == 0 { 1.0 } else { f64::from(step) / f64::from(last_step) };
        let base = min + (max - min) * progress;
        let hits = ratchet(count, progress);
        let context = VelocitySpec { base: Some(base), ..VelocitySpec::accent(step == last_step, step != last_step) };
        let velocity = g.velocity(config.get("velocity"), context);
        let note = add_note(track, last_beat, step, 0.0, velocity);
        if hits > 1 {
            note.retrigger_count = hits;
            note.retrigger_rate = rate;
        }
    }
}

/// `generateSnareFillVariant`: the last beats, step by step with a chance.
fn snare_fill(g: &mut Generator<'_>, track: &mut Track, config: &Value, density: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let start = (track.beat_count - num(config, "startBeatOffset").unwrap_or(1.0) as i32).max(0);
    let steps = nums(config, "steps").unwrap_or_default();
    let fill_density = num(config, "density").unwrap_or(f64::NAN);
    for beat in start..track.beat_count {
        for &step in &steps {
            let step = step as i32;
            if step >= spb || g.random() >= fill_density * density {
                continue;
            }
            if beat * spb + step >= loop_point {
                continue;
            }
            let context = VelocitySpec::accent(step == 0 || step == spb - 1, step != 0);
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, 0.0, velocity);
        }
    }
}

/// `generateBreakCrescendo`: every step of the last `stepsBack`, more and
/// more likely and with more and more hits.
fn break_crescendo(g: &mut Generator<'_>, track: &mut Track, config: &Value) {
    let spb = track.steps_per_beat;
    let length = track.beat_count * spb;
    let steps_back = num(config, "stepsBack").unwrap_or(16.0);
    let start = (f64::from(length) - steps_back).max(0.0) as i32;
    let count_max = num(config, "retriggerCountMax").unwrap_or(0.0);
    let rate = num(config, "retriggerRate").unwrap_or(1.0);
    for abs in start..length {
        let (beat, beat_step) = (abs / spb, abs % spb);
        let prob = f64::from(abs - start + 1) / steps_back;
        let velocity = g.velocity(config.get("velocity"), VelocitySpec::accent(beat_step == 0, false));
        let note = add_note(track, beat, beat_step, 0.0, velocity);
        note.probability = js::to_fixed(prob, 2);
        if count_max > 1.0 {
            let hits = ratchet(count_max, prob);
            if hits > 1 {
                note.retrigger_count = hits;
                note.retrigger_rate = rate;
            }
        }
    }
}

// ─── Hats ────────────────────────────────────────────────────────────────────

/// `getHatTrackType`: open hat for a name with OHH or OPEN, else closed.
fn hat_type(track: &Track) -> &'static str {
    let name = track.name.to_uppercase();
    if name.contains("OHH") || name.contains("OPEN") { "OHH" } else { "CHH" }
}

/// `generateNewHat`.
pub(crate) fn hat(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("hat", scales, rnd);
    let kind = hat_type(track);
    // `resolveHatVariantName`: a random variant is one of the hat's own kind.
    let name = match variant {
        Some(v) if !v.is_empty() && g.configs.contains_key(v) => v.to_owned(),
        _ => {
            let own: Vec<&str> =
                g.configs.iter().filter(|(_, c)| text(c, "trackType") == Some(kind)).map(|(k, _)| k).collect();
            g.pick(&own).map_or(if kind == "OHH" { "ohhBasic" } else { "chhBasic" }, |n| *n).to_owned()
        }
    };
    let Some(config) = g.config(&name).or_else(|| g.config("chhBasic")) else { return };
    track.notes.clear();
    match text(config, "mode") {
        Some("locked") => hat_locked(&mut g, track, config, density),
        Some("shaker") => hat_velocity(&mut g, track, config, density, 0.5, false),
        Some("ride") => hat_velocity(&mut g, track, config, density, 0.55, true),
        Some("grid") => {
            let half = track.steps_per_beat / 2;
            let accent = move |_: i32, step: i32| step == 0 || step == half;
            let ghost = |_: i32, step: i32| step % 2 != 0;
            let opts = Grid { default_loop_beats: 1.0, accent: Some(&accent), ghost: Some(&ghost), pitch: None };
            g.grid(track, config, density, opts);
        }
        Some("roll") => hat_roll(&mut g, track, config),
        Some("transition") => hat_transition(&mut g, track, config, kind),
        _ => drum_phrases(&mut g, track, config, density),
    }
    apply_loop_point(track, config);
}

/// `generateHatLockedVariant`: a velocity pattern over 16 steps a beat —
/// on a grid clamped to 8 (`withTemporaryStepsPerBeat(16)`), as the JS does;
/// each step kept with `density`, which the JS does not do.
fn hat_locked(g: &mut Generator<'_>, track: &mut Track, config: &Value, density: f64) {
    with_steps_per_beat(track, 16, |track| {
        let loop_point = loop_point(track, config, 1.0);
        let pattern = nums(config, "velocityPattern").unwrap_or_default();
        let accent_every = num(config, "accentEvery").unwrap_or(4.0);
        for beat in 0..track.beat_count {
            for step in 0..16 {
                if beat * 16 + step >= loop_point {
                    continue;
                }
                if g.random() >= density {
                    continue;
                }
                let base = cycle(&pattern, step as usize).unwrap_or(0.6);
                let accent = f64::from(step) % accent_every == 0.0;
                let context = VelocitySpec { base: Some(base), ..VelocitySpec::accent(accent, !accent) };
                let velocity = g.velocity(config.get("velocity"), context);
                add_note(track, beat, step, 0.0, velocity);
            }
        }
    });
}

/// `#generateHatVelocityVariant` (shaker, ride): a velocity pattern, each
/// step kept with `density`; the ride's bell step accented.
fn hat_velocity(g: &mut Generator<'_>, track: &mut Track, config: &Value, density: f64, default_base: f64, bell: bool) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let pattern = nums(config, "velocityPattern").unwrap_or_default();
    let accent_every = num(config, "accentEvery").unwrap_or(4.0);
    let bell_config = config.get("bell");
    let bell_step = bell_config.and_then(|b| num(b, "step"));
    for beat in 0..track.beat_count {
        for step in 0..spb {
            let abs = beat * spb + step;
            if abs >= loop_point {
                continue;
            }
            let base = cycle(&pattern, abs as usize).unwrap_or(default_base);
            let accent = f64::from(step) % accent_every == 0.0;
            let is_bell = bell && bell_step == Some(f64::from(step));
            if g.random() >= density {
                continue;
            }
            let base = if is_bell { bell_config.and_then(|b| num(b, "velocity")).unwrap_or(0.75) } else { base };
            let context =
                VelocitySpec { base: Some(base), ..VelocitySpec::accent(accent || is_bell, !accent && !is_bell) };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, 0.0, velocity);
        }
    }
}

/// `generateHatTransitionVariant`: the last beat, the last hit a ratchet.
fn hat_transition(g: &mut Generator<'_>, track: &mut Track, config: &Value, kind: &str) {
    let last_beat = (track.beat_count - num(config, "startBeatOffset").unwrap_or(1.0) as i32).max(0);
    let spb = track.steps_per_beat;
    let interval = if kind == "OHH" { 2 } else { 1 };
    let loop_point = loop_point(track, config, 1.0);
    let mut step = 0;
    while step < spb {
        if last_beat * spb + step < loop_point {
            let last = step == spb - interval;
            let velocity = g.velocity(config.get("velocity"), VelocitySpec::accent(last, !last));
            let note = add_note(track, last_beat, step, 0.0, velocity);
            if last && let Some(count) = num(config, "retriggerCount") {
                note.retrigger_count = count as i32;
                note.retrigger_rate = num(config, "retriggerRate").unwrap_or(1.0);
            }
        }
        step += interval;
    }
}

/// `generateHatRollVariant`: one beat with more and more hits per step.
fn hat_roll(g: &mut Generator<'_>, track: &mut Track, config: &Value) {
    let loop_point = loop_point(track, config, 4.0);
    let spb = track.steps_per_beat;
    let roll_beat = num(config, "rollBeat").map_or((track.beat_count - 1).max(0), |b| b as i32);
    let count = num(config, "retriggerCount").unwrap_or(4.0);
    let rate = num(config, "retriggerRate").unwrap_or(1.0);
    for step in 0..spb {
        if roll_beat * spb + step >= loop_point {
            continue;
        }
        let progress = if spb > 1 { f64::from(step) / f64::from(spb - 1) } else { 1.0 };
        let hits = ratchet(count, progress);
        let velocity = g.velocity(config.get("velocity"), VelocitySpec::accent(step == 0, step != 0));
        let note = add_note(track, roll_beat, step, 0.0, velocity);
        if hits > 1 {
            note.retrigger_count = hits;
            note.retrigger_rate = rate;
        }
    }
}

// ─── Percussion ──────────────────────────────────────────────────────────────

/// `getTrackPitchBias`: high congas and toms up, low ones down.
fn pitch_bias(track: &Track) -> f64 {
    let name = track.name.to_uppercase();
    let has = |parts: &[&str]| parts.iter().any(|p| name.contains(p));
    if has(&["HCONG", "HI_TOM", "HIGH"]) {
        5.0
    } else if has(&["LCONG", "LO_TOM", "LOW"]) {
        -5.0
    } else if has(&["CONG"]) {
        2.0
    } else {
        0.0
    }
}

/// `PercGenerator.getScaleSteps`: the scale, else pentatonic minor, else major.
fn perc_scale(g: &Generator<'_>, name: Option<&str>) -> Vec<f64> {
    let steps = name
        .and_then(|n| g.scales.get(n))
        .or_else(|| g.scales.get("pentatonic minor"))
        .or_else(|| g.scales.get("major"))
        .unwrap_or(&[0, 2, 3, 5, 7, 10]);
    steps.iter().map(|&s| f64::from(s)).collect()
}

/// `generateNewPerc`.
pub(crate) fn perc(
    track: &mut Track,
    variant: Option<&str>,
    density: f64,
    scales: &Scales,
    rnd: &mut dyn FnMut() -> f64,
) {
    let mut g = Generator::new("perc", scales, rnd);
    let name = g.resolve_variant(variant);
    let Some(config) = g.config(&name).or_else(|| g.config("basic")) else { return };
    let tones = perc_scale(&g, text(config, "scaleName"));
    let bias = pitch_bias(track);
    track.notes.clear();
    match text(config, "mode") {
        Some("lockedPerc") => perc_locked(&mut g, track, &tones, bias, config, density),
        Some("crash") => crash(&mut g, track, config),
        Some("grid") => {
            let mut pitch = |g: &mut Generator<'_>| g.random_tone(&tones) + bias;
            let opts = Grid { default_loop_beats: 2.0, pitch: Some(&mut pitch), ..Grid::default() };
            g.grid(track, config, density, opts);
        }
        Some("callResponse") => call_response(&mut g, track, &tones, bias, config, density),
        Some("fill") => perc_fill(&mut g, track, &tones, bias, config),
        _ => {
            let mut pitch = |g: &mut Generator<'_>, phrase: &Value, cached: &[f64]| {
                g.phrase_pitch(phrase, &tones, cached, bias, None)
            };
            let accent = |_: &Value, step: i32| step == 0;
            let ghost = |phrase: &Value, _: i32| text(phrase, "source") != Some("root");
            let opts = Phrases {
                default_loop_beats: 2.0,
                pitch: Some(&mut pitch),
                accent: Some(&accent),
                ghost: Some(&ghost),
                stacking: false,
            };
            g.phrases(track, config, density, opts);
        }
    }
    apply_loop_point(track, config);
}

/// `generateLockedPercVariant`: velocity and pitch patterns over 16 steps a
/// beat (on a grid clamped to 8, as for the hats); silent steps skipped.
fn perc_locked(g: &mut Generator<'_>, track: &mut Track, tones: &[f64], bias: f64, config: &Value, density: f64) {
    with_steps_per_beat(track, 16, |track| {
        let loop_point = loop_point(track, config, 1.0);
        let velocities = nums(config, "velocityPattern").unwrap_or_default();
        let pitches = nums(config, "pitchPattern").unwrap_or_default();
        let accent_every = num(config, "accentEvery").unwrap_or(4.0);
        for beat in 0..track.beat_count {
            for step in 0..16 {
                if beat * 16 + step >= loop_point {
                    continue;
                }
                let base = cycle(&velocities, step as usize).unwrap_or(0.5);
                if base <= 0.0 {
                    continue;
                }
                let accent = f64::from(step) % accent_every == 0.0;
                let degree = cycle(&pitches, step as usize).unwrap_or(0.0);
                let tone = if degree >= 0.0 && degree.fract() == 0.0 { tones.get(degree as usize) } else { None };
                let pitch = tone.copied().unwrap_or(0.0) + bias;
                if g.random() >= density {
                    continue;
                }
                let context = VelocitySpec { base: Some(base), ..VelocitySpec::accent(accent, !accent) };
                let velocity = g.velocity(config.get("velocity"), context);
                add_note(track, beat, step, pitch, velocity);
            }
        }
    });
}

/// `generatePercCallResponseVariant`: call steps on even beats, response
/// steps on odd ones, each with a chance, at random tones.
fn call_response(g: &mut Generator<'_>, track: &mut Track, tones: &[f64], bias: f64, config: &Value, density: f64) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let call = nums(config, "callSteps").unwrap_or_default();
    let response = nums(config, "responseSteps").unwrap_or_default();
    let chance = num(config, "density").unwrap_or(f64::NAN);
    for beat in 0..track.beat_count {
        let steps = if beat % 2 == 0 { &call } else { &response };
        for &step in steps {
            let step = step as i32;
            if step >= spb || g.random() >= chance * density {
                continue;
            }
            if beat * spb + step >= loop_point {
                continue;
            }
            let pitch = g.random_tone(tones) + bias;
            let context = VelocitySpec { variation: true, ..VelocitySpec::accent(step == 0, false) };
            let velocity = g.velocity(config.get("velocity"), context);
            add_note(track, beat, step, pitch, velocity);
        }
    }
}

/// `generatePercFillVariant`: a few steps of one beat walking up the scale.
fn perc_fill(g: &mut Generator<'_>, track: &mut Track, tones: &[f64], bias: f64, config: &Value) {
    let loop_point = loop_point(track, config, 2.0);
    let spb = track.steps_per_beat;
    let start = (track.beat_count - num(config, "startBeatOffset").unwrap_or(1.0) as i32).max(0);
    let steps = nums(config, "steps").unwrap_or_default();
    for (index, &step) in steps.iter().enumerate() {
        let step = step as i32;
        if step >= spb || start * spb + step >= loop_point {
            continue;
        }
        let pitch = cycle(tones, index).unwrap_or(0.0) + bias;
        let context = VelocitySpec { variation: index > 0, ..VelocitySpec::accent(step == 0, false) };
        let velocity = g.velocity(config.get("velocity"), context);
        let note = add_note(track, start, step, pitch, velocity);
        if let Some(prob) = config.get("probability").filter(|p| !p.is_null()) {
            note.probability = prob.as_f64().unwrap_or(f64::NAN);
        }
        if let Some(fill) = num(config, "euclideanPulses") {
            note.euclidean_pulses = fill as i32;
        }
    }
}

/// `generateCrashVariant`: rare hits anywhere, any pitch, anywhere in the stereo field.
fn crash(g: &mut Generator<'_>, track: &mut Track, config: &Value) {
    let loop_point = loop_point(track, config, 1.0);
    let spb = track.steps_per_beat;
    let probability = num(config, "probability").unwrap_or(0.1);
    for beat in 0..track.beat_count {
        for step in 0..spb {
            if beat * spb + step >= loop_point || g.random() >= probability {
                continue;
            }
            let pitch = (g.random() * 24.0).floor() - 12.0;
            let velocity = g.velocity(config.get("velocity"), VelocitySpec::accent(Accent::Off, false));
            let note = add_note(track, beat, step, pitch, velocity);
            note.pan = g.random() * 2.0 - 1.0;
        }
    }
}
