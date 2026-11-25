//! The song's automation lanes while it plays: shown by the mixer, written
//! from it, taken back by it.

use super::super::Session;
use super::super::session_chrome::Drag;
use super::{LIBRARY, run, session};
use crate::control::Spec;
use crate::menu::Command;
use crate::panel::Id;
use crate::session::session_types::PlayMode;
use ordrumbox_core::automation::{AutomationTarget, volume_level};
use ordrumbox_core::steps::TICK;

/// Frames of a song step at 120 BPM.
const STEP: usize = 1000;

fn lane_points(s: &Session, target: AutomationTarget) -> Vec<(u32, Option<f64>)> {
    let lanes = &s.song().unwrap().automations;
    let lane = lanes.iter().find(|lane| lane.is_for("KICK", target)).expect("a lane");
    lane.points().iter().map(|p| (p.step, p.value)).collect()
}

fn step(s: &Session) -> u32 {
    s.song().unwrap().automation_step_at(s.playback.tick().expect("playing"), TICK)
}

#[test]
fn the_mixer_writes_the_lanes_while_recording_and_shows_them() {
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(PlayMode::Song);
    s.run(Command::RecordAutomation);
    s.toggle_play();
    run(&mut s, &mut audio, 4 * STEP);

    // The fader held: its level written step after step, the track left as it is.
    let spec = Spec::fader(0.0, 1.0, 0.01, 1.0);
    let drag = Drag { id: Id::TrackVolume(0), spec, start: 1.0, x: 0, y: 0, bar: None, fine: false };
    s.chrome.widgets.drag = Some(drag);
    s.set_value(Id::TrackVolume(0), 0.5);
    let first = step(&s);
    run(&mut s, &mut audio, 3 * STEP);
    s.chrome.widgets.drag = None;
    run(&mut s, &mut audio, 2 * STEP);
    let level = (volume_level(0.5) * 1000.0).round() / 1000.0;
    let points = lane_points(&s, AutomationTarget::Volume);
    assert_eq!((points.len(), points[0], points[1].1), (2, (first, Some(level)), None), "{points:?}");
    assert!(points[1].0 >= first + 3, "written while held: {points:?}");
    assert_eq!(s.track(0).unwrap().volume, 1.0, "the track is not edited");
    s.undo();
    assert!(lane_points(&s, AutomationTarget::Volume).is_empty(), "the gesture, one undo step");

    // A mute clicked: on from the playhead to the end of the song.
    s.mixer_activate(Id::MixerToggle(0, "mute"));
    let at = lane_points(&s, AutomationTarget::Mute);
    assert_eq!((at[0].1, at[1]), (Some(1.0), (64, None)), "on up to the song's end: {at:?}");
    assert!(!s.track(0).unwrap().mute);
    // The mixer shows it.
    run(&mut s, &mut audio, STEP);
    assert_eq!(s.automated_now(0, AutomationTarget::Mute), Some(1.0));
}

/// The song playing, two steps in, with KICK's volume lane at half all along.
fn playing_with_a_volume_lane() -> (Session, ordrumbox_engine::AudioSide) {
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(PlayMode::Song);
    s.editor.add_automation_lane(None, "KICK", AutomationTarget::Volume).unwrap();
    let base = s.song().unwrap().automations[0].clone();
    let stroke = ordrumbox_core::AutomationStroke::Fill(Some(0.5));
    s.editor.paint_automation(None, 0, &base, (0, 1000), stroke, "a").unwrap();
    s.toggle_play();
    run(&mut s, &mut audio, 2 * STEP);
    (s, audio)
}

#[test]
fn a_control_moved_over_its_lane_takes_it_back_until_re_enabled() {
    let (mut s, _audio) = playing_with_a_volume_lane();
    let lane_gain = ordrumbox_core::automation::volume_gain(0.5);
    assert_eq!(s.automated_now(0, AutomationTarget::Volume), Some(lane_gain), "the fader shows its lane");

    s.set_value(Id::TrackVolume(0), 0.8);
    assert_eq!(s.track(0).unwrap().volume, 0.8, "the track's own volume set");
    assert_eq!(s.automated_now(0, AutomationTarget::Volume), None, "its lane taken back");
    assert!(s.message.contains("taken back"), "{}", s.message);
    assert!(s.menu_state().has_overridden);

    s.run(Command::ReenableAutomation);
    assert_eq!(s.automated_now(0, AutomationTarget::Volume), Some(lane_gain));
    assert_eq!(s.message, "its lane plays again");
}

