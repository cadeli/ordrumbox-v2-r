//! The session's side of the synth editor (`synth_editor.js`,
//! `synth_preset_model.js`, `preset_section.js`): it opens on the track
//! editor's track's preset (else the first one, and a line above the cards
//! says what the track plays, with a button to put it on the preset shown)
//! and every change goes to
//! that preset at once — the player rebuilds what plays with it. The
//! presets the assets do not hold as they are belong to the library
//! (`Library::synths`): each change is an undo step, and they are saved with
//! it (`library.json`, the host's project). Revert puts back the preset as it
//! was when it was opened.

use crate::scale::Scale;
use std::time::Instant;

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;

use super::session_confirm::Pending;
use super::{NO_SYNTHS, Session, Workspace, session_panel};
use crate::canvas::{Canvas, text_width};
use crate::frame;
use crate::panel::Id;
use crate::panel::{Item, Kind, Rect, dim};
use crate::picker::{Export, Purpose};
use crate::store;
use crate::synth_view::{self, Built, SynthLayout};
use ordrumbox_engine::SynthPresets;
use std::path::{Path, PathBuf};

/// The button that puts the track editor's track on the preset shown.
const USE_BUTTON: &str = "use";

/// The preset being edited.
#[derive(Debug, Clone)]
pub(super) struct SynthEdit {
    pub(super) key: String,
    /// As it was when opened (Revert).
    original: Value,
    draft: Object,
}

/// The synth editor: the preset edited, the tab, and when the session
/// started (the LFO clock).
#[derive(Debug)]
pub(super) struct SynthEditor {
    edit: Option<SynthEdit>,
    tab: u8,
    started: Instant,
}

impl SynthEditor {
    pub(super) fn new() -> Self {
        Self { edit: None, tab: 0, started: Instant::now() }
    }

    /// Preset `key` is the one edited.
    pub(super) fn edits(&self, key: &str) -> bool {
        self.edit.as_ref().is_some_and(|edit| edit.key == key)
    }

    /// No preset edited any more.
    pub(super) fn close(&mut self) {
        self.edit = None;
    }
}

impl Session {
    /// The synth editor is shown (the kit editor opens over it).
    pub fn synth_open(&self) -> bool {
        self.workspace == Workspace::Synth && self.kit_editor.is_none()
    }

    /// F6, View > Synth: the synth editor in the place of the grid (on the
    /// track's preset, else the first), or back (`showPanel`, `closePanelAndCommit`).
    pub fn toggle_synth(&mut self) {
        if self.workspace == Workspace::Synth {
            self.synth_editor.edit = None;
            return self.set_workspace(Workspace::Grid);
        }
        self.set_workspace(Workspace::Synth);
        let track_key = self.editor_track().and_then(|t| self.track(t)).and_then(|t| t.synth_preset.clone());
        let key = track_key.filter(|k| self.synths.get(k).is_some()).or_else(|| self.synth_keys().into_iter().next());
        match key {
            Some(key) => {
                self.load_synth(&key);
                if let Some((name, plays)) = self.track_plays().filter(|(_, plays)| plays.is_some()) {
                    self.message =
                        format!("{name} {}: \"{key}\" shown, USE ON {name} to play it", plays.unwrap_or_default());
                }
            }
            None => self.message = "No synth presets loaded.".into(),
        }
    }

    /// The track editor's track's name, and what it plays when it is not
    /// the preset shown (`None`: it plays it).
    fn track_plays(&self) -> Option<(String, Option<String>)> {
        let track = self.track(self.editor_track()?)?;
        let key = self.synth_editor.edit.as_ref().map(|e| e.key.as_str());
        let plays = match &track.synth_preset {
            Some(preset) if track.use_synth && Some(preset.as_str()) == key => None,
            Some(preset) if track.use_synth => Some(format!("plays synth \"{preset}\"")),
            _ => Some("plays a sample".to_owned()),
        };
        Some((track.name.clone(), plays))
    }

