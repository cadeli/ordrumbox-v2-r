use super::*;
use crate::panel::{Id, Item, Rect};
use crate::scale::Scale;
use ordrumbox_core::drum_taxonomy::instrument_family;
use ordrumbox_core::json::Value;
use ordrumbox_core::{ScaleList, json};

fn scales() -> ScaleList {
    ScaleList::from_json(
        &json::parse(r#"{"major":{"scaleSteps":[0,2,4,5,7,9,11]},"minor":{"scaleSteps":[0,2,3,5,7,8,10]},"odd":{}}"#)
            .unwrap(),
    )
}

#[test]
fn arpeggios_round_trip() {
    let scales = scales();
    assert_eq!(scales.names, ["major", "minor", "odd"]);
    assert_eq!(scales.intervals("minor", 9), [0, 2, 3, 5, 7, 8, 10, 12, 14]);
    assert_eq!(scales.intervals("odd", 3), [0]);
    let mut note = Note::default();
    assert_eq!(arp_state(&note, &scales), ArpState { scale: "major".into(), direction: "up".into(), range: 0 });
    note.arp_direction = Some("down".into());
    note.arp = arp_value(&note, &scales, Some("minor"), None, Some(4));
    assert_eq!(note.arp.to_json(), r#"{"intervals":[0,2,3,5],"direction":"down"}"#);
    assert_eq!(arp_state(&note, &scales), ArpState { scale: "minor".into(), direction: "down".into(), range: 4 });
    assert_eq!(arp_value(&note, &scales, None, None, Some(0)), Value::Null);
}

#[test]
fn a_track_tooltip_names_its_whole_sound_and_its_settings() {
    let track = Track {
        name: "KICK".into(),
        mute: true,
        transpose: 2.0,
        notes: vec![Note::default(), Note::default()],
        ..Track::default()
    };
    assert_eq!(
        track_tooltip(&track, "kits/real/a very long kick name.wav"),
        [
            "KICK  mute  auto-assigned",
            "kits/real/a very long kick name.wav",
            "vol:1  pan:0  transpose:2",
            "notes:2  beats:4  steps/beat:4",
        ]
    );
    assert_eq!(track_tooltip(&Track { auto_assign: false, ..Track::default() }, "").len(), 3);
}

/// A note's tooltip on tracks transposed by 0, -2 and 7 semitones.
#[test]
fn note_tooltips() {
    let arp = |text: &str| json::parse(text).unwrap();
    let busy = Note {
        pitch: -13.0,
        probability: 0.25,
        cycle: 2,
        retrigger_count: 3,
        arp: arp(r#"{"intervals": [0, 4, 7], "mode": "up"}"#),
        retrigger_probability: 0.5,
        euclidean_pulses: 5,
        euclidean_rotation: 2,
        retrigger_rate: 12.0,
        pan: -0.333,
        ..Note::default()
    };
    let busy_text = "prob:0.25  every:2  retrig:3  arp:[0,4,7]  arpProb:0.5  eucl:5@2  rate:12  pan:-0.33";
    let rows: [(Note, [&str; 3]); 9] = [
        (Note::default(), ["C4  MIDI 60", "A#3  MIDI 58", "G4  MIDI 67"]),
        (
            Note { velocity: 0.5, pitch: 3.0, ..Note::default() },
            ["D#4  MIDI 63  vel:0.5", "C#4  MIDI 61  vel:0.5", "A#4  MIDI 70  vel:0.5"],
        ),
        (
            busy,
            [
                &format!("B2  MIDI 47  {busy_text}"),
                &format!("A2  MIDI 45  {busy_text}"),
                &format!("F#3  MIDI 54  {busy_text}"),
            ],
        ),
        (
            Note { arp: arp("[0, 7]"), ..Note::default() },
            ["C4  MIDI 60  arp:[0,7]", "A#3  MIDI 58  arp:[0,7]", "G4  MIDI 67  arp:[0,7]"],
        ),
        // One interval is no arpeggio.
        (Note { arp: arp(r#"{"intervals": [3]}"#), ..Note::default() }, ["C4  MIDI 60", "A#3  MIDI 58", "G4  MIDI 67"]),
        (
            Note { euclidean_pulses: 3, velocity: 1.0, ..Note::default() },
            ["C4  MIDI 60  vel:1  eucl:3", "A#3  MIDI 58  vel:1  eucl:3", "G4  MIDI 67  vel:1  eucl:3"],
        ),
        (Note { pitch: 30.0, ..Note::default() }, ["F#6  MIDI 90", "E6  MIDI 88", "C#7  MIDI 97"]),
        (
            Note { pitch: -60.0, velocity: 0.123_456, ..Note::default() },
            ["C-1  MIDI 0  vel:0.12", "A#-2  MIDI -2  vel:0.12", "G-1  MIDI 7  vel:0.12"],
        ),
        // A fractional pitch has no name.
        (Note { pitch: 0.5, ..Note::default() }, ["?4  MIDI 60.5", "?3  MIDI 58.5", "?4  MIDI 67.5"]),
    ];
    for (note, texts) in &rows {
        for (track_pitch, text) in [0.0, -2.0, 7.0].into_iter().zip(texts) {
            assert_eq!(note_tooltip(note, track_pitch), *text, "{note:?} on {track_pitch}");
        }
    }
}

#[test]
fn locale_order() {
    let mut names = vec!["b", "B", "a10", "a2", "_x", "A"];
    names.sort_by(|a, b| locale_cmp(a, b));
    assert_eq!(names, ["_x", "A", "a10", "a2", "b", "B"]);
}

#[test]
fn loop_point_falls_back_to_the_length() {
    let mut track = Track { beat_count: 4, steps_per_beat: 4, ..Track::default() };
    assert_eq!(loop_length(&track), 16.0);
    track.notes.push(Note { beat: 7, step: 1, ..Note::default() });
    assert_eq!(loop_length(&track), 30.0);
    track.loop_length = Some(5);
    assert_eq!(loop_length(&track), 5.0);
}

fn settings(hits: f64) -> ordrumbox_logic::compose::Settings {
    ordrumbox_logic::compose::Settings {
        hits_per_measure: hits,
        complexity: 5.0,
        // As the test track sets it.
        syncopation: 70.0,
        coupling: 0.0,
        variation: 5.0,
        style_weight: 100.0,
    }
}

fn view(track: &Track) -> TrackView<'_> {
    TrackView {
        shape: Shape::default(),
        track,
        title: "Track: KICK - real/kick".into(),
        sample: Some(SampleBar {
            info: "C4 - 210 ms - -0.3 dB".into(),
            envelope: vec![0.5; 8],
            overlay: Overlay::new(&Shape { duration_ms: 210.0, end_ms: 210.0, decay_ms: 100.0, ..Shape::default() }),
        }),
        decay: 0.0,
        instruments: Choice { values: vec!["KICK".into()], labels: Vec::new(), selected: 0 },
        sidechains: Choice { values: vec![String::new()], labels: vec!["none".into()], selected: 0 },
        samples: Choice::default(),
        synths: Choice { values: vec!["none".into()], labels: Vec::new(), selected: 0 },
        lfo: Vec::new(),
        bpm: 120.0,
        compose: Some(ComposeView {
            style: "house".into(),
            shown: ordrumbox_logic::compose::TrackCompose {
                part: "KICK".into(),
                used: settings(6.0),
                base: settings(4.0),
                open_hits: 16,
            },
        }),
    }
}

#[test]
fn the_rows_stay_put_without_a_sample() {
    let track = Track { name: "BASS".into(), ..Track::default() };
    let tab_rect = |sample: bool| {
        let mut view = view(&track);
        if !sample {
            view.sample = None;
        }
        let TrackEditor { items, bottom, .. } =
            track_editor(&view, &EditorState::default(), Rect::new(0, 0, 360, 600), Scale::ONE);
        (items.into_iter().find(|i| i.id == Some(Id::Tab(0))).map(|i| i.rect), bottom)
    };
    assert_eq!(tab_rect(true), tab_rect(false));
}

#[test]
fn every_tab_builds() {
    let mut track = Track { name: "KICK".into(), ..Track::default() };
    for auto in [false, true] {
        track.auto_mode = auto;
        for tab in 0..5 {
            for sub in 0..6 {
                let state = EditorState {
                    tab,
                    generation_tab: sub % 3,
                    generation_advanced: sub >= 3,
                    ..EditorState::default()
                };
                let TrackEditor { items, bottom, .. } =
                    track_editor(&view(&track), &state, Rect::new(700, 30, 360, 600), Scale::ONE);
                assert!(bottom > 30 && items.iter().any(|i| i.id == Some(Id::Decay)));
            }
        }
    }
    // Folded: the simple view only.
    let state = EditorState { tab: 4, ..EditorState::default() };
    let TrackEditor { items, .. } = track_editor(&view(&track), &state, Rect::new(0, 0, 360, 600), Scale::ONE);
    assert!(items.iter().any(|i| i.id == Some(Id::Toggle("composeLock"))));
    assert!(items.iter().any(|i| i.id == Some(Id::GenerationAdvanced)));
    assert!(!items.iter().any(|i| i.id == Some(Id::Generation("autoPitchChance"))));
    let state = EditorState { generation_advanced: true, ..state };
    let TrackEditor { items, .. } = track_editor(&view(&track), &state, Rect::new(0, 0, 360, 600), Scale::ONE);
    assert!(items.iter().any(|i| i.id == Some(Id::Generation("autoPitchChance"))));
    assert!(!items.iter().any(|i| i.id == Some(Id::Generation("hitVariation"))));
}

#[test]
fn the_engine_tab_shows_the_amounts_and_the_variants_a_track_can_use() {
    let ids = |track: &Track, tab: u8| {
        let state = EditorState { tab: 4, generation_tab: tab, generation_advanced: true, ..EditorState::default() };
        let mut view = view(track);
        if instrument_family(&track.name).is_melodic() {
            view.compose = None;
        }
        track_editor(&view, &state, Rect::new(0, 0, 360, 600), Scale::ONE).items
    };
    let kick = Track { name: "KICK".into(), auto_mode: true, auto_hit_variation: Some(20.0), ..Track::default() };
    let groove = ids(&kick, 0);
    let hits = groove.iter().find(|i| i.id == Some(Id::Generation("autoHitVariation"))).unwrap();
    assert!(matches!(&hits.kind, Kind::Value { value, .. } if *value == 20.0));
    // A drum line: no variant, no density; the note amount unset shows the one used.
    let engine = ids(&kick, 1);
    assert!(!engine.iter().any(|i| i.id == Some(Id::TrackSelect("autoVariant"))));
    assert!(!engine.iter().any(|i| i.id == Some(Id::Generation("autoDensity"))));
    let notes = engine.iter().find(|i| i.id == Some(Id::Generation("autoNoteVariation"))).unwrap();
    assert!(matches!(&notes.kind, Kind::Value { value, .. } if *value == kick.auto_note_amount()));
    // A bass line: its own variants, in the simple view.
    let bass = Track { name: "BASS".into(), auto_mode: true, ..Track::default() };
    assert!(ids(&bass, 1).iter().any(|i| i.id == Some(Id::TrackSelect("autoVariant"))));
    assert!(ids(&bass, 1).iter().any(|i| i.id == Some(Id::Generation("autoDensity"))));
    let variants = variant_options(&bass);
    assert!(variants.contains(&"groove".to_owned()) && !variants.contains(&"chordStab".to_owned()));
    assert!(variant_options(&Track { name: "PIANO".into(), ..Track::default() }).contains(&"chordStab".to_owned()));
}

#[test]
fn the_compose_tab_shows_the_style_values_until_set() {
    let mut track = Track { name: "KICK".into(), ..Track::default() };
    track.compose_syncopation = Some(70.0);
    let state =
        EditorState { tab: 4, generation_tab: COMPOSE_TAB, generation_advanced: true, ..EditorState::default() };
    let TrackEditor { items, .. } = track_editor(&view(&track), &state, Rect::new(0, 0, 360, 600), Scale::ONE);
    let shown = |key: &'static str| {
        items.iter().find(|i| i.id == Some(Id::Compose(key))).and_then(|i| match &i.kind {
            Kind::Value { value, text, spec, .. } => Some((*value, text.clone(), spec.default)),
            _ => None,
        })
    };
    // The pattern's energy makes the 4 hits of the style 6.
    assert_eq!(shown("composeHitsPerMeasure"), Some((4.0, "4\u{2192}6*".to_owned(), STYLE_DEFAULT)));
    assert_eq!(shown("composeSyncopation"), Some((70.0, "70".to_owned(), STYLE_DEFAULT)));
    assert_eq!(shown("composeSeed"), Some((0.0, "new".to_owned(), 0.0)));
    assert!(items.iter().any(|i| i.id == Some(Id::Regenerate)));
    // A melodic track has no Compose tab: the manual sliders show.
    let mut bass = view(&track);
    bass.compose = None;
    let TrackEditor { items, .. } = track_editor(&bass, &state, Rect::new(0, 0, 360, 600), Scale::ONE);
    assert!(items.iter().any(|i| i.id == Some(Id::Generation("hitVariation"))));
}

#[test]
fn note_editor_hides_the_euclidean_prob() {
    let view = NoteView {
        title: "KICK [beat 1 step 1]".into(),
        note: Some(Note::default()),
        scales: Choice::default(),
        arp_direction: Choice::default(),
        arp_range: 0,
    };
    let state = EditorState { note_tab: 2, ..EditorState::default() };
    let items = note_editor(&view, &state, Rect::new(0, 0, 360, 300), Scale::ONE);
    let sliders: Vec<_> = items
        .iter()
        .filter_map(|i| match i.id {
            Some(Id::NoteSlider(k)) => Some(k),
            _ => None,
        })
        .collect();
    assert_eq!(sliders, ["euclideanPulses", "euclideanRotation"]);
}

#[test]
fn the_fx_tab_shows_every_effect_and_moves_nothing_when_one_goes_on() {
    let area = Rect::new(700, 30, 360, 600);
    let state = EditorState { tab: TAB_FX, ..EditorState::default() };
    let off = Track { name: "KICK".into(), filter_type: "allpass".into(), ..Track::default() };
    let on = Track {
        name: "KICK".into(),
        reverb_mix: 0.5,
        delay_mix: 0.5,
        delay_type: "slap".into(),
        saturation_mix: 0.5,
        comp_mix: 1.0,
        duck_amount: 0.5,
        sidechain_source: "SNARE".into(),
        filter_type: "lowpass".into(),
        ..Track::default()
    };
    let mut places = Vec::new();
    for (track, lit) in [(&off, false), (&on, true)] {
        let TrackEditor { items, bottom, .. } = track_editor(&view(track), &state, area, Scale::ONE);
        assert_put(&mut places, &items, &format!("effects on {lit}"));
        for i in 0..FX_DEFS.len() as u8 {
            let power = items.iter().find(|item| item.id == Some(Id::FxPower(i))).map(|item| &item.kind);
            assert!(matches!(power, Some(Kind::Button { on, .. }) if *on == lit), "effect {i} on {lit}");
        }
        // Every effect value is a knob of the knob bar's size, under the tab.
        let knobs = items.iter().filter(|item| matches!(item.id, Some(Id::Fx(_))));
        for item in knobs {
            assert_eq!(item.rect.h, knob_row_h(Scale::ONE), "{:?}", item.id);
            assert!(item.rect.y + item.rect.h <= bottom, "{:?}", item.id);
        }
    }
}

#[test]
fn the_lfo_tab_shows_every_target_and_moves_nothing_when_one_goes_on() {
    let area = Rect::new(700, 30, 360, 600);
    let state = EditorState { tab: TAB_LFO, ..EditorState::default() };
    let off = Track { name: "KICK".into(), ..Track::default() };
    let lfo = ordrumbox_core::json::parse(r#"{"type":"square","freq":1.5,"min":0.2,"max":0.8,"phase":0}"#).unwrap();
    let on = Track {
        name: "KICK".into(),
        filter_cutoff_lfo: lfo.as_object().cloned(),
        filter_q_lfo: lfo.as_object().cloned(),
        volume_lfo: lfo.as_object().cloned(),
        pan_lfo: lfo.as_object().cloned(),
        transpose_lfo: lfo.as_object().cloned(),
        ..Track::default()
    };
    let mut places = Vec::new();
    for (track, lit) in [(&off, false), (&on, true)] {
        let TrackEditor { items, bottom, .. } = track_editor(&view(track), &state, area, Scale::ONE);
        assert_put(&mut places, &items, &format!("LFOs on {lit}"));
        for i in 0..LFO_TARGETS.len() as u8 {
            let power = items.iter().find(|item| item.id == Some(Id::LfoPower(i))).map(|item| &item.kind);
            assert!(matches!(power, Some(Kind::Button { on, .. }) if *on == lit), "LFO {i} on {lit}");
            assert!(items.iter().any(|item| item.id == Some(Id::LfoType(i))), "LFO {i} wave");
        }
        // Every LFO value is a knob of the knob bar's size, under the tab.
        let knobs: Vec<_> = items.iter().filter(|item| matches!(item.id, Some(Id::Lfo(..)))).collect();
        assert_eq!(knobs.len(), LFO_TARGETS.len() * 4);
        for item in knobs {
            assert_eq!(item.rect.h, knob_row_h(Scale::ONE), "{:?}", item.id);
            assert!(item.rect.y + item.rect.h <= bottom, "{:?}", item.id);
        }
    }
}

/// Records the place of every control of `items` (the nth of an id
/// shown twice: the compose sliders); one that has had another place in an
/// earlier layout fails.
fn assert_put(places: &mut Vec<(Id, usize, Rect)>, items: &[Item], state: &str) {
    for (i, item) in items.iter().enumerate() {
        let Some(id) = item.id else { continue };
        let nth = items[..i].iter().filter(|other| other.id == Some(id)).count();
        match places.iter().find(|(seen, n, _)| *seen == id && *n == nth) {
            Some((_, _, rect)) => assert_eq!(*rect, item.rect, "{id:?} moved: {state}"),
            None => places.push((id, nth, item.rect)),
        }
    }
}

#[test]
fn no_control_moves_whatever_the_tab_and_the_settings() {
    let area = Rect::new(700, 30, 360, 600);
    let lfo = ordrumbox_core::json::parse(r#"{"freq":1.5,"min":20,"max":19999.5}"#).unwrap();
    let mut places = Vec::new();
    let mut bottoms = Vec::new();
    for (name, compose, sample) in [("KICK", true, true), ("KICK", false, false), ("BASS", false, true)] {
        for (auto, slap, with_lfo) in (0..8).map(|bits: u8| (bits & 1 == 1, bits & 2 == 2, bits & 4 == 4)) {
            let track = Track {
                name: name.into(),
                auto_mode: auto,
                delay_type: if slap { "slap" } else { "tape" }.into(),
                filter_cutoff_lfo: with_lfo.then(|| lfo.as_object().cloned()).flatten(),
                ..Track::default()
            };
            let mut view = view(&track);
            if !compose {
                view.compose = None;
            }
            if !sample {
                view.sample = None;
            }
            for (tab, sub, advanced, selected) in (0..5u8).flat_map(|tab| {
                (0..5u8).flat_map(move |sub| {
                    [false, true].into_iter().flat_map(move |a| [None, Some("autoMode")].map(|p| (tab, sub, a, p)))
                })
            }) {
                let state = EditorState {
                    tab,
                    generation_tab: sub % 3,
                    generation_advanced: advanced,
                    selected_prop: selected,
                    ..EditorState::default()
                };
                let TrackEditor { items, bottom, .. } = track_editor(&view, &state, area, Scale::ONE);
                let shown = format!("{name} auto {auto} slap {slap} lfo {with_lfo} compose {compose} {state:?}");
                assert_put(&mut places, &items, &shown);
                // Nothing under the track editor's tab moves either.
                let content = items.iter().filter(|i| i.rect.y > area.y + 120).map(|i| i.rect.y + i.rect.h);
                assert!(content.max().is_some_and(|y| y <= bottom), "{shown}");
                bottoms.push(bottom);
            }
        }
    }
    // The note editor starts at one y whatever the tab: under the tallest, and no lower.
    assert!(bottoms.windows(2).all(|w| w[0] == w[1]));
    let tallest = places.iter().map(|(_, _, r)| r.y + r.h).max().unwrap();
    assert!(bottoms[0] - tallest < row_h(Scale::ONE) + 2, "{} under {tallest}", bottoms[0]);

    let mut places = Vec::new();
    for note in [Some(Note::default()), None] {
        for note_tab in 0..NOTE_TABS.len() as u8 {
            let view = NoteView {
                title: "KICK [beat 1 step 1]".into(),
                note: note.clone(),
                scales: Choice { values: vec!["major".into()], labels: Vec::new(), selected: 0 },
                arp_direction: Choice { values: vec!["up".into()], labels: Vec::new(), selected: 0 },
                arp_range: 2,
            };
            let state = EditorState { note_tab, ..EditorState::default() };
            let items = note_editor(&view, &state, Rect::new(700, 400, 360, 200), Scale::ONE);
            assert_put(&mut places, &items, &format!("note tab {note_tab}"));
        }
    }
}