#[test]
fn a_lane_taken_back_is_shown_and_re_enabled_on_its_own() {
    use crate::panel::Kind;
    use crate::session::session_song::tests::{find_hit, menu};
    use crate::song_view::SongHit;
    use crate::test_support::SIZE;
    use crate::theme::TAKEN_BACK;
    let (mut s, _audio) = playing_with_a_volume_lane();
    let marks = |s: &Session| {
        let items = s.panel_layout(SIZE.0, SIZE.1, SIZE.2).items;
        items.iter().filter(|item| item.kind == Kind::Mark { color: TAKEN_BACK }).count()
    };
    s.run(Command::Mixer);
    assert_eq!((marks(&s), s.taken_back_lanes()), (0, vec![false]), "nothing taken back");

    s.set_value(Id::TrackVolume(0), 0.8);
    // The fader marked, and the track editor's Volume knob beside it; in the
    // grid, its slider and the knob; the lane too.
    assert_eq!(marks(&s), 2);
    s.run(Command::Grid);
    assert_eq!(marks(&s), 2);
    assert_eq!(s.taken_back_lanes(), [true]);
    assert!(s.message.contains("its badge"), "{}", s.message);

    // The lane's menu offers to re-enable it; its badge does.
    s.run(Command::SongView);
    let (x, y) = find_hit(&s, |hit| hit == SongHit::LaneName(0));
    s.song_click(SIZE, x, y, crate::ui_input::Button::Right);
    assert_eq!(menu(&s).first().map(String::as_str), Some("reenable:0"));
    s.chrome.widgets.dropdown = None;
    let (x, y) = find_hit(&s, |hit| hit == SongHit::LaneBadge(0));
    s.song_click(SIZE, x, y, crate::ui_input::Button::Left);
    assert_eq!(s.taken_back_lanes(), [false]);
    assert_eq!(s.message, "KICK Volume plays its lane again");
    assert!(!s.menu_state().has_overridden);
    let (x, y) = find_hit(&s, |hit| hit == SongHit::LaneName(0));
    s.song_click(SIZE, x, y, crate::ui_input::Button::Right);
    assert_eq!(menu(&s).first().map(String::as_str), Some("taller:0"), "nothing to re-enable");
}

#[test]
fn the_lanes_taken_back_are_forgotten_with_their_song() {
    let (mut s, mut audio) = playing_with_a_volume_lane();
    s.set_value(Id::TrackVolume(0), 0.8);
    assert!(s.menu_state().has_overridden);

    let first = s.editor.selected_song();
    let other = s.editor.add_song("other");
    s.editor.set_selected_song(other);
    run(&mut s, &mut audio, STEP);
    s.editor.set_selected_song(first);
    run(&mut s, &mut audio, STEP);
    assert!(!s.menu_state().has_overridden, "not taken back again by selecting its song");
}

#[test]
fn the_grid_and_the_track_editor_show_the_lanes_too() {
    use crate::panel::Kind;
    use crate::test_support::SIZE;
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(PlayMode::Song);
    for (target, value) in [(AutomationTarget::Volume, 0.5), (AutomationTarget::Mute, 1.0)] {
        let lane = s.editor.add_automation_lane(None, "KICK", target).unwrap();
        let base = s.song().unwrap().automations[lane].clone();
        let stroke = ordrumbox_core::AutomationStroke::Fill(Some(value));
        s.editor.paint_automation(None, lane, &base, (0, 1000), stroke, "a").unwrap();
    }
    assert!(s.shown_switches().is_empty(), "stopped: the tracks' own");
    s.toggle_play();
    run(&mut s, &mut audio, 2 * STEP);
    let gain = ordrumbox_core::automation::volume_gain(0.5);

    // The grid's slider, moved as by an LFO; its M lit.
    s.run(Command::Grid);
    let slider = s.panel_layout(SIZE.0, SIZE.1, SIZE.2).items.into_iter().find_map(|item| match (item.id, item.kind) {
        (Some(Id::TrackVolume(0)), Kind::Value { value, lfo, .. }) => Some((value, lfo)),
        _ => None,
    });
    assert_eq!(slider, Some((gain, true)));
    assert_eq!(s.shown_switches(), [(true, false)]);
    // The track editor's knob.
    assert!(s.track_view(0).unwrap().lfo.contains(&("volume", gain)));

    // Its knob moved: the lane taken back, as from the mixer.
    s.set_value(Id::KnobBar("volume"), 0.7);
    assert_eq!((s.track(0).unwrap().volume, s.automated_now(0, AutomationTarget::Volume)), (0.7, None));
    // M in the grid: the mute lane taken back, the track left playing.
    s.switch_track(0, crate::session::Switch::Mute);
    assert_eq!((s.shown_switches(), s.track(0).unwrap().mute), (vec![(false, false)], false));
}

