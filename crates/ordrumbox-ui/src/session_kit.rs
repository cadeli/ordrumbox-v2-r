//! The kit editor (Ctrl+K): the kit's samples, the one selected and its
//! analysis, and what its keys do (hear, remove, normalize, export).

use super::session_confirm::Pending;
use super::{Session, session_grid};
use crate::panel::{Dropdown, Id};
use crate::scale::Scale;
use crate::view::KitView;
use crate::{theme, view};
use ordrumbox_core::instruments::catalog;
use ordrumbox_engine::analysis::Analysis;
use ordrumbox_engine::kit::KitSample;
use ordrumbox_engine::offline::RenderSetup;
use ordrumbox_engine::{analysis, offline};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::canvas::{chars_in, text_width};
use crate::editors::Choice;
use crate::panel::{self, Item, Kind, Rect};
use crate::song_view;
use crate::view::KitLayout;

/// The kit editor: what it shows of the current kit.
#[derive(Debug, Default)]
pub(super) struct KitEditor {
    /// The sample selected in the current kit, when the editor is open.
    pub(super) sample: Option<usize>,
    /// Analysis of the sample shown, by url.
    pub(super) analysis: Option<(String, Analysis)>,
    /// Where the kit and the sample shown are read from (folder, file),
    /// found with the analysis: looking costs a file system call a sample.
    pub(super) files: (String, Option<PathBuf>),
}

impl KitEditor {
    pub(super) const fn is_open(&self) -> bool {
        self.sample.is_some()
    }
}

impl Session {
    // ─── Kit editor ──────────────────────────────────────────────────────────

    pub fn kit_editor_open(&self) -> bool {
        self.kit_editor.is_open()
    }

    /// The kit editor's selected sample and the kit's sample count, when open.
    pub fn kit_list(&self) -> Option<(usize, usize)> {
        let sample = self.kit_editor.sample?;
        Some((sample, self.kit().map_or(0, |k| k.samples.len())))
    }

    /// Ctrl+K: the kit editor (the drumkit manager), open or closed.
    pub fn toggle_kit_editor(&mut self) {
        self.kit_editor.sample = match self.kit_editor.sample {
            Some(_) => None,
            None => Some(0),
        };
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
        self.update_analysis();
    }

    /// The analysis of the sample shown, computed when it changes.
    pub(super) fn update_analysis(&mut self) {
        let url = self.kit_editor.sample.and_then(|s| self.kits.sample(self.kit_index, s)).map(|i| i.path.clone());
        match url {
            Some(url) if self.kit_editor.analysis.as_ref().is_some_and(|(u, _)| *u == url) => {}
            Some(url) => {
                self.kit_editor.analysis = self.kits.buffer(&url).map(|b| (url.clone(), analysis::analyze(b)));
            }
            None => self.kit_editor.analysis = None,
        }
        self.kit_editor.files = match (self.kit_editor.sample, self.kit()) {
            (Some(sample), Some(kit)) => {
                (self.kit_folder(kit), kit.samples.get(sample).and_then(|s| self.kits.file(&s.path)))
            }
            _ => (String::new(), None),
        };
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
        let kit = self.kit().map(|k| k.name.clone()).unwrap_or_default();
        self.render = (!bank.samples.is_empty()).then(|| RenderSetup {
            bank: bank.clone(),
            scales: self.generators.scales.clone(),
            synths: self.synths.presets.clone(),
            kit,
            sample_rate: offline::EXPORT_RATE,
            seed: 1,
        });
        self.message = match (self.kits.save(), errors.first()) {
            (Err(e), _) => e,
            (Ok(()), Some(first)) => format!("{message} ({} sample(s) not loaded, first: {first})", errors.len()),
            (Ok(()), None) => message,
        };
        self.kit_index = self.kit_index.min(self.kits.kits().len().saturating_sub(1));
        self.kit_editor.analysis = None;
        self.select_kit(self.kit_index);
    }

