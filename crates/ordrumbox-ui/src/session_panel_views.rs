//! What the editors' widgets show, built at every frame from the library,
//! the kits and the player: the track editor's tabs, the sound a track
//! plays, the note editor's note, the kit editor's controls.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::style::Macros;
use ordrumbox_engine::TrackSound;
use ordrumbox_engine::assign::{assign_track, substitute_for};

use super::Session;
use super::session_panel::NoteTarget;
use super::session_selection::NoteFocus;
use crate::editors::{
    self, ARP_DIRECTIONS, Choice, ComposeView, KitControls, LFO_TARGETS, NoteView, SampleBar, TrackView,
};
use crate::view::SoundLabel;
use ordrumbox_core::drum_taxonomy::instrument_family;
use ordrumbox_core::{Note, Styles, Track};
use ordrumbox_engine::analysis;
use ordrumbox_engine::analysis::Analysis;
use ordrumbox_logic::{compose, generate};

/// What a track plays, before it is worded for the grid or a tooltip.
#[derive(Debug)]
enum Sounding {
    /// The sample at `path` (`kit/file.wav`), standing in for the `wanted`
    /// instrument when the track's own is not in the kit.
    Sample {
        path: String,
        wanted: Option<String>,
    },
    /// A synth preset, by its key when it has one.
    Synth(Option<String>),
    Nothing,
}

impl Session {
    /// What track `t` plays: what the player assigned it, else its first pick.
    fn track_sound(&self, t: usize) -> TrackSound {
        let Some(track) = self.track(t) else { return TrackSound::None };
        let library = self.editor.library();
        match (self.playback.controller(), &self.render) {
            (Some(c), _) => c.track_sound(library, self.pattern_index, t),
            (None, Some(render)) => {
                let kit = self.kit().map_or("", |k| k.name.as_str());
                assign_track(track, &render.bank, kit, &self.synths.presets, &mut |_| 0)
            }
            (None, None) => TrackSound::None,
        }
    }

    /// What track `t` plays, before it is worded; `None` when that is not
    /// known (no such track, a sample the bank does not hold).
    fn track_sounding(&self, t: usize) -> Option<Sounding> {
        let track = self.track(t)?;
        Some(match self.track_sound(t) {
            TrackSound::Sample(i) => {
                let bank = self
                    .playback
                    .controller()
                    .map(|c| c.bank().clone())
                    .or_else(|| self.render.as_ref().map(|r| r.bank.clone()));
                let sample = &bank.as_ref().and_then(|b| b.get(i))?.sample;
                // A sample picked for want of the track's own instrument says so.
                let wanted = substitute_for(&track.name, &sample.key).filter(|_| track.auto_assign);
                Sounding::Sample { path: sample.path.clone(), wanted }
            }
            TrackSound::Synth(key) => Sounding::Synth(Some(key)),
            TrackSound::None if track.use_synth => Sounding::Synth(track.synth_preset.clone()),
            TrackSound::None if !track.auto_assign && !track.sample_path.is_empty() => {
                Sounding::Sample { path: track.sample_path.clone(), wanted: None }
            }
            TrackSound::None => Sounding::Nothing,
        })
    }

    /// What track `t` plays, in full (its name's tooltip): its sample
    /// (`kit/file`, after `for CONGAS:` when it stands in for the track's
    /// instrument), its synth preset, or that it has none.
    pub(super) fn track_sound_label(&self, t: usize) -> String {
        match self.track_sounding(t) {
            None => String::new(),
            Some(Sounding::Sample { path, wanted: Some(wanted) }) => format!("for {wanted}: {path}"),
            Some(Sounding::Sample { path, wanted: None }) => path,
            Some(Sounding::Synth(Some(key))) => format!("synth {key}"),
            Some(Sounding::Synth(None)) => "synth".to_owned(),
            Some(Sounding::Nothing) => "no sound".to_owned(),
        }
    }