#[test]
fn pan_cutoff_and_q_knobs_write_show_and_take_back_their_lanes() {
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(PlayMode::Song);
    s.run(Command::RecordAutomation);
    s.toggle_play();
    run(&mut s, &mut audio, 4 * STEP);

    // The mixer's pan knob held: its lane written, left, while held.
    let pan = s.track(0).unwrap().pan;
    let drag = Drag {
        id: Id::MixerKnob(0, "pan"),
        spec: Spec::knob(-1.0, 1.0, 0.01, 0.0),
        start: 0.0,
        x: 0,
        y: 0,
        bar: None,
        fine: false,
    };
    s.chrome.widgets.drag = Some(drag);
    s.set_value(Id::MixerKnob(0, "pan"), -1.0);
    let first = step(&s);
    run(&mut s, &mut audio, 2 * STEP);
    s.chrome.widgets.drag = None;
    let points = lane_points(&s, AutomationTarget::Pan);
    assert_eq!(points[0], (first, Some(0.0)), "{points:?}");
    assert!(points[1].0 >= first + 2, "written while held: {points:?}");
    assert_eq!(s.track(0).unwrap().pan, pan, "the track is not edited");

    // The track editor's cutoff knob: a lane in hertz.
    s.set_value(Id::Fx("filterCutoff"), 632.456);
    let cutoff = lane_points(&s, AutomationTarget::Cutoff);
    assert_eq!(cutoff[0].1, Some(0.5), "{cutoff:?}");
    // Moved, not held: written on the playhead's step, which the knob shows.
    let shown = s.track_view(0).unwrap().lfo.iter().find(|(key, _)| *key == "filterCutoff").map(|&(_, hz)| hz);
    assert!(shown.is_some_and(|hz| (hz - 632.456).abs() < 1.0), "the knob shows its lane: {shown:?}");

    // Not recording: the knob takes its lane back, the track takes the value.
    s.run(Command::RecordAutomation);
    s.set_value(Id::Fx("filterCutoff"), 1000.0);
    assert_eq!((s.track(0).unwrap().filter_cutoff, s.automated_now(0, AutomationTarget::Cutoff)), (1000.0, None));
    assert!(s.message.contains("Cutoff taken back"), "{}", s.message);
}

#[test]
fn send_and_effect_knobs_write_show_and_take_back_their_lanes() {
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(PlayMode::Song);
    s.run(Command::RecordAutomation);
    s.toggle_play();
    run(&mut s, &mut audio, 4 * STEP);

    // The mixer's echo send: its lane written, the track left, the knob showing it.
    let send = s.track(0).unwrap().echo_send;
    s.set_value(Id::MixerKnob(0, "echoSend"), 0.8);
    assert_eq!(lane_points(&s, AutomationTarget::EchoSend)[0].1, Some(0.8));
    assert_eq!(s.track(0).unwrap().echo_send, send, "the track is not edited");
    assert_eq!(s.automated_now(0, AutomationTarget::EchoSend), Some(0.8));

    // The track editor's saturation knob, its field `saturationMix`.
    s.set_value(Id::Fx("saturationMix"), 0.4);
    assert_eq!(lane_points(&s, AutomationTarget::Saturation)[0].1, Some(0.4));
    assert!(s.track_view(0).unwrap().lfo.contains(&("saturationMix", 0.4)), "the knob shows its lane");

    // Not recording: the knob takes its lane back.
    s.run(Command::RecordAutomation);
    s.set_value(Id::Fx("saturationMix"), 0.2);
    let track = s.track(0).unwrap();
    assert_eq!((track.saturation_mix, s.automated_now(0, AutomationTarget::Saturation)), (0.2, None));
    assert!(s.message.contains("Saturation taken back"), "{}", s.message);
}

#[test]
fn transpose_probability_and_swing_controls_write_and_show_their_lanes() {
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(PlayMode::Song);
    s.run(Command::RecordAutomation);
    s.toggle_play();
    run(&mut s, &mut audio, 4 * STEP);

    // The knob bar's transpose, in whole semitones.
    s.set_value(Id::KnobBar("transpose"), 7.0);
    let transpose = AutomationTarget::Transpose;
    let value = lane_points(&s, transpose)[0].1.expect("a value");
    assert_eq!(transpose.to_control(value), 7.0, "kept to a thousandth, played as +7");
    assert!(s.track_view(0).unwrap().lfo.contains(&("transpose", 7.0)), "the knob shows its lane");
    // The generation tab's probability and the loop tab's swing.
    s.set_value(Id::Generation("probability"), 0.25);
    s.set_value(Id::Loop("swingAmount"), 0.5);
    assert_eq!(lane_points(&s, AutomationTarget::Probability)[0].1, Some(0.25));
    assert_eq!(lane_points(&s, AutomationTarget::Swing)[0].1, Some(0.5));
    let track = s.track(0).unwrap();
    assert_eq!((track.transpose, track.probability, track.swing_amount), (0.0, 1.0, 0.0), "the track is not edited");
    let lfo = &s.track_view(0).unwrap().lfo;
    assert!(lfo.contains(&("probability", 0.25)) && lfo.contains(&("swingAmount", 0.5)));
}
