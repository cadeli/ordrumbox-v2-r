//! Generating notes and importing MIDI (only when asked for).

use super::*;

impl Editor {
    /// Shift+B: a new pattern made in a style (style `style`, else a drawn
    /// one): its tracks, meter and lines, the library at the style's tempo,
    /// one undo step; its index.
    pub fn generate_pattern(
        &mut self,
        scales: &Scales,
        styles: &Styles,
        style: Option<&str>,
        rnd: &mut dyn FnMut() -> f64,
    ) -> usize {
        let mut pattern = self.blank_pattern(None);
        let options = Options { genre: style.map(str::to_owned), ..Options::default() };
        let mut g = AutoGenerator::new(scales, styles, rnd);
        generate::restyle_pattern(&mut g, &mut pattern, &options);
        let tempo = g.tempo;
        let desc = format!("Generate pattern \"{}\"", pattern_name(&pattern));
        self.batch(&desc.clone(), |e| {
            e.set_style_tempo(tempo);
            e.edit_patterns(desc, |patterns| {
                patterns.push(pattern);
                patterns.len() - 1
            })
        })
    }

    /// The library at the tempo a style drew, when it drew one.
    pub(super) fn set_style_tempo(&mut self, tempo: Option<f64>) {
        if let Some(bpm) = tempo {
            self.set_bpm(bpm, false);
        }
    }

    /// ⟳ (B): pattern `p` made again in style `style` (else its own, else a
    /// drawn one), locked tracks kept, the library at the style's tempo; one
    /// undo step.
    pub fn regenerate_pattern(
        &mut self,
        p: usize,
        scales: &Scales,
        styles: &Styles,
        style: Option<&str>,
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let options = Options { genre: style.map(str::to_owned), ..Options::default() };
        let Some(pattern) = self.library.patterns.get(p) else { return false };
        let desc = format!("Generate \"{}\"", pattern_name(pattern));
        self.batch(&desc, |e| {
            let mut g = AutoGenerator::new(scales, styles, rnd);
            e.edit(
                p,
                |pattern| format!("Generate \"{}\"", pattern_name(pattern)),
                None,
                |pattern| {
                    generate::restyle_pattern(&mut g, pattern, &options);
                },
            );
            let tempo = g.tempo;
            e.set_style_tempo(tempo);
        });
        true
    }

    /// A generation macro of pattern `p` (`composeEnergy`…, 0–100) set to
    /// `value`, its lines made again from its seed; a drag is one undo step.
    pub fn set_pattern_macro(
        &mut self,
        p: usize,
        key: &'static str,
        value: f64,
        (scales, styles): (&Scales, &Styles),
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let value = if value.is_finite() { value.clamp(0.0, 100.0).round() } else { MACRO_NEUTRAL };
        let coalesce = Some(format!("pattern:{p}:{key}"));
        let label = match key {
            ENERGY => "energy",
            COMPLEXITY => "complexity",
            _ => "variation",
        };
        self.edit(
            p,
            |_| format!("Set {label} → {value}"),
            coalesce,
            |pattern| {
                pattern.extra.insert(key, Value::Number(value));
                generate::recompose(&mut AutoGenerator::new(scales, styles, rnd), pattern);
            },
        )
        .is_some()
    }

    /// AUTO: every unlocked track of pattern `p` in auto mode, or all out of
    /// it; whether auto mode is now on.
    pub fn toggle_pattern_auto(&mut self, p: usize) -> Option<bool> {
        let desc = |pattern: &Pattern| {
            let on = pattern.tracks.iter().any(|t| t.auto_mode);
            if on { "Auto on" } else { "Auto off" }.to_owned()
        };
        self.edit(p, desc, None, generate::toggle_auto)
    }

    /// ↻ Drum on pattern `p` (in style `style`, else the pattern's): whether
    /// drum generation is now on.
    pub fn toggle_drums(
        &mut self,
        p: usize,
        scales: &Scales,
        styles: &Styles,
        style: Option<&str>,
        rnd: &mut dyn FnMut() -> f64,
    ) -> Option<bool> {
        let desc = |pattern: &Pattern| {
            let on = pattern.tracks.iter().any(|t| instrument_family(&t.name).is_drum() && t.auto_mode);
            if on { "Generate drums" } else { "Drums auto off" }.to_owned()
        };
        let options = Options { genre: style.map(str::to_owned), ..Options::default() };
        self.edit(p, desc, None, |pattern| {
            generate::toggle_drums(&mut AutoGenerator::new(scales, styles, rnd), pattern, &options)
        })
    }

