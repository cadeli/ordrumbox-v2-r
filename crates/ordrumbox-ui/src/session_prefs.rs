//! Edit > Preferences: one modal that gathers what the user chooses: the
//! sound output, the MIDI input, the metronome, the VU meters and the toasts.
//! The choices are kept in a draft; OK applies them, Esc leaves them all. The
//! sound output and the MIDI input are settings (`audioDevice`, `midiInput`):
//! a new sound output starts the sound again, which reloads the session as
//! File > Change user data folder does; a new MIDI input is opened by the program.

use ordrumbox_core::json::Value;

use super::Session;
use super::session_overlay::Overlay;
use super::session_types::OverlayKey;
use super::session_user_data::NO_USER_FOLDER;
use crate::audio_output::{DEVICE_KEY, chosen_device};
use crate::midi_settings::{
    CLOCK_IN_KEY, CLOCK_OUT_KEY, MIDI_KEY, MIDI_OUT_KEY, MidiPort, chosen_clock_in, chosen_clock_out,
    chosen_midi_input, chosen_midi_output,
};
use crate::scale::Scale;
use crate::{store, view};

/// The longest a device's name is shown (the modal keeps its width).
const NAME_CHARS: usize = 36;

/// A line of the modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Audio,
    Midi,
    ClockIn,
    MidiOut,
    ClockOut,
    Metronome,
    Vus,
    Toasts,
    Ok,
}

/// A setting with a list of values: each is its text in the settings file and its label.
#[derive(Debug, Clone)]
struct Choice {
    options: Vec<(String, String)>,
    at: usize,
    /// The option chosen when the modal opened.
    was: usize,
}

impl Choice {
    /// `options`, `current` (its setting's text) among them: added at the
    /// end, as not connected, when the list has no such device.
    fn new(mut options: Vec<(String, String)>, current: &str) -> Self {
        let at = options.iter().position(|(value, _)| value == current).unwrap_or_else(|| {
            options.push((current.to_owned(), format!("{current} (not connected)")));
            options.len() - 1
        });
        Self { options, at, was: at }
    }

    fn changed(&self) -> bool {
        self.at != self.was
    }

    fn value(&self) -> &str {
        &self.options[self.at].0
    }

    fn label_width(&self) -> usize {
        self.options.iter().map(|(_, label)| label.chars().count().min(NAME_CHARS)).max().unwrap_or(0)
    }
}

/// The modal's draft.
#[derive(Debug, Clone)]
pub(super) struct Prefs {
    rows: Vec<Row>,
    row: usize,
    audio: Choice,
    midi: Choice,
    midi_out: Choice,
    clock_in: bool,
    /// The clock settings when the modal opened.
    clock_in_was: bool,
    clock_out: bool,
    clock_out_was: bool,
    metronome: bool,
    vus: bool,
    toasts: bool,
}

impl Prefs {
    fn text(&self, row: Row) -> (&'static str, String) {
        let on = |on: bool| if on { "on" } else { "off" }.to_owned();
        let choice = |c: &Choice| {
            let label: String = c.options[c.at].1.chars().take(NAME_CHARS).collect();
            format!("{label:<width$}", width = c.label_width())
        };
        match row {
            Row::Audio => ("Audio output", choice(&self.audio)),
            Row::Midi => ("MIDI input", choice(&self.midi)),
            Row::ClockIn => ("MIDI clock in", if self.clock_in { "follow" } else { "ignore" }.to_owned()),
            Row::MidiOut => ("MIDI output", choice(&self.midi_out)),
            Row::ClockOut => ("MIDI clock out", on(self.clock_out)),
            Row::Metronome => ("Metronome", on(self.metronome)),
            Row::Vus => ("VU meters", on(self.vus)),
            Row::Toasts => ("Toasts", on(self.toasts)),
            Row::Ok => ("OK", String::new()),
        }
    }