    /// ↑↓: the previous / next sample of the kit.
    pub fn kit_select(&mut self, delta: isize) {
        let Some(sample) = self.kit_editor.sample else { return };
        let count = self.kit().map_or(0, |k| k.samples.len());
        if count > 0 {
            self.kit_editor.sample = Some((sample as isize + delta).rem_euclid(count as isize) as usize);
            self.update_analysis();
        }
    }

    /// A click on the kit editor's list: that sample, heard.
    pub fn kit_click(&mut self, sample: usize) {
        if !self.kit_editor.is_open() {
            return;
        }
        self.kit_editor.sample = Some(sample);
        self.update_analysis();
        self.kit_audition();
    }

    /// Enter: the selected sample, as it is.
    pub fn kit_audition(&mut self) {
        let Some(sample) = self.kit_editor.sample else { return };
        let Some(url) = self.kits.sample(self.kit_index, sample).map(|i| i.path.clone()) else { return };
        if let Some(controller) = self.playback.controller_mut()
            && let Some(index) = controller.bank().find_path(&url)
        {
            controller.audition_sample(index);
        }
    }

    /// Delete: the selected sample out of the kit.
    pub fn kit_remove(&mut self) {
        let Some(sample) = self.kit_editor.sample else { return };
        let kit = self.kit_index;
        if self.kits.kits().get(kit).is_some_and(|k| sample < k.samples.len()) {
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

    /// The kit editor's widgets above the sample's controls: the kit's title
    /// and description (a click edits them), the instrument of each sample
    /// shown in the table.
    pub(super) fn kit_items(&self, layout: &KitLayout, scale: Scale) -> Vec<Item> {
        let (Some(sample), Some(kit)) = (self.kit_editor.sample, self.kit()) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        let fitted = |text: &str, rect: Rect| -> String { text.chars().take(chars_in(rect.w, scale)).collect() };
        let text = |text: String| if text.is_empty() { None } else { Some(text) };
        let title = layout.title_rect();
        let (shown, color) = match text(kit_text(kit, KIT_TITLE)) {
            Some(t) => (t, theme::text()),
            None => ("(click to name it)".to_owned(), panel::dim()),
        };
        items.push(Item::new(Some(Id::KitText(KIT_TITLE)), title, Kind::Text { text: fitted(&shown, title), color }));
        let desc = layout.desc_rect(0);
        let lines = match text(kit_text(kit, KIT_DESC)) {
            Some(d) => song_view::wrap(&d, chars_in(desc.w, scale)).into_iter().map(|l| (l, theme::text())).collect(),
            None => vec![("(click to write about the kit)".to_owned(), panel::dim())],
        };
        let last = view::KIT_DESC_LINES - 1;
        for (line, (mut text, color)) in lines.iter().cloned().enumerate().take(view::KIT_DESC_LINES) {
            let rect = layout.desc_rect(line);
            // More than fits: the last line says so.
            if line == last && lines.len() > view::KIT_DESC_LINES {
                text = format!("{}...", fitted(&text, Rect { w: rect.w - text_width("...", scale), ..rect }));
            }
            items.push(Item::new(Some(Id::KitText(KIT_DESC)), rect, Kind::Text { text, color }));
        }
        let column = layout.played_by_rect();
        let played_by = Rect { w: text_width(PLAYED_BY, scale).min(column.w), ..column };
        let label = Kind::Text { text: fitted(PLAYED_BY, played_by), color: theme::muted() };
        items.push(Item::new(Some(Id::KitPlayedBy), played_by, label));
        let first = layout.first(sample);
        for (row, (i, s)) in kit.samples.iter().enumerate().skip(first).take(layout.shown()).enumerate() {
            let choice = instrument_choice(&s.key);
            items.push(Item::new(Some(Id::KitSampleInstrument(i)), layout.instrument_rect(row), choice.kind()));
            // Lit while the instrument is the one its file name gives.
            let on = self.kits.guess_instrument(self.kit_index, i).is_ok_and(|key| key == s.key);
            let auto = Kind::Button { label: "AUTO".into(), on };
            items.push(Item::new(Some(Id::KitAutoInstrument(i)), layout.auto_rect(row), auto));
        }
        items
    }

    /// The text entry of the kit's title or description.
    pub(super) fn kit_text_entry(&mut self, key: &'static str) {
        let Some(kit) = self.kit() else { return };
        let title = if key == KIT_TITLE { "Kit title:" } else { "Kit description:" };
        let text = kit_text(kit, key);
        self.show_entry(Id::KitText(key), None, title.into(), text);
    }

    /// The kit's title or description confirmed: saved with the kits.
    pub(super) fn commit_kit_text(&mut self, key: &str, text: &str) {
        if let Some(message) = self.kits.set_text(self.kit_index, key, text.trim()) {
            self.message = match self.kits.save() {
                Ok(()) => message,
                Err(e) => e,
            };
        }
    }

    /// The instrument menus of sample `sample`, under its row's `rect`: the
    /// categories, as the track's Rename menu has them.
    pub(super) fn kit_instrument_menu(&mut self, sample: usize, rect: Rect) {
        let Some(s) = self.kits.sample(self.kit_index, sample) else { return };
        let (values, labels) = session_grid::category_lines();
        let title = format!("{}: {}", sample_file_name(&s.path), s.key);
        let menu = Dropdown::menu(Id::KitInstrumentMenu(sample), rect.x, rect.y + rect.h, title, values, labels);
        self.chrome.widgets.dropdown = Some(menu);
    }

    /// A line of sample `sample`'s instrument menus, picked in `menu`: a
    /// category opens its instruments beside it, an instrument is the sample's.
    pub(super) fn kit_instrument_menu_pick(&mut self, sample: usize, value: &str, menu: Dropdown) {
        if self.category_submenu(Id::KitInstrumentMenu(sample), value, menu) {
            return;
        }
        if let Some(id) = session_grid::picked_instrument(value) {
            self.kit_set_instrument(sample, id);
        }
    }

    /// The instrument select of sample `sample`'s row: that instrument.
    pub(super) fn kit_set_instrument(&mut self, sample: usize, id: &str) {
        if let Some(message) = self.kits.set_instrument(self.kit_index, sample, id) {
            self.kits_changed(message);
        }
    }

    /// The auto-assign button of sample `sample`'s row: the instrument its
    /// file name gives, else a message saying it gives none.
    pub(super) fn kit_auto_instrument(&mut self, sample: usize) {
        match self.kits.guess_instrument(self.kit_index, sample) {
            Ok(id) => match self.kits.set_instrument(self.kit_index, sample, &id) {
                Some(message) => self.kits_changed(message),
                None => self.message = format!("Already {id}"),
            },
            Err(message) => self.message = message,
        }
    }

    /// What the kit editor shows; its sample's details start at `details_top`.
    pub(super) fn kit_view(&self, details_top: i32) -> Option<KitView<'_>> {
        let sample = self.kit_editor.sample?;
        let kit = self.kit()?;
        let samples = kit.samples.iter().map(|i| sample_file_name(&i.path)).collect();
        let selected = kit.samples.get(sample);
        let analysis = self
            .kit_editor
            .analysis
            .as_ref()
            .filter(|(url, _)| selected.is_some_and(|i| i.path == *url))
            .map(|(_, a)| a);
        let facts = kit_facts(selected, self.kit_editor.files.1.as_deref(), analysis);
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
            folder: self.kit_editor.files.0.clone(),
            samples,
            users,
            selected: sample,
            details_top,
            facts,
            envelope: analysis.map_or(&[][..], |a| &a.envelope),
            decay_marker,
        })
    }
}