    /// ↻ on track `t` of pattern `p`: a new line in the pattern's style, one undo step.
    pub fn regenerate_track(
        &mut self,
        p: usize,
        t: usize,
        scales: &Scales,
        styles: &Styles,
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let name = match self.library.patterns.get(p).and_then(|pattern| pattern.tracks.get(t)) {
            Some(track) => track.name.clone(),
            None => return false,
        };
        self.edit(
            p,
            |_| format!("Generate {name}"),
            None,
            |pattern| generate::regenerate_track(&mut AutoGenerator::new(scales, styles, rnd), pattern, t),
        )
        .is_some()
    }

    /// ↻ Bass / ↻ Chords on pattern `p`: whether that
    /// generation is now on.
    pub fn toggle_melodic(
        &mut self,
        p: usize,
        kind: MelodicKind,
        scales: &Scales,
        styles: &Styles,
        rnd: &mut dyn FnMut() -> f64,
    ) -> Option<bool> {
        let (part, wanted) = match kind {
            MelodicKind::Bass => ("bass", InstrumentFamily::Bass),
            MelodicKind::Chords => ("chords", InstrumentFamily::Piano),
        };
        let desc = |pattern: &Pattern| {
            let on = pattern.tracks.iter().any(|t| instrument_family(&t.name) == wanted && t.auto_mode);
            if on { format!("Generate {part}") } else { format!("{part} auto off") }
        };
        self.edit(p, desc, None, |pattern| {
            generate::toggle_melodic(&mut AutoGenerator::new(scales, styles, rnd), pattern, kind)
        })
    }

    /// Patterns from the MIDI file `bytes` named `file_name`
    /// (`MidiImportService.importFile`), one undo step; `roots` are the
    /// samples' root notes. The index of the last new pattern and the
    /// message, or why nothing was imported.
    pub fn import_midi(
        &mut self,
        file_name: &str,
        bytes: &[u8],
        roots: &[(String, f64)],
    ) -> Result<(usize, String), String> {
        let imported = midi::import(file_name, bytes, roots)?;
        let index = self.edit_patterns("Import MIDI file".to_owned(), |patterns| {
            let mut taken: HashSet<String> = patterns.iter().map(|p| p.id.clone()).collect();
            for mut pattern in imported.patterns {
                pattern.id = unique_id(&slugify(pattern_name(&pattern)), &mut taken);
                patterns.push(pattern);
            }
            patterns.len() - 1
        });
        Ok((index, imported.message))
    }

    /// Random notes on track `t`.
    pub fn randomize_track(&mut self, p: usize, t: usize, rnd: &mut dyn FnMut() -> f64) -> bool {
        self.edit_track(
            p,
            t,
            |track| format!("Randomize {}", track.name),
            None,
            |track| generate::random_track(track, rnd),
        )
        .is_some()
    }

    /// The loop start of pattern `p` (loop `index` of the playback): its
    /// tracks in auto mode get new lines. Not an undo step — the lines change
    /// every loop, as in the JS app — but a change to save.
    pub fn auto_loop(
        &mut self,
        p: usize,
        index: u64,
        scales: &Scales,
        styles: &Styles,
        rnd: &mut dyn FnMut() -> f64,
    ) -> bool {
        let Some(pattern) = self.library.patterns.get_mut(p) else { return false };
        if !generate::plays_auto(pattern) {
            return false;
        }
        let changed = generate::run_loop(&mut AutoGenerator::new(scales, styles, rnd), pattern, index);
        if changed {
            self.revision += 1;
        }
        changed
    }

    /// Exchange the tracks of pattern `p` with `tracks`: the lines of a loop
    /// generated ahead going in or out. Not an undo step, like [`Self::auto_loop`].
    pub fn swap_tracks(&mut self, p: usize, tracks: &mut Vec<Track>) {
        if let Some(pattern) = self.library.patterns.get_mut(p) {
            std::mem::swap(&mut pattern.tracks, tracks);
            self.revision += 1;
        }
    }
}