    /// The line under the preset bar, right of the tabs: what the track
    /// plays, and when it is not the preset shown, a button that puts it on it.
    fn track_line(&self, layout: &SynthLayout, scale: Scale, items: &mut Vec<Item>) {
        let Some((name, plays)) = self.track_plays() else { return };
        let tab = layout.tabs[layout.tabs.len() - 1];
        let mut x = tab.x + tab.w + 16 * scale;
        let text = match plays {
            None => format!("{name} plays this preset"),
            Some(plays) => {
                let label = format!("USE ON {name}");
                let w = text_width(&label, scale) + 12 * scale;
                let rect = Rect::new(x, tab.y, w, tab.h);
                items.push(Item::new(Some(Id::SynthButton(USE_BUTTON)), rect, Kind::Button { label, on: false }));
                x += w + 8 * scale;
                format!("{name} {plays}")
            }
        };
        items.push(Item::text(x, tab.y + 3 * scale, text, dim(), scale));
    }

    /// The presets' keys, sorted.
    pub(super) fn synth_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.synths.keys().into_iter().map(str::to_owned).collect();
        keys.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
        keys
    }

    /// Edit preset `key`.
    pub(super) fn load_synth(&mut self, key: &str) {
        let Some(preset) = self.synths.get(key).cloned() else { return };
        let mut draft = preset.as_object().cloned().unwrap_or_default();
        model::hydrate(&mut draft);
        self.synth_editor.edit = Some(SynthEdit { key: key.to_owned(), original: preset, draft });
    }

    /// For a library saved without presets of its own (before they were
    /// saved with it): those of the presets file `path` earlier versions
    /// saved that `assets` does not hold as they are. Saved with the library
    /// from then on; a file that cannot be read is said, left as it is, and
    /// read again at the next start (`None`).
    pub(super) fn read_user_synths(
        path: Option<PathBuf>,
        assets: &SynthPresets,
        status: &mut Vec<String>,
    ) -> Option<Object> {
        let Some(path) = path else { return Some(Object::new()) };
        match store::load_synths(&path) {
            Ok(saved) => {
                let mut own = Object::new();
                if let Some(saved) = &saved {
                    for key in saved.keys() {
                        if let Some(preset) = saved.get(key)
                            && assets.get(key) != Some(preset)
                        {
                            own.insert(key, preset.clone());
                        }
                    }
                }
                Some(own)
            }
            Err(e) => {
                status.push(format!(
                    "synth presets not loaded: {e} (once repaired, import it from the synth editor's preset bar)"
                ));
                None
            }
        }
    }

    /// The presets played and shown follow the library's own, when the
    /// library changed since they were made (an edit, an undo, a library
    /// replaced): to the player, to exports, to the preset edited.
    pub(super) fn refresh_synths(&mut self) {
        let revision = self.editor.revision();
        if self.synths_revision == Some(revision) {
            return;
        }
        self.synths_revision = Some(revision);
        let synths = merged_synths(self.editor.library().synths.as_ref(), &self.asset_synths);
        if synths == self.synths {
            return;
        }
        self.synths = synths;
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_synths(self.editor.library(), self.synths.clone());
        }
        if let Some(render) = &mut self.render {
            render.synths = self.synths.clone();
        }
        // The preset edited changed from elsewhere (an undo, an import):
        // opened again, Revert goes back to it; gone, the first one.
        let Some(edit) = &self.synth_editor.edit else { return };
        let key = edit.key.clone();
        match self.synths.get(&key) {
            Some(preset) if preset.as_object() != Some(&edit.draft) => self.load_synth(&key),
            Some(_) => {}
            None => match self.synth_keys().into_iter().next() {
                Some(first) => self.load_synth(&first),
                None => self.synth_editor.edit = None,
            },
        }
    }

    /// The assets' presets are `synths` (tests): the presets played follow.
    #[cfg(test)]
    pub(super) fn set_asset_synths(&mut self, synths: SynthPresets) {
        self.asset_synths = synths;
        self.synths_revision = None;
        self.refresh_synths();
    }

    /// Preset `key` becomes `preset` (`None`: deleted), one undo step `desc`
    /// (with `coalesce`, one with the edits of that key just before). One
    /// the assets hold as it is goes back to being theirs. Whether it changed.
    pub(super) fn put_synth(
        &mut self,
        key: &str,
        preset: Option<Value>,
        desc: String,
        coalesce: Option<String>,
    ) -> bool {
        self.put_synths(&[(key, preset)], desc, coalesce)
    }

    /// [`Self::put_synth`] for several presets, one undo step.
    fn put_synths(&mut self, presets: &[(&str, Option<Value>)], desc: String, coalesce: Option<String>) -> bool {
        let assets = &self.asset_synths;
        let changed = self.editor.edit_synths(desc, coalesce, |own| {
            for (key, preset) in presets {
                match (preset, assets.get(key)) {
                    (Some(preset), Some(asset)) if preset == asset => own.remove(key),
                    (Some(preset), _) => own.insert(*key, preset.clone()),
                    (None, Some(_)) => own.insert(*key, Value::Null),
                    (None, None) => own.remove(key),
                };
            }
        });
        if changed {
            self.edited();
        }
        changed
    }

    /// The draft into its preset (`commit`): a knob turned is one undo step.
    fn commit_synth(&mut self) {
        let Some(edit) = &self.synth_editor.edit else { return };
        let (key, preset) = (edit.key.clone(), Value::Object(edit.draft.clone()));
        let desc = format!("Edit synth preset \"{key}\"");
        self.put_synth(&key, Some(preset), desc, Some(format!("synth:{key}")));
    }

    /// Set `path` of the draft (a group off by default is switched on when
    /// one of its controls is touched: `#setValue`, `#implicitEnable`).
    fn edit_synth(&mut self, path: &str, value: Value) {
        let Some(edit) = &mut self.synth_editor.edit else { return };
        model::implicit_enable(&mut edit.draft, path);
        if model::set(&mut edit.draft, path, value) {
            self.commit_synth();
        }
    }

    // ─── What is shown ───────────────────────────────────────────────────────

    fn synth_layout(&self, size: (i32, i32, Scale)) -> SynthLayout {
        let body = self.body(size);
        SynthLayout::new(body.main_w, body.editor_h, size.2, self.status.len())
    }

    /// Seconds the LFOs are at, and the tempo they sync to.
    fn synth_clock(&self) -> (f64, f64) {
        (self.synth_editor.started.elapsed().as_secs_f64(), self.playing_bpm())
    }

    /// The editor as built for a `size` window, if it shows a preset.
    pub(super) fn synth_built(&self, size: (i32, i32, Scale)) -> Option<(Built, SynthLayout)> {
        let edit = self.synth_editor.edit.as_ref()?;
        let layout = self.synth_layout(size);
        let (time, bpm) = self.synth_clock();
        let lfo = |path: &str| {
            let modulation = model::lfo_mod_on(&edit.draft, path, time, bpm);
            (modulation != 0.0).then(|| model::number(&edit.draft, path).unwrap_or(0.0) + modulation)
        };
        let built = synth_view::build(&edit.draft, &self.synth_keys(), &edit.key, self.synth_editor.tab, &layout, &lfo);
        Some((built, layout))
    }

    pub(super) fn synth_items(&self, layout: &mut session_panel::PanelLayout, size: (i32, i32, Scale)) {
        if let Some((built, synth_layout)) = self.synth_built(size) {
            layout.items.extend(built.items);
            self.track_line(&synth_layout, size.2, &mut layout.items);
        }
    }

    /// What the view paints under the items: the cards and the plots.
    pub(super) fn draw_synth(&self, canvas: &mut Canvas, scale: Scale) {
        if !self.synth_open() {
            return;
        }
        let Some(edit) = &self.synth_editor.edit else {
            let s = scale;
            canvas.text(16 * s, frame::body_top(scale), "No synth presets loaded.", s, crate::panel::dim());
            return;
        };
        let Some((built, layout)) = self.synth_built((canvas.width(), canvas.height(), scale)) else { return };
        let (time, bpm) = self.synth_clock();
        synth_view::draw(canvas, &edit.draft, &built, &layout, time, bpm);
    }

    /// An LFO moves a knob or the plots: keep redrawing.
    pub fn synth_animating(&self) -> bool {
        self.synth_open()
            && self.synth_editor.edit.as_ref().is_some_and(|e| {
                ["lfo", "lfo2"].iter().any(|g| {
                    let target = model::get(&e.draft, &format!("{g}.target")).and_then(Value::as_str).unwrap_or("NOT");
                    target != "NOT"
                        && model::number(&e.draft, &format!("{g}.depth")).unwrap_or(0.0) > 0.0
                        && model::lfo_targeted(&e.draft, target)
                })
            })
    }

    // ─── The controls ────────────────────────────────────────────────────────

    /// The path of field `fi` of the draft.
    fn synth_field(&self, fi: u16) -> Option<String> {
        synth_view::all_fields(&self.synth_editor.edit.as_ref()?.draft).into_iter().nth(usize::from(fi))
    }

    /// A knob of the editor turned.
    pub(super) fn synth_value(&mut self, fi: u16, value: f64) {
        if let Some(path) = self.synth_field(fi) {
            self.edit_synth(&path, Value::from(if value.is_nan() { 0.0 } else { value }));
        }
    }

    /// A list of the editor set: the preset, or a field.
    pub(super) fn synth_select(&mut self, id: Id, value: &str) {
        match id {
            Id::SynthPreset => {
                if self.synth_editor.edit.as_ref().is_none_or(|e| e.key != value) {
                    self.load_synth(value);
                }
            }
            Id::SynthField(fi) => {
                if let Some(path) = self.synth_field(fi) {
                    self.edit_synth(&path, model::choice_value(&path, value));
                }
            }
            _ => {}
        }
    }

    /// A tab, a power button, a choice, a switch or a preset button pressed.
    pub(super) fn synth_activate(&mut self, id: Id) {
        match id {
            Id::SynthTab(i) => self.synth_editor.tab = i.min(3),
            Id::SynthButton(button) => self.synth_button(button),
            Id::SynthPower(gi) => {
                let Some(edit) = &mut self.synth_editor.edit else { return };
                let Some(group) = model::groups(&edit.draft).into_iter().nth(usize::from(gi)) else { return };
                model::toggle_card(&mut edit.draft, &group);
                self.commit_synth();
            }
            Id::SynthIcon(gi, oi) => {
                let Some(edit) = &self.synth_editor.edit else { return };
                let Some(group) = model::groups(&edit.draft).into_iter().nth(usize::from(gi)) else { return };
                let (path, value) = if group.starts_with("vco") || group.starts_with("lfo") {
                    (format!("{group}.wave"), model::WAVES.get(usize::from(oi)).map(|w| Value::from(*w)))
                } else if group == "filter" || group == "noise" {
                    let key = if group == "filter" { "type" } else { "filterType" };
                    (format!("{group}.{key}"), model::FILTER_TYPES.get(usize::from(oi)).map(|t| Value::from(*t)))
                } else {
                    ("fm.algo".to_owned(), Some(Value::from(f64::from(oi))))
                };
                if let Some(value) = value {
                    self.edit_synth(&path, value);
                }
            }
            Id::SynthField(fi) => {
                let Some(path) = self.synth_field(fi) else { return };
                let on =
                    self.synth_editor.edit.as_ref().and_then(|e| model::get(&e.draft, &path)).and_then(Value::as_bool);
                if let Some(on) = on {
                    self.edit_synth(&path, Value::Bool(!on));
                }
            }
            _ => {}
        }
    }

    /// Delete preset `key`; the next one is shown.
    pub(super) fn remove_synth(&mut self, key: &str) {
        let keys = self.synth_keys();
        let list: Vec<&str> = keys.iter().map(String::as_str).collect();
        let next = model::after_delete(&list, key).map(str::to_owned);
        if let Some(next) = next {
            self.load_synth(&next);
        }
        self.put_synth(key, None, format!("Delete synth preset \"{key}\""), None);
    }

    /// The preset bar's buttons (`preset_section.js`).
    pub(super) fn synth_button(&mut self, button: &str) {
        match button {
            "delete" => {
                let Some(key) = self.synth_editor.edit.as_ref().map(|e| e.key.clone()) else { return };
                let keys = self.synth_keys();
                if keys.len() <= 1 {
                    self.message = "Cannot delete the last preset".into();
                    return;
                }
                self.ask(Pending::RemoveSynth(key));
            }
            "new" => {
                let keys = self.synth_keys();
                let name = model::new_preset_name(&keys.iter().map(String::as_str).collect::<Vec<_>>());
                let desc = format!("New synth preset \"{name}\"");
                self.put_synth(&name, Some(Value::Object(model::defaults())), desc, None);
                self.load_synth(&name);
            }
            "duplicate" => {
                let Some(edit) = &mut self.synth_editor.edit else { return };
                let key = format!("{}_copy", edit.key);
                edit.key = key.clone();
                edit.original = Value::Object(edit.draft.clone());
                let preset = edit.original.clone();
                self.put_synth(&key, Some(preset), format!("Duplicate synth preset as \"{key}\""), None);
            }
            "revert" => {
                let Some(edit) = &mut self.synth_editor.edit else { return };
                edit.draft = edit.original.as_object().cloned().unwrap_or_default();
                model::hydrate(&mut edit.draft);
                let (key, original) = (edit.key.clone(), edit.original.clone());
                self.put_synth(&key, Some(original), format!("Revert synth preset \"{key}\""), None);
            }
            USE_BUTTON => {
                let Some(key) = self.synth_editor.edit.as_ref().map(|e| e.key.clone()) else { return };
                let Some(t) = self.editor_track() else { return };
                self.assign_synth(t, &key);
                self.message = format!("{} plays \"{key}\"", self.track_name(t));
            }
            "export" => self.open_export(Export::Synths),
            "import" => self.open_picker(Purpose::ImportSynth),
            _ => {}
        }
    }

    /// Export: every preset to `path`.
    pub(super) fn export_synths(&mut self, path: &Path) {
        if self.synths.is_empty() {
            self.message = NO_SYNTHS.into();
            return;
        }
        self.message = match store::save_synths(path, &self.synths) {
            Ok(()) => format!("Synth sounds exported to {}", path.display()),
            Err(e) => format!("Export failed: {}: {e}", path.display()),
        };
    }

    /// Import: the presets of a JSON object join (or replace) the presets.
    pub(super) fn import_synths(&mut self, path: &Path) {
        let value = match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| ordrumbox_core::json::parse(&t).map_err(|e| e.to_string()))
        {
            Ok(value) => value,
            Err(e) => {
                self.message = format!("Import failed: {e}");
                return;
            }
        };
        let Some(object) = value.as_object() else {
            self.message = "Invalid synth file: expected a JSON object".into();
            return;
        };
        let presets: Vec<(&str, Option<Value>)> = object
            .iter()
            .filter(|(_, preset)| preset.as_object().is_some())
            .map(|(k, p)| (k, Some(p.clone())))
            .collect();
        let desc = format!("Import {} synth preset(s)", presets.len());
        self.put_synths(&presets, desc, None);
        self.message = format!("Imported {} synth sound(s)", presets.len());
    }
}

