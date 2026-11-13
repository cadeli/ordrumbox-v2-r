//! The generators: a pattern or a track made in a style, the style's macros,
//! the tracks and patterns in auto mode, and the random track.

use super::Session;
use super::session_labels::style_label;
use ordrumbox_core::Styles;
use ordrumbox_logic::generate;
use ordrumbox_logic::generate::MelodicKind;
use ordrumbox_logic::tools::learned_styles;

impl Session {
    // ─── Generation ──────────────────────────────────────────────────────────

    /// The styles the generation follows now: `styles.json` with what the
    /// library's tags teach.
    fn generation_styles(&mut self) -> Styles {
        learned_styles(&mut self.learned_styles, &self.styles, &self.editor).clone()
    }

    /// The header's style list: style `name` (empty: a drawn one) for the
    /// patterns to come, and the current pattern made again in it.
    pub fn choose_style(&mut self, name: &str) {
        self.gen_style = Some(name.to_owned()).filter(|n| !n.is_empty());
        let style = self.gen_style.clone().or_else(|| {
            let styles = self.generation_styles();
            let names = styles.names();
            let i = (self.rng.next_f64() * names.len() as f64) as usize;
            names.get(i.min(names.len().saturating_sub(1))).map(|n| (*n).to_owned())
        });
        self.regenerate_pattern_in(style.as_deref());
    }

    /// B, ⟳: the pattern made again in its style (or the chosen one when it
    /// has none); no pattern: a new one.
    pub fn regenerate_pattern(&mut self) {
        let has_style =
            self.pattern().is_some_and(|p| p.extra.get(generate::GENRE).is_some_and(|g| g.as_str().is_some()));
        let style = if has_style { None } else { self.gen_style.clone() };
        self.regenerate_pattern_in(style.as_deref());
    }

    fn regenerate_pattern_in(&mut self, style: Option<&str>) {
        if self.pattern().is_none() {
            return self.generate_pattern();
        }
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let p = self.pattern_index;
        if self.editor.regenerate_pattern(p, &self.scales, &styles, style, &mut || rng.next_f64()) {
            self.clamp_selection();
            self.edited();
        }
        self.message = format!("generated in {}", self.pattern_style_label(&styles));
    }

    /// The pattern's style as the messages name it.
    fn pattern_style_label(&self, styles: &Styles) -> String {
        let genre = self.pattern().and_then(|p| p.extra.get(generate::GENRE)?.as_str()).unwrap_or_default();
        styles.get(genre).map_or_else(|| genre.to_owned(), style_label)
    }

    /// A header macro (`composeEnergy`…) of the pattern set to `value`.
    pub fn set_macro(&mut self, key: &'static str, value: f64) {
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let sources = (&self.scales, &styles);
        if self.editor.set_pattern_macro(self.pattern_index, key, value, sources, &mut || rng.next_f64()) {
            self.edited();
        }
    }

    /// Whether the pattern's unlocked tracks play in auto mode (AUTO lit).
    pub fn pattern_auto(&self) -> bool {
        self.pattern().is_some_and(|p| p.tracks.iter().any(|t| !t.compose_lock && generate::is_auto_on(t)))
    }

    /// AUTO: every unlocked track in auto mode, or all out of it.
    pub fn toggle_pattern_auto(&mut self) {
        let Some(on) = self.editor.toggle_pattern_auto(self.pattern_index) else { return };
        self.edited();
        self.message = if on { "auto: the pattern moves as it plays".into() } else { "auto off".into() };
    }

    /// The focused track locked or unlocked (the generation leaves a locked one as it is).
    pub fn toggle_lock(&mut self) {
        let Some(t) = self.editor_track() else {
            self.message = "select a track first".into();
            return;
        };
        if self.editor.toggle_track(self.pattern_index, t, "composeLock") {
            self.edited();
        }
    }

    /// Shift+B: a new pattern, generated (style, tracks and lines).
    pub fn generate_pattern(&mut self) {
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let style = self.gen_style.as_deref();
        self.pattern_index = self.editor.generate_pattern(&self.scales, &styles, style, &mut || rng.next_f64());
        self.deselect();
        self.edited();
        self.chrome.current_page = 0;
        if let Some(pattern) = self.pattern() {
            let genre = pattern.extra.get(generate::GENRE).and_then(|g| g.as_str()).unwrap_or_default();
            let label = styles.get(genre).map_or_else(|| genre.to_owned(), style_label);
            self.message = format!("generated {} ({label})", pattern.name.as_deref().unwrap_or_default());
        }
    }

    /// D, S, A: ↻ Drum / Bass / Chords — new lines in auto mode, or auto mode off.
    pub fn toggle_generation(&mut self, part: Option<MelodicKind>) {
        let p = self.pattern_index;
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let rnd = &mut || rng.next_f64();
        let on = match part {
            None => self.editor.toggle_drums(p, &self.scales, &styles, self.gen_style.as_deref(), rnd),
            Some(kind) => self.editor.toggle_melodic(p, kind, &self.scales, &styles, rnd),
        };
        let Some(on) = on else { return };
        self.clamp_selection();
        self.edited();
        let name = match part {
            None => "drums",
            Some(MelodicKind::Bass) => "bass",
            Some(MelodicKind::Chords) => "chords",
        };
        self.message = if on {
            let genre = self.pattern().and_then(|p| p.extra.get(generate::GENRE)?.as_str()).unwrap_or_default();
            let label = styles.get(genre).map_or_else(|| genre.to_owned(), style_label);
            format!("{name}: generated, new lines every loop ({label})")
        } else {
            format!("{name}: auto off")
        };
    }

    /// ↻ in the Generate tab: a new line on the edited track, in the pattern's style.
    pub fn regenerate_track(&mut self, t: usize) {
        let styles = self.generation_styles();
        let name = self.track(t).map(|tr| tr.name.clone()).unwrap_or_default();
        if self.track(t).is_some_and(|tr| tr.compose_lock) {
            self.message = format!("{name} is locked: unlock it for a new line");
            return;
        }
        let rng = &mut self.rng;
        if self.editor.regenerate_track(self.pattern_index, t, &self.scales, &styles, &mut || rng.next_f64()) {
            self.edited();
            self.message = format!("new line on {name}");
        }
    }

    /// Z: auto mode of the focused track (a new line every loop).
    pub fn toggle_auto(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        if self.editor.toggle_track(self.pattern_index, t, "autoMode") {
            self.edited();
        }
    }

    /// X: random notes on the focused track.
    pub fn randomize_track(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        self.undoable(|s| s.randomize_track_now(t));
    }

    fn randomize_track_now(&mut self, t: usize) {
        let rng = &mut self.rng;
        if self.editor.randomize_track(self.pattern_index, t, &mut || rng.next_f64()) {
            self.selection.selected = None;
            self.edited();
            let name = self.track(t).map(|track| track.name.clone()).unwrap_or_default();
            self.message = format!("Randomized \"{name}\"");
        }
    }
}