    /// What the grid shows under track `t`'s name: where its sound comes
    /// from (its kit, `synth`) above the sound's own name (the sample's file
    /// without its extension, the preset).
    pub(super) fn track_sound_lines(&self, t: usize) -> SoundLabel {
        match self.track_sounding(t) {
            None => SoundLabel::default(),
            Some(Sounding::Sample { path, wanted }) => {
                let sample = SoundLabel::sample(&path);
                match wanted {
                    Some(wanted) => SoundLabel { source: format!("for {wanted}: {}", sample.source), ..sample },
                    None => sample,
                }
            }
            Some(Sounding::Synth(key)) => SoundLabel { source: "synth".to_owned(), name: key.unwrap_or_default() },
            Some(Sounding::Nothing) => SoundLabel { source: String::new(), name: "no sound".to_owned() },
        }
    }

    /// The url of the sample track `t` plays (none for a synth track).
    pub(super) fn track_sample_path(&self, t: usize) -> Option<String> {
        let track = self.track(t)?;
        if track.use_synth {
            return None;
        }
        let sound = self.track_sound(t);
        let bank = self
            .playback
            .controller()
            .map(|c| c.bank().clone())
            .or_else(|| self.render.as_ref().map(|r| r.bank.clone()));
        match sound {
            TrackSound::Sample(i) => bank.and_then(|b| b.get(i).map(|s| s.sample.path.clone())),
            _ => (!track.auto_assign).then(|| track.sample_path.clone()),
        }
    }

    fn analysis_of(&self, url: &str) -> Option<Analysis> {
        if let Some(a) = self.track_analysis.borrow().get(url) {
            return Some(a.clone());
        }
        let analysis = analysis::analyze(self.kits.buffer(url)?);
        self.track_analysis.borrow_mut().insert(url.to_owned(), analysis.clone());
        Some(analysis)
    }

    pub(super) fn track_view(&self, t: usize) -> Option<TrackView<'_>> {
        let track = self.track(t)?;
        let url = self.track_sample_path(t);
        let place = url.as_deref().and_then(|u| self.kits.find_path(u));
        let instrument = place.and_then(|(k, i)| self.kits.sample(k, i));
        let kit_name = place.and_then(|(k, _)| self.kits.kits().get(k)).map(|k| k.name.as_str());

        // `getSoundInfo`.
        let info = if track.use_synth {
            track.synth_preset.clone()
        } else {
            instrument.map(|i| {
                let name = if i.display_name.is_empty() { &i.key } else { &i.display_name };
                kit_name.map_or_else(|| name.clone(), |k| format!("{k}/{name}"))
            })
        };
        let title = format!("Track: {}{}", track.name, info.map(|i| format!(" - {i}")).unwrap_or_default());

        // Sample bar: analysis text, envelope, decay end (0 without a decay).
        let decay = instrument.and_then(|i| i.decay).unwrap_or(0.0);
        let sample = url.as_deref().filter(|_| !track.use_synth).and_then(|u| self.analysis_of(u)).map(|a| {
            let pitch = a.note.map_or_else(|| "—".to_owned(), |n| format!("{}{}", n.name, n.octave));
            let duration = format!("{} ms", crate::control::to_fixed_text(a.duration_sec * 1000.0, 0));
            let peak = format!("{} dB", crate::control::to_fixed_text(a.peak_db, 1));
            let marker = (a.duration_sec > 0.0).then(|| (decay / 1000.0 / a.duration_sec).min(1.0));
            SampleBar { info: format!("{pitch} · {duration} · {peak}"), envelope: a.envelope.clone(), marker }
        });

