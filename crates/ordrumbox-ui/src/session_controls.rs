//! A MIDI controller's knobs: Edit > MIDI controls binds a control change
//! (a knob of the controller) to the master volume or to a track's volume,
//! pan, filter cutoff or mute (choose the control, press Learn, turn the
//! knob); and a knob turned then moves what it is bound to, on top of the
//! tracks' settings, as a plugin host's mix does. The bindings are a setting
//! (`midiControls`); the knobs' places are not saved, nor an undo step.

use ordrumbox_engine::{Control, ControlMap, Knob};

use super::Session;
use super::session_overlay::Overlay;
use super::session_types::OverlayKey;
use crate::scale::Scale;
use crate::{store, view};

/// The key of the bindings in `settings.json`.
const KEY: &str = "midiControls";
/// The longest a track's name is shown in a control's label.
const NAME_CHARS: usize = 10;
/// The modal's title.
const TITLE: &str = "MIDI controls";

/// A line of the modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Control,
    Learn,
    Clear,
    Close,
}

const ROWS: [Row; 4] = [Row::Control, Row::Learn, Row::Clear, Row::Close];

/// The modal's state: the controls of the pattern shown, the one chosen, and
/// whether the next control change turned is the knob to bind to it.
#[derive(Debug, Clone)]
pub(super) struct Draft {
    controls: Vec<Control>,
    names: Vec<String>,
    at: usize,
    row: usize,
    learning: bool,
}

impl Draft {
    fn selected(&self) -> Control {
        self.controls[self.at]
    }

    /// What `control` is called: "Master volume", "T3 KICK cutoff".
    fn label(&self, control: Control) -> String {
        match control.track() {
            None => {
                let what = control.what();
                format!("{}{}", what[..1].to_uppercase(), &what[1..])
            }
            Some(t) => {
                let name: String = self.names.get(t).map_or("", String::as_str).chars().take(NAME_CHARS).collect();
                format!("T{} {name} {}", t + 1, control.what())
            }
        }
    }

    /// The lines the modal shows; its width does not change with what is chosen.
    pub(super) fn lines(&self, map: &ControlMap) -> Vec<String> {
        let width = self.controls.iter().map(|&c| self.label(c).chars().count()).max().unwrap_or(0);
        let knob = match map.knob_of(self.selected()) {
            Some(Knob { channel, controller }) => format!("CC {controller}, channel {}", channel + 1),
            None => "not bound".to_owned(),
        };
        let texts = [
            format!("Control {:<width$}  {:>2}/{:>2}", self.label(self.selected()), self.at + 1, self.controls.len()),
            if self.learning { "[ Turn a knob... ]".into() } else { "[ Learn ]".to_owned() },
            "[ Clear ]".to_owned(),
            "[ Close ]".to_owned(),
        ];
        let mut lines: Vec<String> = texts
            .iter()
            .enumerate()
            .map(|(i, text)| format!("{} {text}", if i == self.row { ">" } else { " " }))
            .collect();
        lines.insert(1, format!("  Knob    {knob:<width$}", width = width + 8));
        lines.push(String::new());
        lines.push("Up Down: line   Left Right: control   Enter: run   Esc: close".into());
        lines
    }

    /// The draft's line `row` (the Knob line, which no key reaches, is not one).
    fn row_of_line(line: usize) -> Option<usize> {
        match line {
            0 => Some(0),
            2..=4 => Some(line - 1),
            _ => None,
        }
    }
}

impl Session {
    /// Edit > MIDI controls.
    pub fn open_controls(&mut self) {
        let Some(pattern) = self.pattern() else {
            self.message = "MIDI controls: no pattern".into();
            return;
        };
        let names: Vec<String> = pattern.tracks.iter().map(|t| t.name.clone()).collect();
        self.chrome.overlay = Some(Overlay::Controls(Draft {
            controls: Control::all(names.len()),
            names,
            at: 0,
            row: 0,
            learning: false,
        }));
    }

    /// The bindings of the settings file; what is wrong with them is shown at startup.
    pub(super) fn load_controls(&mut self) {
        let Some(path) = self.saving.settings_file.clone() else { return };
        // `load_master` has reported a file that cannot be read.
        if let Ok(Some(value)) = store::load_setting(&path, KEY) {
            let (map, issues) = ControlMap::from_json(&value);
            self.status.extend(issues.into_iter().map(|issue| format!("settings.json: {KEY}: {issue}")));
            self.controls = map;
        }
    }

