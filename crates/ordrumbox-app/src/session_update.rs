//! The update check (`update.rs`): at start when due, from Help on demand;
//! a newer release goes in the status lines and enables Help > Download.

use super::*;

/// The update check: its settings, the one under way, a newer release
/// found, and how a finished check wakes the window.
#[derive(Debug, Default)]
pub(super) struct Updates {
    settings: update::Settings,
    check: Option<update::Check>,
    newer_release: Option<update::Release>,
    waker: Option<update::Waker>,
}

impl Updates {
    /// A check is under way.
    pub(super) fn checking(&self) -> bool {
        self.check.is_some()
    }

    /// The newer release found, if any.
    pub(super) fn newer_release(&self) -> Option<&update::Release> {
        self.newer_release.as_ref()
    }
}

impl Session {
    /// The `updates` settings of `settings.json`, if it has some.
    pub(super) fn load_update_settings(&mut self) {
        let Some(path) = store::settings_path() else { return };
        match store::load_setting(&path, "updates") {
            Ok(Some(value)) => {
                let (settings, issues) = update::Settings::from_json(&value);
                self.status.extend(issues.into_iter().map(|issue| format!("settings.json: {issue}")));
                self.updates.settings = settings;
            }
            Ok(None) => {}
            // `load_master` has reported the file already.
            Err(_) => self.updates.settings.check = false,
        }
    }

    /// Keep `waker` for the checks, and check now when it is due.
    pub fn start_updates(&mut self, waker: update::Waker) {
        self.updates.waker = Some(waker);
        let now = update::now_secs();
        if self.updates.settings.due(now) {
            self.updates.settings.last_check = Some(now);
            if let Some(path) = store::settings_path()
                && let Err(e) = store::save_setting(&path, "updates", self.updates.settings.to_json())
            {
                self.status.push(format!("settings not saved: {}: {e}", path.display()));
            }
            self.check_updates(false);
        }
    }

    /// Start a check (`manual`: asked from Help, its outcome always told).
    pub(super) fn check_updates(&mut self, manual: bool) {
        let Some(waker) = &self.updates.waker else {
            self.message = "update check not available".into();
            return;
        };
        self.updates.check = Some(update::Check::start(manual, waker));
        if manual {
            self.message = "checking for updates...".into();
        }
    }

    /// Take a finished check's outcome; whether there is something to show.
    pub fn poll_updates(&mut self) -> bool {
        let Some(check) = &self.updates.check else { return false };
        let Some(outcome) = check.poll() else { return false };
        let manual = check.manual;
        self.updates.check = None;
        match outcome {
            update::Outcome::Newer(release) => {
                let line = format!(
                    "orDrumbox {} is available (this is {}): Help > Download",
                    release.version,
                    update::CURRENT
                );
                if !self.status.contains(&line) {
                    self.status.push(line.clone());
                }
                if manual {
                    self.message = line;
                }
                self.updates.newer_release = Some(release);
            }
            update::Outcome::UpToDate if manual => {
                self.message = format!("orDrumbox {} is up to date", update::CURRENT);
            }
            update::Outcome::NoRelease if manual => {
                self.message = format!("no release published yet (this is {})", update::CURRENT);
            }
            update::Outcome::UpToDate | update::Outcome::NoRelease => return false,
            update::Outcome::Failed(e) if manual => self.message = format!("update check failed: {e}"),
            update::Outcome::Failed(e) => self.status.push(format!("update check failed: {e}")),
        }
        true
    }

    /// Help > Download: the newer release's page in the browser.
    pub(super) fn open_download_page(&mut self) {
        let Some(release) = &self.updates.newer_release else {
            self.message = "no newer version found: Help > Check for updates".into();
            return;
        };
        self.message = match update::open_page(&release.url) {
            Ok(()) => format!("opened {}", release.url),
            Err(e) => e,
        };
    }
}