impl Session {
    /// The folder kit `kit`'s samples are read from: the one most of them
    /// are in, with how many other folders hold the rest; `-` without
    /// samples. Without sample folders (tests), the folder of their paths.
    fn kit_folder(&self, kit: &ordrumbox_engine::kit::Kit) -> String {
        let mut folders: Vec<(String, usize)> = Vec::new();
        for sample in &kit.samples {
            let folder = match self.kits.file(&sample.path) {
                Some(file) => file.parent().map_or_else(String::new, |p| p.display().to_string()),
                None => sample.path.rsplit_once('/').map_or_else(String::new, |(dir, _)| dir.to_owned()),
            };
            match folders.iter_mut().find(|(f, _)| *f == folder) {
                Some((_, count)) => *count += 1,
                None => folders.push((folder, 1)),
            }
        }
        // The first of the most used, in the kit's order.
        let Some(most) = folders.iter().map(|(_, count)| *count).max() else { return "-".to_owned() };
        let Some((main, _)) = folders.iter().find(|(_, count)| *count == most) else { return "-".to_owned() };
        match folders.len() - 1 {
            0 => main.clone(),
            others => format!("{main} (and {others} other folder{})", if others == 1 { "" } else { "s" }),
        }
    }
}

/// The file name of sample path `path` (`real/kick.wav`: `kick.wav`).
fn sample_file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// What the kit editor shows of a sample: [`view::KIT_FACTS`] lines, label
/// and value, `-` for what is not known (no sample, no analysis yet). The
/// file is where it is read from (`file`), else its path in the kits.
fn kit_facts(
    sample: Option<&KitSample>,
    file: Option<&Path>,
    analysis: Option<&analysis::Analysis>,
) -> Vec<(&'static str, String)> {
    let unknown = || "-".to_owned();
    let db = |v: f64| if v.is_finite() { format!("{v:.1} dB") } else { unknown() };
    let detected = sample.map_or_else(unknown, |i| {
        let name = if i.display_name.is_empty() { &i.path } else { &i.display_name };
        catalog().from_file_name(name).map_or_else(|| "none".to_owned(), |d| d.id.clone())
    });
    let facts = vec![
        ("Pitch", analysis.and_then(|a| a.note).map_or_else(unknown, |n| n.to_string())),
        ("Length", analysis.map_or_else(unknown, |a| format!("{:.0} ms", a.duration_sec * 1000.0))),
        ("Peak", analysis.map_or_else(unknown, |a| db(a.peak_db))),
        ("RMS", analysis.map_or_else(unknown, |a| db(a.rms_db))),
        ("Detected", detected),
        (
            "File",
            file.map(|f| f.display().to_string()).or_else(|| sample.map(|i| i.path.clone())).unwrap_or_else(unknown),
        ),
    ];
    debug_assert_eq!(facts.len(), view::KIT_FACTS);
    facts
}