        // Instruments that have samples, sorted; the current one.
        let keys = self.sampled_instruments();
        let mut ids: Vec<String> =
            catalog().instruments.iter().map(|i| i.id.clone()).filter(|id| keys.contains(id.as_str())).collect();
        ids.sort();
        let current_id = match instrument.map(|i| i.key.as_str()) {
            Some(key) if keys.contains(key) => key.to_owned(),
            _ if keys.contains(track.name.as_str()) => track.name.clone(),
            _ => ids.first().cloned().unwrap_or_else(|| "KICK".into()),
        };
        let instruments = Choice { selected: editors::index_of(&ids, &current_id), values: ids, labels: Vec::new() };

        // Samples of that instrument: the current kit's first.
        let current_url = url.clone().unwrap_or_else(|| track.sample_path.clone());
        let found = self.samples_for(&current_id);
        let samples = if found.is_empty() && !track.use_synth {
            Choice { values: vec![String::new()], labels: vec!["— no samples —".into()], selected: 0 }
        } else {
            let mut values: Vec<String> = found.iter().map(|(_, url, _)| url.clone()).collect();
            let mut labels: Vec<String> = found.iter().map(|(kit, _, name)| format!("{kit}/{name}")).collect();
            if track.use_synth {
                // A synth track plays no sample: `none` first, shown (its empty
                // value is ignored when picked; a sample turns the synth off).
                values.insert(0, String::new());
                labels.insert(0, "none".into());
                Choice { selected: 0, values, labels }
            } else {
                Choice { selected: editors::index_of(&values, &current_url), values, labels }
            }
        };

        // Synth presets.
        let keys = self.synths.presets.keys();
        let current = if track.use_synth {
            track.synth_preset.clone().unwrap_or_else(|| ordrumbox_logic::generate::BASS_PRESET.into())
        } else {
            "none".into()
        };
        let mut values: Vec<String> = std::iter::once("none").chain(keys.iter().copied()).map(str::to_owned).collect();
        if track.use_synth && !keys.contains(&current.as_str()) {
            values.push(current.clone());
        }
        let synths = Choice { selected: editors::index_of(&values, &current), values, labels: Vec::new() };