/// The presets played: `own`, a library's (`null`: one of the assets'
/// deleted), then the assets' ones it does not name, so a new version's
/// presets appear.
pub(super) fn merged_synths(own: Option<&Object>, assets: &SynthPresets) -> SynthPresets {
    let mut merged = SynthPresets::default();
    for (key, preset) in own.into_iter().flat_map(Object::iter) {
        if !preset.is_null() {
            merged.set(key, preset.clone());
        }
    }
    for key in assets.keys() {
        if own.is_none_or(|own| own.get(key).is_none())
            && let Some(preset) = assets.get(key)
        {
            merged.set(key, preset.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::super::OverlayKey;
    use super::*;
    use crate::kits::KitSet;
    use crate::view::Cursor;
    use ordrumbox_core::Library;

    use crate::test_support::{SIZE, click};

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"BASS","useSynth":true,"synthPreset":"b"}]}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        s.set_asset_synths(
            SynthPresets::from_json(&ordrumbox_core::json::parse(r#"{"b":{"vco1":{"gain":0.5}},"a":{}}"#).unwrap())
                .unwrap(),
        );
        s.set_size(SIZE);
        s
    }

    fn item(s: &Session, pick: impl Fn(&Item) -> bool) -> Item {
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        layout.items.into_iter().find(|i| pick(i)).expect("an item")
    }

    fn center(r: Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    #[test]
    fn opens_on_the_track_preset_and_edits_it() {
        let mut s = session();
        s.toggle_synth();
        assert_eq!(s.synth_editor.edit.as_ref().map(|e| e.key.as_str()), Some("b"));
        // VCO2's wave: VCO2 switches on (gain 0.5), the preset follows.
        let fields = synth_view::all_fields(&s.synth_editor.edit.as_ref().unwrap().draft);
        let groups = model::groups(&s.synth_editor.edit.as_ref().unwrap().draft);
        let vco2 = groups.iter().position(|g| g == "vco2").unwrap() as u8;
        let icon = item(&s, |i| i.id == Some(Id::SynthIcon(vco2, 2)));
        let (x, y) = center(icon.rect);
        click(&mut s, x, y);
        let preset = s.synths.get("b").unwrap();
        assert_eq!(model::get(preset.as_object().unwrap(), "vco2.wave"), Some(&Value::from("sawtooth")));
        assert_eq!(model::number(preset.as_object().unwrap(), "vco2.gain"), Some(0.5));
        assert!(s.saving.library.pending(), "saved with the library");
        // A knob: VCO1's gain, with the wheel.
        let fi = fields.iter().position(|f| f == "vco1.gain").unwrap() as u16;
        let knob = item(&s, |i| i.id == Some(Id::SynthField(fi)));
        let (x, y) = center(knob.rect);
        s.wheel(SIZE, x, y, 2, false, false);
        assert_eq!(model::number(s.synths.get("b").unwrap().as_object().unwrap(), "vco1.gain"), Some(0.52));
        // Revert: as it was.
        s.synth_button("revert");
        assert_eq!(s.synths.get("b").unwrap().to_json(), r#"{"vco1":{"gain":0.5}}"#);
        assert_eq!(model::number(&s.synth_editor.edit.as_ref().unwrap().draft, "vco2.gain"), Some(0.0));
    }

    #[test]
    fn a_sample_track_is_told_so_and_put_on_the_preset_shown() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"},{"name":"BASS","useSynth":true,"synthPreset":"b"}]}]}"#,
        )
        .unwrap();
        let mut s = session();
        s.editor = ordrumbox_logic::editor::Editor::new(library);
        s.selection.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        s.toggle_synth();
        assert_eq!(s.synth_editor.edit.as_ref().map(|e| e.key.as_str()), Some("a"), "the first preset");
        assert_eq!(s.message, "KICK plays a sample: \"a\" shown, USE ON KICK to play it");
        let text = |s: &Session| {
            let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
            layout.items.iter().find_map(|i| match &i.kind {
                Kind::Text { text, .. } if text.starts_with("KICK plays") => Some(text.clone()),
                _ => None,
            })
        };
        assert_eq!(text(&s).as_deref(), Some("KICK plays a sample"));
        let button = item(&s, |i| i.id == Some(Id::SynthButton(USE_BUTTON)));
        let (x, y) = center(button.rect);
        click(&mut s, x, y);
        let track = s.track(0).unwrap();
        assert!(track.use_synth && track.synth_preset.as_deref() == Some("a"));
        assert_eq!(s.message, "KICK plays \"a\"");
        assert_eq!(text(&s).as_deref(), Some("KICK plays this preset"));
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        assert!(!layout.items.iter().any(|i| i.id == Some(Id::SynthButton(USE_BUTTON))));
        // Another preset picked: the track still plays its own.
        s.synth_select(Id::SynthPreset, "b");
        assert_eq!(text(&s).as_deref(), Some("KICK plays synth \"a\""));
        // One undo step: back on its sample.
        s.undo();
        assert!(!s.track(0).unwrap().use_synth);
    }

    #[test]
    fn preset_bar() {
        let mut s = session();
        s.toggle_synth();
        s.synth_button("new");
        assert_eq!(s.synth_editor.edit.as_ref().unwrap().key, "new_preset");
        s.synth_button("duplicate");
        assert_eq!(s.synth_keys(), ["a", "b", "new_preset", "new_preset_copy"]);
        s.synth_button("delete");
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(s.synth_editor.edit.as_ref().unwrap().key, "new_preset");
        // The power button of the envelope (ENV tab).
        s.synth_activate(Id::SynthTab(3));
        let groups = model::groups(&s.synth_editor.edit.as_ref().unwrap().draft);
        let env = groups.iter().position(|g| g == "envelope").unwrap() as u8;
        s.synth_activate(Id::SynthPower(env));
        assert!(model::bypassed(&s.synth_editor.edit.as_ref().unwrap().draft, "envelope"));
        s.toggle_synth();
        assert!(s.synth_editor.edit.is_none() && s.workspace == Workspace::Grid);
    }

    #[test]
    fn preset_edits_are_undone_and_saved_with_the_library() {
        let mut s = session();
        s.toggle_synth();
        let gain = |s: &Session| model::number(s.synths.get("b").unwrap().as_object().unwrap(), "vco1.gain");
        let revision = s.library_revision();
        let fi = synth_view::all_fields(&s.synth_editor.edit.as_ref().unwrap().draft)
            .iter()
            .position(|f| f == "vco1.gain")
            .unwrap() as u16;
        s.synth_value(fi, 0.3);
        assert_eq!(gain(&s), Some(0.3));
        assert_ne!(s.library_revision(), revision, "a plugin gives the host the library again");
        assert!(s.library_text().contains("\"synthPresets\""));
        // Undo: as the assets hold it, the editor showing it.
        s.undo();
        assert_eq!(gain(&s), Some(0.5));
        assert_eq!(model::number(&s.synth_editor.edit.as_ref().unwrap().draft, "vco1.gain"), Some(0.5));
        assert_eq!(s.editor.library().synths, None, "nothing of its own");
        s.redo();
        assert_eq!(gain(&s), Some(0.3));

        // An asset's preset deleted stays deleted: saved as null.
        s.remove_synth("a");
        assert_eq!(s.synth_keys(), ["b"]);
        let (library, _) = Library::parse(&s.library_text()).unwrap();
        let own = library.synths.clone().unwrap();
        assert!(own.get("a").is_some_and(Value::is_null));
        assert_eq!(merged_synths(Some(&own), &s.asset_synths).keys(), ["b"]);
        s.undo();
        assert_eq!(s.synth_keys(), ["a", "b"]);
    }
}
