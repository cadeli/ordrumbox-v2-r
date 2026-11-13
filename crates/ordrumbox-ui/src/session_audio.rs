//! File > Audio output: the sound device to play on, chosen from the list the
//! program gives. The choice is a setting (`audioDevice`); the sound is started
//! again on the device, which reloads the session as File > Change user data
//! folder does, after what waits to be written is.

use super::session_user_data::NO_USER_FOLDER;
use super::*;
use crate::audio_output::DEVICE_KEY;

/// The value of the menu line for the system's default device.
const DEFAULT_VALUE: &str = "default";

impl Session {
    /// A menu of the sound devices, the one in use marked.
    pub fn open_audio_devices(&mut self) {
        let Some(list) = self.playback.devices() else {
            self.message = "this program does not let the sound device be chosen".into();
            return;
        };
        let chosen = match crate::audio_output::chosen_device() {
            Ok(chosen) => chosen,
            Err(e) => {
                self.message = format!("audio output: {e}");
                return;
            }
        };
        let mark = |on: bool| if on { "> " } else { "  " };
        let mut values = vec![DEFAULT_VALUE.to_owned()];
        let mut labels = vec![format!("{}System default", mark(chosen.is_none()))];
        for name in list() {
            labels.push(format!("{}{name}", mark(chosen.as_deref() == Some(name.as_str()))));
            values.push(format!("device:{name}"));
        }
        let scale = self.chrome.panel_size.2;
        let title = "Audio output".to_owned();
        self.chrome.widgets.dropdown =
            Some(Dropdown::menu(Id::AudioMenu, 8 * scale, menu::bar_height(scale) + 2 * scale, title, values, labels));
    }

    /// A line of the device menu picked: saved, and the sound started on it.
    pub(super) fn choose_audio_device(&mut self, value: &str) {
        let Some(path) = store::settings_path() else {
            self.message = format!("audio output not saved: {NO_USER_FOLDER}");
            return;
        };
        let name = value.strip_prefix("device:").unwrap_or_default();
        let saved = store::save_setting(&path, DEVICE_KEY, ordrumbox_core::json::Value::from(name));
        if let Err(e) = saved {
            self.message = format!("audio output not saved: {e}");
            return;
        }
        let message = if name.is_empty() {
            "audio output: the system's default".to_owned()
        } else {
            format!("audio output: {name}")
        };
        self.write_pending();
        self.reload(message);
    }
}