    /// The lines the modal shows: a row each, the one chosen marked.
    pub(super) fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, &row)| {
                let mark = if i == self.row { ">" } else { " " };
                match self.text(row) {
                    (label, value) if row == Row::Ok => format!("{mark} [ {label} ]{value}"),
                    (label, value) => format!("{mark} {label:<15} {value}"),
                }
            })
            .collect();
        lines.push(String::new());
        lines.push("Up Down: line   Enter, Left Right: change   Esc: leave as it was".into());
        lines
    }

    fn change(&mut self, delta: isize) {
        let wrap = |at: usize, len: usize| (at as isize + delta).rem_euclid(len as isize) as usize;
        match self.rows[self.row] {
            Row::Audio => self.audio.at = wrap(self.audio.at, self.audio.options.len()),
            Row::Midi => self.midi.at = wrap(self.midi.at, self.midi.options.len()),
            Row::ClockIn => self.clock_in = !self.clock_in,
            Row::MidiOut => self.midi_out.at = wrap(self.midi_out.at, self.midi_out.options.len()),
            Row::ClockOut => self.clock_out = !self.clock_out,
            Row::Metronome => self.metronome = !self.metronome,
            Row::Vus => self.vus = !self.vus,
            Row::Toasts => self.toasts = !self.toasts,
            Row::Ok => {}
        }
    }
}

impl Session {
    /// Edit > Preferences.
    pub fn open_preferences(&mut self) {
        let offers = self.playback.offers();
        let chosen = (|| {
            Ok::<_, String>((
                self.audio_choice()?,
                self.midi_choice()?,
                self.midi_out_choice()?,
                chosen_clock_in()?,
                chosen_clock_out()?,
            ))
        })();
        let (audio, midi, midi_out, clock_in, clock_out) = match chosen {
            Ok(chosen) => chosen,
            Err(e) => {
                self.message = format!("preferences: {e}");
                return;
            }
        };
        let mut rows = Vec::new();
        if !self.host.hosted && offers.devices.is_some() {
            rows.push(Row::Audio);
        }
        if !self.host.hosted && offers.midi_ports.is_some() {
            rows.extend([Row::Midi, Row::ClockIn]);
        }
        if !self.host.hosted && offers.midi_out_ports.is_some() {
            rows.extend([Row::MidiOut, Row::ClockOut]);
        }
        rows.extend([Row::Metronome, Row::Vus, Row::Toasts, Row::Ok]);
        self.chrome.overlay = Some(Overlay::Preferences(Prefs {
            rows,
            row: 0,
            audio,
            midi,
            midi_out,
            clock_in,
            clock_in_was: clock_in,
            clock_out,
            clock_out_was: clock_out,
            metronome: self.transport.metronome,
            vus: self.chrome.show_vus,
            toasts: !self.flash.hidden,
        }));
    }

    fn audio_choice(&self) -> Result<Choice, String> {
        let mut options = vec![(String::new(), "System default".to_owned())];
        if let Some(list) = self.playback.offers().devices {
            options.extend(list().into_iter().map(|name| (name.clone(), name)));
        }
        Ok(Choice::new(options, chosen_device()?.as_deref().unwrap_or_default()))
    }

    fn midi_choice(&self) -> Result<Choice, String> {
        let mut options = vec![
            (MidiPort::First.setting().to_owned(), "First available".to_owned()),
            (MidiPort::Off.setting().to_owned(), "None".to_owned()),
        ];
        if let Some(list) = self.playback.offers().midi_ports {
            options.extend(list().into_iter().map(|name| (name.clone(), name)));
        }
        Ok(Choice::new(options, chosen_midi_input()?.setting()))
    }

    fn midi_out_choice(&self) -> Result<Choice, String> {
        let mut options = vec![(MidiPort::Off.setting().to_owned(), "None".to_owned())];
        if let Some(list) = self.playback.offers().midi_out_ports {
            options.extend(list().into_iter().map(|name| (name.clone(), name)));
        }
        Ok(Choice::new(options, chosen_midi_output()?.setting()))
    }

