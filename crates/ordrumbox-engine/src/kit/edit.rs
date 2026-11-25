//! The edits of the kit editor on kits and their samples, each with the
//! message that says what changed; the UI keeps the audio and the files.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::Value;

use super::{Kit, KitSample};
use crate::kit_folders::key_from_file_name;

/// A setting of a kit sample that a knob or a switch changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleField {
    Gain,
    Transpose,
    Decay,
    /// Where reading starts and ends, ms into the sample.
    Start,
    End,
    /// Where the loop starts and ends, ms into the sample.
    LoopStart,
    LoopEnd,
    Attack,
    Hold,
    /// On / off, set to 1 or 0.
    Reverse,
    Loop,
    Gate,
}

/// A switch value: on from 0.5.
fn is_on(value: f64) -> bool {
    value >= 0.5
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

impl KitSample {
    /// Set gain (dB), tune (semitones), decay, start, end, loop start, loop
    /// end, attack or hold (ms), or the reverse, gate or loop switch (1 or
    /// 0); the message when it changed.
    pub fn set_value(&mut self, field: SampleField, value: f64) -> Option<String> {
        let name = &self.display_name;
        let ms = |what: &str| Some(format!("{name}: {what} {value} ms"));
        match field {
            SampleField::Gain if value != self.gain_db => {
                self.gain_db = value;
                Some(format!("{name}: gain {value:+.1} dB"))
            }
            SampleField::Transpose if value != self.transpose => {
                self.transpose = value;
                Some(format!("{name}: tune {value:+.1} st"))
            }
            SampleField::Decay if Some(value) != self.decay_ms => {
                let message = ms("decay");
                self.decay_ms = Some(value);
                message
            }
            SampleField::Start if value != self.start_ms => {
                let message = ms("start");
                self.start_ms = value;
                message
            }
            SampleField::End if Some(value) != self.end_ms => {
                let message = ms("end");
                self.end_ms = Some(value);
                message
            }
            SampleField::LoopStart if Some(value) != self.loop_start_ms => {
                let message = ms("loop start");
                self.loop_start_ms = Some(value);
                message
            }
            SampleField::LoopEnd if Some(value) != self.loop_end_ms => {
                let message = ms("loop end");
                self.loop_end_ms = Some(value);
                message
            }
            SampleField::Attack if Some(value) != self.attack_ms => {
                let message = ms("attack");
                self.attack_ms = Some(value);
                message
            }
            SampleField::Hold if value != self.hold_ms => {
                let message = ms("hold");
                self.hold_ms = value;
                message
            }
            SampleField::Reverse if is_on(value) != self.reverse => {
                self.reverse = is_on(value);
                Some(format!("{}: reverse {}", self.display_name, on_off(self.reverse)))
            }
            SampleField::Gate if is_on(value) != self.gated => {
                self.gated = is_on(value);
                Some(format!("{}: follow note length {}", self.display_name, on_off(self.gated)))
            }
            SampleField::Loop if is_on(value) != self.looping => {
                self.looping = is_on(value);
                Some(format!("{}: loop {}", self.display_name, on_off(self.looping)))
            }
            _ => None,
        }
    }

    /// Set the instrument the sample is, an id of the catalog (another is no
    /// change); the message when it changed.
    pub fn set_instrument(&mut self, id: &str) -> Option<String> {
        catalog().by_exact_id(id)?;
        (self.key.as_deref() != Some(id)).then(|| {
            self.key = Some(id.to_owned());
            format!("Set \"{}\" to instrument \"{id}\"", self.display_name)
        })
    }

    /// The instrument the sample's file name gives; `Err` when it gives none.
    pub fn instrument_from_file_name(&self) -> Result<String, String> {
        let file = self.path.rsplit_once('/').map_or(self.path.as_str(), |(_, name)| name);
        key_from_file_name(file).ok_or_else(|| format!("No instrument in the name \"{file}\""))
    }
}

/// The instruments of `samples` from their names, each read beside the
/// others: a Roland CL is the claves next to its RS.
pub fn set_instruments_from_names(samples: &mut [KitSample]) {
    let names: Vec<&str> = samples.iter().map(|s| s.display_name.as_str()).collect();
    let keys: Vec<Option<String>> =
        catalog().from_kit_file_names(&names).iter().map(|i| i.map(|i| i.id.clone())).collect();
    for (sample, key) in samples.iter_mut().zip(keys) {
        sample.key = key;
    }
}

impl Kit {
    /// A text of the kit (its title, its description) by its `kits.json`
    /// key; empty, the key goes. The message when it changed.
    pub fn set_text(&mut self, key: &str, text: &str) -> Option<String> {
        let before = self.extra.get(key).and_then(Value::as_str).unwrap_or_default();
        if before == text {
            return None;
        }
        if text.is_empty() {
            self.extra.remove(key);
        } else {
            self.extra.insert(key, Value::from(text));
        }
        Some(format!("Kit \"{}\": {key} changed", self.name))
    }

    /// `sample` in the place of the sample of its path, else added; its index.
    pub fn put_sample(&mut self, sample: KitSample) -> usize {
        match self.samples.iter().position(|s| s.path == sample.path) {
            Some(i) => {
                self.samples[i] = sample;
                i
            }
            None => {
                self.samples.push(sample);
                self.samples.len() - 1
            }
        }
    }

    /// Sample `sample` removed; its path is kept as removed, so that a file
    /// of the kit's folder is not added back when the folders are read. The
    /// message when there was one.
    pub fn remove_sample(&mut self, sample: usize) -> Option<String> {
        (sample < self.samples.len()).then(|| {
            let removed = self.samples.remove(sample);
            let message = format!("Removed \"{}\"", removed.display_name);
            self.removed.push(removed.path);
            message
        })
    }
}

/// Where the sample of `url` is: kit and index.
#[must_use]
pub fn find_path(kits: &[Kit], url: &str) -> Option<(usize, usize)> {
    kits.iter().enumerate().find_map(|(k, kit)| Some((k, kit.samples.iter().position(|s| s.path == url)?)))
}

/// Sample `sample` of kit `kit` moved to the end of kit `target`; the message
/// when it moved.
pub fn move_sample(kits: &mut [Kit], kit: usize, sample: usize, target: usize) -> Option<String> {
    if target == kit || target >= kits.len() || sample >= kits.get(kit)?.samples.len() {
        return None;
    }
    let moved = kits[kit].samples.remove(sample);
    let message = format!("Moved \"{}\" to kit \"{}\"", moved.display_name, kits[target].name);
    kits[target].samples.push(moved);
    Some(message)
}

/// `kit` in the place of the kit of its name, else added; its index.
pub fn put_kit(kits: &mut Vec<Kit>, kit: Kit) -> usize {
    match kits.iter().position(|k| k.name == kit.name) {
        Some(i) => {
            kits[i] = kit;
            i
        }
        None => {
            kits.push(kit);
            kits.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kick() -> KitSample {
        KitSample {
            key: Some("KICK".into()),
            display_name: "kick".into(),
            path: "k.wav".into(),
            ..KitSample::default()
        }
    }

    #[test]
    fn a_value_set_twice_changes_once() {
        let mut s = kick();
        assert_eq!(s.set_value(SampleField::Gain, -3.0).as_deref(), Some("kick: gain -3.0 dB"));
        assert_eq!(s.set_value(SampleField::Gain, -3.0), None);
        assert_eq!(s.set_value(SampleField::Decay, 630.0).as_deref(), Some("kick: decay 630 ms"));
        assert_eq!(s.decay_ms, Some(630.0));
        assert_eq!(s.set_value(SampleField::Loop, 1.0).as_deref(), Some("kick: loop on"));
        assert_eq!(s.set_value(SampleField::Loop, 0.7), None);
        assert!(s.looping);
    }

    #[test]
    fn an_instrument_is_one_of_the_catalog() {
        let mut s = kick();
        assert_eq!(s.set_instrument("NO SUCH"), None);
        assert_eq!(s.set_instrument("KICK"), None);
        assert!(s.set_instrument("SNARE").is_some());
        assert_eq!(s.key.as_deref(), Some("SNARE"));
        s.path = "drums/SnareDrum.wav".into();
        assert_eq!(s.instrument_from_file_name().as_deref(), Ok("SNARE"));
        s.path = "drums/x.wav".into();
        assert_eq!(s.instrument_from_file_name(), Err("No instrument in the name \"x.wav\"".into()));
    }

    #[test]
    fn samples_move_go_and_come_back_in_place() {
        let mut kits = vec![Kit { name: "a".into(), samples: vec![kick()], ..Kit::default() }, Kit::default()];
        kits[1].name = "b".into();
        assert_eq!(find_path(&kits, "k.wav"), Some((0, 0)));
        assert_eq!(move_sample(&mut kits, 0, 0, 0), None);
        assert_eq!(move_sample(&mut kits, 0, 0, 1).as_deref(), Some("Moved \"kick\" to kit \"b\""));
        assert_eq!(find_path(&kits, "k.wav"), Some((1, 0)));
        assert_eq!(kits[1].put_sample(KitSample { gain_db: -6.0, ..kick() }), 0);
        assert_eq!(kits[1].samples[0].gain_db, -6.0);
        assert_eq!(kits[1].remove_sample(0).as_deref(), Some("Removed \"kick\""));
        assert_eq!(kits[1].removed, ["k.wav"], "kept, so that the folder does not add it back");
        assert_eq!(put_kit(&mut kits, Kit { name: "b".into(), ..Kit::default() }), 1);
        assert_eq!(put_kit(&mut kits, Kit { name: "c".into(), ..Kit::default() }), 2);
    }

    #[test]
    fn a_text_empty_goes() {
        let mut kit = Kit { name: "a".into(), ..Kit::default() };
        assert!(kit.set_text("desc", "dry").is_some());
        assert_eq!(kit.set_text("desc", "dry"), None);
        assert!(kit.set_text("desc", "").is_some());
        assert!(kit.extra.get("desc").is_none());
    }
}