/// The kit's title (`info`) and description (`desc`) keys, as `kits.json`
/// has them.
pub(super) const KIT_TITLE: &str = "info";
pub(super) const KIT_DESC: &str = "desc";
/// The title of the table's last column.
const PLAYED_BY: &str = "Played by track";

/// The kit's text of `key` (empty when it has none).
fn kit_text(kit: &ordrumbox_engine::kit::Kit, key: &str) -> String {
    kit.extra.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_owned()
}

/// The instruments to choose from for a sample of instrument `key`: the
/// catalog's, and `key` first when the catalog has no such instrument (a
/// sample nothing was detected for), so that the select shows what it is.
fn instrument_choice(key: &str) -> Choice {
    let mut values: Vec<String> = catalog().instruments.iter().map(|i| i.id.clone()).collect();
    let selected = values.iter().position(|v| v == key).unwrap_or_else(|| {
        values.insert(0, key.to_owned());
        0
    });
    Choice { values, labels: Vec::new(), selected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use ordrumbox_core::Library;
    use ordrumbox_engine::kit::{BankSample, Kit};
    use ordrumbox_engine::{SampleBank, SampleBuffer};

    use crate::test_support::{SIZE, click};

    /// A session on kit `real`: a KICK and a sample of no known instrument,
    /// its kit editor open.
    fn session() -> Session {
        let mut s = session_of(&[("KICK", "real/kick.wav"), ("WEIRD", "real/x.wav")]);
        s.toggle_kit_editor();
        s
    }

    /// A session of a KICK track on kit `real`, of `samples` (instrument, path).
    fn session_of(samples: &[(&str, &str)]) -> Session {
        let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"}]}]}"#).unwrap();
        let sample = |key: &str, path: &str| KitSample { key: key.into(), path: path.into(), ..Default::default() };
        let samples: Vec<_> = samples.iter().map(|&(key, path)| sample(key, path)).collect();
        let buffer = Arc::new(SampleBuffer::mono(vec![0.25, -0.5, 0.1], 44_100));
        let bank = SampleBank {
            samples: samples
                .iter()
                .map(|s| BankSample { kit: "real".into(), sample: s.clone(), buffer: buffer.clone() })
                .collect(),
        };
        let kits = vec![Kit { name: "real".into(), samples, ..Kit::default() }];
        let mut s = Session::new(library, KitSet::new(kits, &bank, None, None, None), Vec::new());
        s.set_size(SIZE);
        s.kits_changed("kits loaded".into());
        s
    }

    /// The middle of the panel item `id`.
    fn item_at(s: &Session, id: Id) -> (i32, i32) {
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        let item = layout.items.iter().find(|i| i.id == Some(id)).expect("the item");
        (item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2)
    }

    fn pick(s: &mut Session, label: &str) {
        let d = s.chrome.widgets.dropdown.as_ref().expect("a menu");
        let i = d.labels.iter().position(|l| l == label).unwrap_or_else(|| panic!("{label} in {:?}", d.labels));
        s.pick(i);
    }

    #[test]
    fn the_table_names_the_files_and_the_folder() {
        let s = session();
        let view = s.kit_view(0).unwrap();
        assert_eq!(view.samples, ["kick.wav", "x.wav"]);
        assert_eq!(view.folder, "real");
        assert_eq!(view.facts.iter().find(|(label, _)| *label == "File").unwrap().1, "real/kick.wav");
    }

    #[test]
    fn an_instrument_is_picked_by_category_as_a_track_is_renamed() {
        let mut s = session();
        let (x, y) = item_at(&s, Id::KitSampleInstrument(1));
        assert!(click(&mut s, x, y));
        assert_eq!(s.chrome.widgets.dropdown.as_ref().map(|d| d.id), Some(Id::KitInstrumentMenu(1)));
        pick(&mut s, "Percussion >");
        pick(&mut s, "COWBELL");
        assert_eq!(s.kits.sample(0, 1).map(|i| i.key.as_str()), Some("COWBELL"));
        assert_eq!(s.kits.sample(0, 0).map(|i| i.key.as_str()), Some("KICK"), "only that sample");
    }

    #[test]
    fn the_sound_tab_picks_the_instrument_by_category_among_the_sampled_ones() {
        let mut s = session_of(&[("KICK", "real/kick.wav"), ("COWBELL", "real/bell.wav"), ("SNARE", "real/sn.wav")]);
        s.selection.cursor = Some(view::Cursor { track: 0, beat: 0, step: 0 });
        s.editors.tab = crate::editors::TAB_SOUND;
        let (x, y) = item_at(&s, Id::Instrument);
        assert!(click(&mut s, x, y));
        let menu = s.chrome.widgets.dropdown.as_ref().expect("a menu");
        assert_eq!(menu.id, Id::InstrumentMenu);
        assert_eq!(menu.title.as_deref(), Some("Instrument: KICK"));
        assert!(!menu.labels.contains(&"Toms >".to_owned()), "no tom sampled: {:?}", menu.labels);
        pick(&mut s, "Percussion >");
        let labels = &s.chrome.widgets.dropdown.as_ref().expect("the submenu").labels;
        assert_eq!(labels, &["COWBELL"], "only the sampled ones");
        pick(&mut s, "COWBELL");
        assert!(s.chrome.widgets.dropdown.is_none());
        assert_eq!(s.pattern().unwrap().tracks[0].sample_path, "real/bell.wav");
    }

    #[test]
    fn the_played_by_column_says_what_it_shows() {
        let mut s = session();
        let (x, y) = item_at(&s, Id::KitPlayedBy);
        assert!(s.control_hover(SIZE, x, y));
        let (id, _) = s.chrome.control_hover.expect("a tooltip");
        let [title, text] = s.control_tip(id).expect("its text");
        assert_eq!(title, "Played by track");
        assert!(text.contains("tracks") && text.contains("pattern"), "{text}");
        // A click on the title does nothing.
        let before = s.chrome.widgets.dropdown.is_some() || s.chrome.widgets.entry.is_some();
        click(&mut s, x, y);
        assert_eq!(s.chrome.widgets.dropdown.is_some() || s.chrome.widgets.entry.is_some(), before);
    }

    #[test]
    fn each_row_guesses_its_instrument_from_its_file_name() {
        let mut s = session();
        s.kit_set_instrument(0, "SNARE");
        let (x, y) = item_at(&s, Id::KitAutoInstrument(0));
        assert!(s.control_hover(SIZE, x, y));
        assert_eq!(
            s.chrome.control_hover.and_then(|(id, _)| s.control_tip(id)).map(|[title, _]| title).as_deref(),
            Some("Auto-assign")
        );
        assert!(!auto_lit(&s, 0), "SNARE is not what kick.wav gives");
        click(&mut s, x, y);
        assert_eq!(s.kits.sample(0, 0).map(|k| k.key.as_str()), Some("KICK"), "kick.wav");
        assert!(auto_lit(&s, 0), "KICK is");
        click(&mut s, x, y);
        assert_eq!(s.message, "Already KICK");
        // x.wav names no instrument: the sample keeps its own.
        let (x, y) = item_at(&s, Id::KitAutoInstrument(1));
        click(&mut s, x, y);
        assert_eq!(s.kits.sample(0, 1).map(|k| k.key.as_str()), Some("WEIRD"));
        assert!(s.message.contains("x.wav"), "{}", s.message);
        assert!(!auto_lit(&s, 1), "no guess, never lit");
    }

    /// Whether the AUTO button of sample `sample` is lit.
    fn auto_lit(s: &Session, sample: usize) -> bool {
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        let item = layout.items.iter().find(|i| i.id == Some(Id::KitAutoInstrument(sample))).expect("the button");
        matches!(item.kind, Kind::Button { on: true, .. })
    }

    #[test]
    fn the_text_entries_open_at_the_mouse() {
        let mut s = session();
        let (x, y) = item_at(&s, Id::KitText(KIT_DESC));
        assert!(click(&mut s, x, y));
        let entry = s.chrome.widgets.entry.as_ref().expect("the entry");
        assert_eq!(entry.id, Id::KitText(KIT_DESC));
        let r = entry.rect(SIZE.0, SIZE.1, SIZE.2);
        assert_eq!((r.x + 12, r.y + 22), (x, y), "its field starts at the mouse");
        // Near the window's edge, it stays inside.
        s.chrome.widgets.pointer = (SIZE.0 - 2, SIZE.1 - 2);
        s.kit_text_entry(KIT_TITLE);
        let r = s.chrome.widgets.entry.as_ref().unwrap().rect(SIZE.0, SIZE.1, SIZE.2);
        assert!(r.x + r.w <= SIZE.0 && r.y + r.h <= SIZE.1);
    }
}
