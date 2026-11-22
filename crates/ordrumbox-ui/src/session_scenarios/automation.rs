//! The song's automation lanes while it plays: shown by the mixer, written
//! from it, taken back by it.

use super::super::Session;
use super::super::session_chrome::Drag;
use super::{LIBRARY, run, session};
use crate::control::Spec;
use crate::menu::Command;
use crate::panel::Id;
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
    s.show_mode(true);
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

#[test]
fn a_control_moved_over_its_lane_takes_it_back_until_re_enabled() {
    let (mut s, mut audio) = session(LIBRARY);
    s.show_mode(true);
    s.editor.add_automation_lane(None, "KICK", AutomationTarget::Volume).unwrap();
    let base = s.song().unwrap().automations[0].clone();
    let stroke = ordrumbox_core::AutomationStroke::Fill(Some(0.5));
    s.editor.paint_automation(None, 0, &base, (0, 1000), stroke, "a").unwrap();
    s.toggle_play();
    run(&mut s, &mut audio, 2 * STEP);
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