        // LFO values while playing (the LFO step watch).
        let tick = self.playback.tick();
        let lfo = match tick {
            Some(tick) => LFO_TARGETS
                .iter()
                .filter_map(|target| {
                    let object = track.field(target.lfo_key)?;
                    Some((target.key, ordrumbox_core::lfo::value(object.as_object()?, tick as f64)))
                })
                .collect(),
            None => Vec::new(),
        };
        let bpm = self.playing_bpm();
        let compose = self.compose_view(track);
        Some(TrackView { track, title, sample, decay, instruments, samples, synths, lfo, bpm, compose })
    }

    /// The styles the header and the Generate tab show: what the library taught,
    /// else (before the first frame learned it) the written ones.
    pub(super) fn shown_styles(&self) -> &Styles {
        self.generators.shown_styles()
    }

    /// How a drum track plays its pattern's style (the style the
    /// generation last learned, else the written one).
    fn compose_view(&self, track: &Track) -> Option<ComposeView> {
        if !instrument_family(&track.name).is_drum() {
            return None;
        }
        let genre = self.pattern()?.extra.get(generate::GENRE)?.as_str()?;
        let style = self.shown_styles().get(genre)?;
        let macros = Macros::of(self.pattern()?);
        let shown = compose::track_settings(track, style, macros)?;
        Some(ComposeView { style: style.name.clone(), shown })
    }

    /// The instruments some kit has a sample of.
    pub(super) fn sampled_instruments(&self) -> std::collections::HashSet<String> {
        self.kits.kits().iter().flat_map(|k| k.samples.iter().map(|i| i.key.clone())).collect()
    }

    /// Every sample of instrument `id`: (kit, url, name), the current kit's
    /// first, then by kit and name.
    pub(super) fn samples_for(&self, id: &str) -> Vec<(String, String, String)> {
        let current = self.kit().map_or("", |k| k.name.as_str());
        let mut found: Vec<(String, String, String)> = self
            .kits
            .kits()
            .iter()
            .flat_map(|kit| {
                kit.samples.iter().filter(|i| i.key == id).map(|i| {
                    let name = if i.display_name.is_empty() { i.path.clone() } else { i.display_name.clone() };
                    (kit.name.clone(), i.path.clone(), name)
                })
            })
            .collect();
        found.sort_by(|a, b| {
            (a.0 != current)
                .cmp(&(b.0 != current))
                .then_with(|| editors::locale_cmp(&a.0, &b.0))
                .then_with(|| editors::locale_cmp(&a.2, &b.2))
        });
        found
    }

    pub(super) fn note_target(&self, t: usize) -> NoteTarget {
        let Some(track) = self.track(t) else { return NoteTarget::Detached };
        if let Some((st, n)) = self.selection.selected.filter(|&(st, n)| st == t && n < track.notes.len()) {
            debug_assert_eq!(st, t);
            return NoteTarget::Note(n);
        }
        match self.selection.note_focus {
            Some(NoteFocus::Step { track: ft, beat, step }) if ft == t => {
                match track.notes.iter().position(|n| n.beat == beat && n.step == step) {
                    Some(n) => NoteTarget::Note(n),
                    None => NoteTarget::Empty { beat, step },
                }
            }
            _ if track.notes.is_empty() => NoteTarget::Detached,
            _ => NoteTarget::Note(0),
        }
    }

    /// The note the note editor shows (its copy for a detached note).
    pub(super) fn shown_note(&self, t: usize) -> Option<Note> {
        match self.note_target(t) {
            NoteTarget::Note(n) => self.track(t)?.notes.get(n).cloned(),
            NoteTarget::Empty { .. } => None,
            NoteTarget::Detached => Some(
                self.selection
                    .detached
                    .as_ref()
                    .filter(|(dt, _)| *dt == t)
                    .map_or_else(Note::default, |(_, n)| n.clone()),
            ),
        }
    }

    pub(super) fn note_view(&self, t: usize) -> NoteView {
        let name = self.track(t).map_or("", |tr| tr.name.as_str());
        let note = self.shown_note(t);
        let title = match (self.note_target(t), &note) {
            (NoteTarget::Empty { beat, step }, _) => {
                format!("Note: {name} [beat {} step {}] — no note", beat + 1, step + 1)
            }
            (_, Some(n)) => format!("Note: {name} [beat {} step {}]", n.beat + 1, n.step + 1),
            _ => format!("Note: {name}"),
        };
        let Some(n) = &note else {
            return NoteView {
                title,
                note: None,
                scales: Choice::default(),
                arp_direction: Choice::default(),
                arp_range: 0,
            };
        };
        let state = editors::arp_state(n, &self.generators.scale_list);
        let names = self.generators.scale_list.names.clone();
        let scale = n.arp_scale.clone().unwrap_or_else(|| state.scale.clone());
        let scales = Choice { selected: editors::index_of(&names, &scale), values: names, labels: Vec::new() };
        let directions: Vec<String> = ARP_DIRECTIONS.iter().map(|d| (*d).to_owned()).collect();
        let direction = n.arp_direction.clone().unwrap_or_else(|| state.direction.clone());
        let arp_direction =
            Choice { selected: editors::index_of(&directions, &direction), values: directions, labels: Vec::new() };
        NoteView { title, note, scales, arp_direction, arp_range: state.range }
    }

    pub(super) fn kit_controls(&self) -> Option<KitControls> {
        let sample = self.kit_editor.sample?;
        let instrument = self.kits.sample(self.kit_index, sample)?;
        let names: Vec<String> = self.kits.kits().iter().map(|k| k.name.clone()).collect();
        let kits =
            Choice { selected: self.kit_index.min(names.len().saturating_sub(1)), values: names, labels: Vec::new() };
        Some(KitControls {
            kits,
            gain: instrument.gain_db,
            transpose: instrument.transpose,
            decay: instrument.decay.unwrap_or(0.0),
        })
    }
}
