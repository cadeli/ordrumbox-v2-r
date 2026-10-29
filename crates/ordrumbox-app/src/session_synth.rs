//! The session's side of the synth editor (`synth_editor.js`,
//! `synth_preset_model.js`, `preset_section.js`): it opens on the track
//! editor's track's preset (else the first one) and every change goes to
//! that preset at once — the player rebuilds what plays with it — and to
//! `generated_sounds.json` in the user data folder half a second later (the
//! JS caches them in IndexedDB), read at start over the assets' presets.
//! Revert puts back the preset as it was when it was opened.

use std::time::Instant;

use ordrumbox_core::json::{Object, Value};

use super::*;
use crate::synth_model as model;
use crate::synth_view::{self, Built, SynthLayout};

/// Preset edits reach the disk this long after the last one.
const SYNTHS_DELAY: Duration = Duration::from_millis(500);

/// The preset being edited.
#[derive(Debug, Clone)]
pub(super) struct SynthEdit {
    key: String,
    /// As it was when opened (Revert).
    original: Value,
    draft: Object,
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
            self.synth = None;
            return self.set_workspace(Workspace::Grid);
        }
        self.set_workspace(Workspace::Synth);
        let track_key = self.editor_track().and_then(|t| self.track(t)).and_then(|t| t.synth_preset.clone());
        let key = track_key.filter(|k| self.synths.get(k).is_some()).or_else(|| self.synth_keys().into_iter().next());
        match key {
            Some(key) => self.load_synth(&key),
            None => self.message = "No synth presets loaded.".into(),
        }
    }

    /// The presets' keys, sorted.
    fn synth_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.synths.keys().into_iter().map(str::to_owned).collect();
        keys.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
        keys
    }

    /// Edit preset `key`.
    fn load_synth(&mut self, key: &str) {
        let Some(preset) = self.synths.get(key).cloned() else { return };
        let mut draft = preset.as_object().cloned().unwrap_or_default();
        model::hydrate(&mut draft);
        self.synth = Some(SynthEdit { key: key.to_owned(), original: preset, draft });
    }

    /// The presets saved, read at start over the assets' ones.
    pub(super) fn load_user_synths(synths: &mut SynthPresets, status: &mut Vec<String>) {
        let Some(path) = store::saved_synths_path() else { return };
        match store::load_synths(&path) {
            Ok(Some(saved)) => *synths = saved,
            Ok(None) => {}
            Err(e) => status.push(format!("synth presets not loaded: {e}")),
        }
    }

    /// The presets changed: to the player, to exports, to the disk soon.
    fn synths_changed(&mut self) {
        if let Some(controller) = &mut self.controller {
            controller.set_synths(self.editor.library(), self.synths.clone());
        }
        if let Some(render) = &mut self.render {
            render.synths = self.synths.clone();
        }
        self.synths_dirty = Some(Instant::now());
    }

    /// The draft into its preset (`commit`).
    fn commit_synth(&mut self) {
        let Some(edit) = &self.synth else { return };
        self.synths.set(&edit.key, Value::Object(edit.draft.clone()));
        self.synths_changed();
    }

    /// Write the presets when the edits have settled; when to call again.
    pub(super) fn flush_synths(&mut self) -> Option<Instant> {
        let since = self.synths_dirty?;
        let due = since + SYNTHS_DELAY;
        if Instant::now() < due {
            return Some(due);
        }
        self.synths_dirty = None;
        if let Some(path) = store::synths_path()
            && let Err(e) = store::save_synths(&path, &self.synths)
        {
            self.message = format!("Synth preset changes are not being saved: {}: {e}", path.display());
        }
        None
    }

    /// Set `path` of the draft (a group off by default is switched on when
    /// one of its controls is touched: `#setValue`, `#implicitEnable`).
    fn edit_synth(&mut self, path: &str, value: Value) {
        let Some(edit) = &mut self.synth else { return };
        model::implicit_enable(&mut edit.draft, path);
        if model::set(&mut edit.draft, path, value) {
            self.commit_synth();
        }
    }

    // ─── What is shown ───────────────────────────────────────────────────────

    fn synth_layout(&self, size: (i32, i32, i32)) -> SynthLayout {
        let body = self.body(size);
        SynthLayout::new(body.main_w, body.editor_h, size.2, self.status.len())
    }

    /// Seconds the LFOs are at, and the tempo they sync to.
    fn synth_clock(&self) -> (f64, f64) {
        (self.started.elapsed().as_secs_f64(), self.pattern().map_or(120.0, |p| p.bpm))
    }

    /// The editor as built for a `size` window, if it shows a preset.
    pub(super) fn synth_built(&self, size: (i32, i32, i32)) -> Option<(Built, SynthLayout)> {
        let edit = self.synth.as_ref()?;
        let layout = self.synth_layout(size);
        let (time, bpm) = self.synth_clock();
        let lfo = |path: &str| {
            let modulation = model::lfo_mod_on(&edit.draft, path, time, bpm);
            (modulation != 0.0).then(|| model::number(&edit.draft, path).unwrap_or(0.0) + modulation)
        };
        let built = synth_view::build(&edit.draft, &self.synth_keys(), &edit.key, self.synth_tab, &layout, &lfo);
        Some((built, layout))
    }

    pub(super) fn synth_items(&self, layout: &mut session_panel::PanelLayout, size: (i32, i32, i32)) {
        if let Some((built, _)) = self.synth_built(size) {
            layout.items.extend(built.items);
        }
    }

    /// What the view paints under the items: the cards and the plots.
    pub(super) fn draw_synth(&self, canvas: &mut Canvas, scale: i32) {
        if !self.synth_open() {
            return;
        }
        let Some(edit) = &self.synth else {
            let s = scale;
            canvas.text(16 * s, frame::body_top(scale), "No synth presets loaded.", s, crate::panel::DIM);
            return;
        };
        let Some((built, layout)) = self.synth_built((canvas.width(), canvas.height(), scale)) else { return };
        let (time, bpm) = self.synth_clock();
        synth_view::draw(canvas, &edit.draft, &built, &layout, time, bpm);
    }

    /// An LFO moves a knob or the plots: keep redrawing.
    pub fn synth_animating(&self) -> bool {
        self.synth_open()
            && self.synth.as_ref().is_some_and(|e| {
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
        synth_view::all_fields(&self.synth.as_ref()?.draft).into_iter().nth(usize::from(fi))
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
                if self.synth.as_ref().is_none_or(|e| e.key != value) {
                    self.load_synth(value);
                }
            }
            Id::SynthField(fi) => {
                if let Some(path) = self.synth_field(fi) {
                    self.edit_synth(&path, Value::from(value));
                }
            }
            _ => {}
        }
    }

    /// A tab, a power button, a choice, a switch or a preset button pressed.
    pub(super) fn synth_activate(&mut self, id: Id) {
        match id {
            Id::SynthTab(i) => self.synth_tab = i.min(3),
            Id::SynthButton(button) => self.synth_button(button),
            Id::SynthPower(gi) => {
                let Some(edit) = &mut self.synth else { return };
                let Some(group) = model::groups(&edit.draft).into_iter().nth(usize::from(gi)) else { return };
                model::toggle_card(&mut edit.draft, &group);
                self.commit_synth();
            }
            Id::SynthIcon(gi, oi) => {
                let Some(edit) = &self.synth else { return };
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
                let on = self.synth.as_ref().and_then(|e| model::get(&e.draft, &path)).and_then(Value::as_bool);
                if let Some(on) = on {
                    self.edit_synth(&path, Value::Bool(!on));
                }
            }
            _ => {}
        }
    }

    /// The preset bar's buttons (`preset_section.js`).
    pub(super) fn synth_button(&mut self, button: &str) {
        match button {
            "delete" => {
                let Some(key) = self.synth.as_ref().map(|e| e.key.clone()) else { return };
                let keys = self.synth_keys();
                if keys.len() <= 1 {
                    self.message = "Cannot delete the last preset".into();
                    return;
                }
                let list: Vec<&str> = keys.iter().map(String::as_str).collect();
                let next = model::after_delete(&list, &key).map(str::to_owned);
                self.synths.remove(&key);
                self.synths_changed();
                if let Some(next) = next {
                    self.load_synth(&next);
                }
                self.message = format!("Deleted \"{key}\"");
            }
            "new" => {
                let keys = self.synth_keys();
                let name = model::new_preset_name(&keys.iter().map(String::as_str).collect::<Vec<_>>());
                self.synths.set(&name, Value::Object(model::defaults()));
                self.synths_changed();
                self.load_synth(&name);
                self.message = format!("Preset \"{name}\" created");
            }
            "duplicate" => {
                let Some(edit) = &mut self.synth else { return };
                let key = format!("{}_copy", edit.key);
                edit.key = key.clone();
                edit.original = Value::Object(edit.draft.clone());
                self.commit_synth();
                self.message = format!("Preset \"{key}\" created");
            }
            "revert" => {
                let Some(edit) = &mut self.synth else { return };
                edit.draft = edit.original.as_object().cloned().unwrap_or_default();
                model::hydrate(&mut edit.draft);
                let (key, original) = (edit.key.clone(), edit.original.clone());
                self.synths.set(&key, original);
                self.synths_changed();
                self.message = format!("\"{key}\" reverted");
            }
            "export" => self.export_synths(),
            "import" => self.open_picker(Purpose::ImportSynth),
            _ => {}
        }
    }

    /// Export: every preset to `exports/ordrumbox-synth-sounds.json`.
    fn export_synths(&mut self) {
        if self.synths.is_empty() {
            self.message = "No synth sounds loaded".into();
            return;
        }
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        let path = dir.join("ordrumbox-synth-sounds.json");
        self.message = match store::save_synths(&path, &self.synths) {
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
        let mut count = 0;
        for (key, preset) in object.iter() {
            if preset.as_object().is_some() {
                self.synths.set(key, preset.clone());
                count += 1;
            }
        }
        self.synths_changed();
        if let Some(key) = self.synth.as_ref().map(|e| e.key.clone()) {
            self.load_synth(&key);
        }
        self.message = format!("Imported {count} synth sound(s)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::panel::{Item, Rect};

    const SIZE: (i32, i32, i32) = (1100, 680, 1);

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"BASS","useSynth":true,"synthPreset":"b"}]}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        s.synths =
            SynthPresets::from_json(&ordrumbox_core::json::parse(r#"{"b":{"vco1":{"gain":0.5}},"a":{}}"#).unwrap());
        s.set_size(SIZE);
        s
    }

    fn item(s: &Session, pick: impl Fn(&Item) -> bool) -> Item {
        let layout = s.panel_layout(SIZE.0, SIZE.1, 1);
        layout.items.into_iter().find(|i| pick(i)).expect("an item")
    }

    fn center(r: Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    #[test]
    fn opens_on_the_track_preset_and_edits_it() {
        let mut s = session();
        s.toggle_synth();
        assert_eq!(s.synth.as_ref().map(|e| e.key.as_str()), Some("b"));
        // VCO2's wave: VCO2 switches on (gain 0.5), the preset follows.
        let fields = synth_view::all_fields(&s.synth.as_ref().unwrap().draft);
        let groups = model::groups(&s.synth.as_ref().unwrap().draft);
        let vco2 = groups.iter().position(|g| g == "vco2").unwrap() as u8;
        let icon = item(&s, |i| i.id == Some(Id::SynthIcon(vco2, 2)));
        let (x, y) = center(icon.rect);
        s.mouse_down(SIZE, x, y, Button::Left, false);
        let preset = s.synths.get("b").unwrap();
        assert_eq!(model::get(preset.as_object().unwrap(), "vco2.wave"), Some(&Value::from("sawtooth")));
        assert_eq!(model::number(preset.as_object().unwrap(), "vco2.gain"), Some(0.5));
        assert!(s.synths_dirty.is_some());
        // A knob: VCO1's gain, with the wheel.
        let fi = fields.iter().position(|f| f == "vco1.gain").unwrap() as u16;
        let knob = item(&s, |i| i.id == Some(Id::SynthField(fi)));
        let (x, y) = center(knob.rect);
        s.wheel(SIZE, x, y, 2, false, false);
        assert_eq!(model::number(s.synths.get("b").unwrap().as_object().unwrap(), "vco1.gain"), Some(0.52));
        // Revert: as it was.
        s.synth_button("revert");
        assert_eq!(s.synths.get("b").unwrap().to_json(), r#"{"vco1":{"gain":0.5}}"#);
        assert_eq!(model::number(&s.synth.as_ref().unwrap().draft, "vco2.gain"), Some(0.0));
    }

    #[test]
    fn preset_bar() {
        let mut s = session();
        s.toggle_synth();
        s.synth_button("new");
        assert_eq!(s.synth.as_ref().unwrap().key, "new_preset");
        s.synth_button("duplicate");
        assert_eq!(s.synth_keys(), ["a", "b", "new_preset", "new_preset_copy"]);
        s.synth_button("delete");
        assert_eq!(s.synth.as_ref().unwrap().key, "new_preset");
        // The power button of the envelope (ENV tab).
        s.synth_activate(Id::SynthTab(3));
        let groups = model::groups(&s.synth.as_ref().unwrap().draft);
        let env = groups.iter().position(|g| g == "envelope").unwrap() as u8;
        s.synth_activate(Id::SynthPower(env));
        assert!(model::bypassed(&s.synth.as_ref().unwrap().draft, "envelope"));
        s.toggle_synth();
        assert!(s.synth.is_none() && s.workspace == Workspace::Grid);
    }
}
