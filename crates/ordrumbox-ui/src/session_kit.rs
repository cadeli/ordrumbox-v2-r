//! The kit editor (Ctrl+K): the kit's samples, the one selected and its
//! analysis, and what its keys do (hear, remove, normalize, export).

use super::*;

impl Session {
    // ─── Kit editor ──────────────────────────────────────────────────────────

    pub fn kit_editor_open(&self) -> bool {
        self.kit_editor.is_some()
    }

    /// The kit editor's selected sample and the kit's sample count, when open.
    pub fn kit_list(&self) -> Option<(usize, usize)> {
        let sample = self.kit_editor?;
        Some((sample, self.kits.kits.get(self.kit_index).map_or(0, |k| k.samples.len())))
    }

    /// Ctrl+K: the kit editor (the drumkit manager), open or closed.
    pub fn toggle_kit_editor(&mut self) {
        self.kit_editor = match self.kit_editor {
            Some(_) => None,
            None => Some(0),
        };
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
        self.update_analysis();
    }

    /// The analysis of the sample shown, computed when it changes.
    pub(super) fn update_analysis(&mut self) {
        let url = self.kit_editor.and_then(|s| self.kits.sample(self.kit_index, s)).map(|i| i.path.clone());
        match url {
            Some(url) if self.analysis.as_ref().is_some_and(|(u, _)| *u == url) => {}
            Some(url) => {
                self.analysis = self.kits.buffer(&url).map(|b| (url.clone(), analysis::analyze(b)));
            }
            None => self.analysis = None,
        }
    }

    /// The kits changed: a new bank for playback and exports, the root
    /// notes, the kits saved, the selection kept inside.
    pub(super) fn kits_changed(&mut self, message: String) {
        self.saving.kits.clear();
        self.track_analysis.borrow_mut().clear();
        let (bank, errors) = self.kits.bank();
        self.roots = bank.samples.iter().filter_map(|s| Some((s.sample.key.clone(), s.sample.root_midi?))).collect();
        let bank = Arc::new(bank);
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_bank(bank.clone());
            controller.refresh(self.editor.library());
        }
        let kit = self.kits.kits.get(self.kit_index).map(|k| k.name.clone()).unwrap_or_default();
        self.render = (!bank.samples.is_empty()).then(|| RenderSetup {
            bank: bank.clone(),
            scales: self.scales.clone(),
            synths: self.synths.clone(),
            kit,
            sample_rate: offline::EXPORT_RATE,
            seed: 1,
        });
        self.message = match (self.kits.save(), errors.first()) {
            (Err(e), _) => e,
            (Ok(()), Some(first)) => format!("{message} ({} sample(s) not loaded, first: {first})", errors.len()),
            (Ok(()), None) => message,
        };
        self.kit_index = self.kit_index.min(self.kits.kits.len().saturating_sub(1));
        self.analysis = None;
        self.select_kit(self.kit_index);
    }

    /// ↑↓: the previous / next sample of the kit.
    pub fn kit_select(&mut self, delta: isize) {
        let Some(sample) = self.kit_editor else { return };
        let count = self.kits.kits.get(self.kit_index).map_or(0, |k| k.samples.len());
        if count > 0 {
            self.kit_editor = Some((sample as isize + delta).rem_euclid(count as isize) as usize);
            self.update_analysis();
        }
    }

    /// A click on the kit editor's list: that sample, heard.
    pub fn kit_click(&mut self, sample: usize) {
        if self.kit_editor.is_none() {
            return;
        }
        self.kit_editor = Some(sample);
        self.update_analysis();
        self.kit_audition();
    }

    /// Enter: the selected sample, as it is.
    pub fn kit_audition(&mut self) {
        let Some(sample) = self.kit_editor else { return };
        let Some(url) = self.kits.sample(self.kit_index, sample).map(|i| i.path.clone()) else { return };
        if let Some(controller) = self.playback.controller_mut()
            && let Some(index) = controller.bank().find_path(&url)
        {
            controller.audition_sample(index);
        }
    }

    /// Delete: the selected sample out of the kit.
    pub fn kit_remove(&mut self) {
        let Some(sample) = self.kit_editor else { return };
        let kit = self.kit_index;
        if self.kits.kits.get(kit).is_some_and(|k| sample < k.samples.len()) {
            self.ask(Pending::RemoveSample { kit, sample });
        }
    }

    pub(super) fn kit_remove_now(&mut self, kit: usize, sample: usize) {
        if let Some(message) = self.kits.remove(kit, sample) {
            self.kits_changed(message);
        }
    }

    /// N: every sample of the kit to a 0 dB peak.
    pub fn kit_normalize(&mut self) {
        match self.kits.normalize(self.kit_index) {
            Ok(0) => self.message = "No samples to normalize".into(),
            Ok(n) => self.kits_changed(format!("Normalized {n} sample(s)")),
            Err(e) => self.message = e,
        }
    }

    /// E: the kit's mapping to a JSON file at `path`.
    pub(super) fn kit_export(&mut self, path: &Path) {
        self.message = self.kits.export_mapping(self.kit_index, path).unwrap_or_else(|e| e);
    }

    /// What the kit editor shows; its sample's details start at `details_top`.
    pub(super) fn kit_view(&self, details_top: i32) -> Option<KitView<'_>> {
        let sample = self.kit_editor?;
        let kit = self.kits.kits.get(self.kit_index)?;
        let samples = kit.samples.iter().map(|i| (i.display_name.as_str(), i.key.as_str())).collect();
        let selected = kit.samples.get(sample);
        let analysis =
            self.analysis.as_ref().filter(|(url, _)| selected.is_some_and(|i| i.path == *url)).map(|(_, a)| a);
        let info = analysis.map(|a| {
            let db = |v: f64| if v.is_finite() { format!("{v:.1}") } else { "-".into() };
            let note = a.note.map_or_else(|| "-".to_owned(), |n| n.to_string());
            format!("{note}  {:.0} ms  {} dB peak  {} dB RMS", a.duration_sec * 1000.0, db(a.peak_db), db(a.rms_db))
        });
        let detected = selected.map(|i| {
            let name = if i.display_name.is_empty() { &i.path } else { &i.display_name };
            ordrumbox_core::instruments::catalog()
                .from_file_name(name)
                .map_or_else(|| "No instrument detected".to_owned(), |d| format!("Detected: {}", d.id))
        });
        let decay_marker = match (selected, analysis) {
            (Some(i), Some(a)) if a.duration_sec > 0.0 => Some((i.decay_ms() / 1000.0 / a.duration_sec).min(1.0)),
            _ => None,
        };
        // Which tracks of the pattern play each sample.
        let paths: Vec<(String, String)> = (0..self.pattern().map_or(0, |p| p.tracks.len()))
            .filter_map(|t| Some((self.track_sample_path(t)?, self.track(t)?.name.clone())))
            .collect();
        let users = kit
            .samples
            .iter()
            .map(|s| paths.iter().filter(|(path, _)| *path == s.path).map(|(_, name)| name.clone()).collect())
            .collect();
        Some(KitView {
            kit: &kit.name,
            samples,
            users,
            selected: sample,
            details_top,
            info,
            detected,
            envelope: analysis.map_or(&[][..], |a| &a.envelope),
            decay_marker,
        })
    }
}