    /// A key while the modal is open; whether it was.
    pub(super) fn prefs_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::Preferences(prefs)) = &mut self.chrome.overlay else { return false };
        let count = prefs.rows.len();
        match key {
            OverlayKey::Escape => self.chrome.overlay = None,
            OverlayKey::Move(delta) => prefs.row = (prefs.row as isize + delta).clamp(0, count as isize - 1) as usize,
            OverlayKey::Edge(end) => prefs.row = if *end { count - 1 } else { 0 },
            OverlayKey::Side(delta) => prefs.change(*delta),
            OverlayKey::Enter if prefs.rows[prefs.row] == Row::Ok => self.apply_preferences(),
            OverlayKey::Enter => prefs.change(1),
            OverlayKey::Back | OverlayKey::Letter(_) => {}
        }
        true
    }

    /// A click while the modal is open: outside it leaves it, on a line
    /// chooses it and changes it (OK applies); whether it was open.
    pub(super) fn prefs_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> bool {
        let Some(Overlay::Preferences(prefs)) = &mut self.chrome.overlay else { return false };
        let lines = prefs.lines();
        let window = (width, height);
        if !view::notice_contains(window, "Preferences", &lines, scale, x, y) {
            self.chrome.overlay = None;
        } else if let Some(i) = view::notice_line_at(window, "Preferences", &lines, scale, x, y)
            && i < prefs.rows.len()
        {
            prefs.row = i;
            if prefs.rows[i] == Row::Ok {
                self.apply_preferences();
            } else {
                prefs.change(1);
            }
        }
        true
    }

    /// OK: the draft applied. The sound output last: it reloads the session.
    fn apply_preferences(&mut self) {
        let Some(Overlay::Preferences(prefs)) = self.chrome.overlay.take() else { return };
        self.transport.metronome = prefs.metronome;
        if prefs.vus != self.chrome.show_vus {
            self.toggle_vus();
        }
        if prefs.toasts == self.flash.hidden {
            self.toggle_toasts();
        }
        self.message = "preferences kept".into();
        self.apply_midi_preferences(&prefs);
        if prefs.rows.contains(&Row::Audio) && prefs.audio.changed() {
            if let Err(e) = self.save_choice(DEVICE_KEY, prefs.audio.value()) {
                self.message = format!("audio output not saved: {e}");
                return;
            }
            let (_, label) = &prefs.audio.options[prefs.audio.at];
            let message = format!("audio output: {label}");
            self.write_pending();
            self.reload(message);
        }
    }

    /// The MIDI rows of the draft that changed, saved: the program opens its ports again.
    fn apply_midi_preferences(&mut self, prefs: &Prefs) {
        let rows = |row| prefs.rows.contains(&row);
        let settings = [
            (rows(Row::Midi) && prefs.midi.changed(), MIDI_KEY, Value::from(prefs.midi.value())),
            (rows(Row::ClockIn) && prefs.clock_in != prefs.clock_in_was, CLOCK_IN_KEY, Value::from(prefs.clock_in)),
            (rows(Row::MidiOut) && prefs.midi_out.changed(), MIDI_OUT_KEY, Value::from(prefs.midi_out.value())),
            (
                rows(Row::ClockOut) && prefs.clock_out != prefs.clock_out_was,
                CLOCK_OUT_KEY,
                Value::from(prefs.clock_out),
            ),
        ];
        for (_, key, value) in settings.into_iter().filter(|(changed, ..)| *changed) {
            if let Err(e) = self.save_setting_value(key, value) {
                self.message = format!("MIDI setting {key} not saved: {e}");
                return;
            }
            self.program.midi_changed = true;
            self.message = "MIDI settings chosen".into();
        }
    }

    fn save_setting_value(&self, key: &str, value: Value) -> Result<(), String> {
        let path = store::settings_path().ok_or(NO_USER_FOLDER)?;
        store::save_setting(&path, key, value).map_err(|e| e.to_string())
    }

    fn save_choice(&self, key: &str, value: &str) -> Result<(), String> {
        let path = store::settings_path().ok_or(NO_USER_FOLDER)?;
        store::save_setting(&path, key, Value::from(value)).map_err(|e| e.to_string())
    }
}