    fn save_controls(&mut self) {
        let Some(path) = &self.saving.settings_file else {
            self.message = "MIDI controls not saved: no user data folder".into();
            return;
        };
        if let Err(e) = store::save_setting(path, KEY, self.controls.to_json()) {
            self.message = format!("MIDI controls not saved: {}: {e}", path.display());
        }
    }

    /// The live mix the knobs make, given to the engine; the mutes shown.
    fn post_controls(&mut self) {
        let mix = *self.controls.mix();
        self.playback.set_live_mix(mix);
        self.set_host_muted(mix.muted);
    }

    /// A MIDI control change: `value` (0–127) of controller `controller` on
    /// `channel`. While the modal learns, it binds the knob to the control
    /// chosen; else it moves the control the knob is bound to. True when
    /// something shown changed.
    pub fn midi_control(&mut self, channel: u8, controller: u8, value: u8) -> bool {
        let knob = Knob { channel, controller };
        if let Some(Overlay::Controls(draft)) = &mut self.chrome.overlay
            && draft.learning
        {
            draft.learning = false;
            let (control, label) = (draft.selected(), draft.label(draft.selected()));
            self.controls.bind(knob, control);
            self.message = format!("CC {controller}, channel {}: {label}", channel + 1);
            self.save_controls();
            self.post_controls();
            return true;
        }
        let muted = self.host.muted;
        if self.controls.turn(knob, value).is_none() {
            return false;
        }
        self.post_controls();
        self.host.muted != muted
    }

    /// A key while the modal is open; whether it was.
    pub(super) fn controls_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::Controls(draft)) = &mut self.chrome.overlay else { return false };
        let count = draft.controls.len();
        match key {
            OverlayKey::Escape => self.chrome.overlay = None,
            OverlayKey::Move(delta) => {
                draft.row = (draft.row as isize + delta).clamp(0, ROWS.len() as isize - 1) as usize;
                draft.learning = false;
            }
            OverlayKey::Edge(end) => draft.row = if *end { ROWS.len() - 1 } else { 0 },
            OverlayKey::Side(delta) => {
                draft.at = (draft.at as isize + delta).rem_euclid(count as isize) as usize;
                draft.learning = false;
            }
            OverlayKey::Enter => self.run_controls_row(),
            OverlayKey::Back | OverlayKey::Letter(_) => {}
        }
        true
    }

    /// Enter on the chosen line.
    fn run_controls_row(&mut self) {
        let Some(Overlay::Controls(draft)) = &mut self.chrome.overlay else { return };
        match ROWS[draft.row] {
            Row::Control => draft.at = (draft.at + 1) % draft.controls.len(),
            Row::Learn => draft.learning = !draft.learning,
            Row::Clear => {
                let (control, label) = (draft.selected(), draft.label(draft.selected()));
                draft.learning = false;
                let had_knob = self.controls.knob_of(control).is_some();
                self.controls.unbind(control);
                self.message = if had_knob { format!("{label}: knob let go") } else { format!("{label}: no knob") };
                self.save_controls();
                self.post_controls();
            }
            Row::Close => self.chrome.overlay = None,
        }
    }

    /// A click while the modal is open: outside it leaves it, on a line
    /// chooses it and runs it; whether it was open.
    pub(super) fn controls_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> bool {
        let Some(Overlay::Controls(draft)) = &mut self.chrome.overlay else { return false };
        let lines = draft.lines(&self.controls);
        let window = (width, height);
        if !view::notice_contains(window, TITLE, &lines, scale, x, y) {
            self.chrome.overlay = None;
        } else if let Some(row) = view::notice_line_at(window, TITLE, &lines, scale, x, y).and_then(Draft::row_of_line)
        {
            draft.row = row;
            draft.learning = false;
            self.run_controls_row();
        }
        true
    }

    /// The modal, drawn.
    pub(super) fn draw_controls(&self, canvas: &mut crate::canvas::Canvas, draft: &Draft, scale: Scale) {
        view::draw_notice(canvas, TITLE, &draft.lines(&self.controls), scale);
    }
}
