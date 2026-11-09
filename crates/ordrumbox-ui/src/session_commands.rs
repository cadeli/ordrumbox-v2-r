//! The menu bar: what each menu shows (checked, greyed out) and what each
//! command runs.

use super::*;

impl Session {
    /// Generation on (tracks in auto mode): drums, bass, chords.
    fn generation(&self) -> [bool; 3] {
        let generating = |kind: fn(InstrumentFamily) -> bool| {
            self.pattern()
                .is_some_and(|p| p.tracks.iter().any(|t| generate::is_auto_on(t) && kind(instrument_family(&t.name))))
        };
        [
            generating(InstrumentFamily::is_drum),
            generating(|k| k == InstrumentFamily::Bass),
            generating(|k| k == InstrumentFamily::Piano),
        ]
    }

    /// What the menus show now.
    pub(super) fn menu_state(&self) -> MenuState {
        let history = self.editor.history();
        let pattern = self.pattern();
        let focus = self.focus_track().and_then(|t| self.track(t)).map(|t| (t.auto_mode, t.mute, t.solo));
        MenuState {
            can_save: self.saving.library_path.is_some(),
            can_render: self.render.is_some(),
            has_pattern: pattern.is_some(),
            undo: history.next_undo().map(str::to_owned),
            redo: history.next_redo().map(str::to_owned),
            has_cursor: self.selection.cursor.is_some(),
            focus_track: focus,
            can_remove_track: focus.is_some() && pattern.is_some_and(|p| p.tracks.len() > 1),
            clipboard_step: matches!(self.selection.clipboard, Some(Clipboard::Step(_))),
            clipboard_track: matches!(self.selection.clipboard, Some(Clipboard::Track(_))),
            has_selection: self.selection.selected.is_some(),
            playing: self.playing(),
            has_songs: !self.editor.library().songs.is_empty(),
            generation: self.generation(),
            auto_all: self.pattern_auto(),
            locked: self.focus_track().and_then(|t| self.track(t)).is_some_and(|t| t.compose_lock),
            kits: self.kits.kits.iter().map(|k| k.name.clone()).collect(),
            kit_index: self.kit_index,
            kit_editor: self.kit_editor.is_some(),
            track_editor: self.chrome.show_track_editor,
            note_editor: self.chrome.show_note_editor,
            grid: self.workspace == Workspace::Grid && self.kit_editor.is_none(),
            show_vus: self.chrome.show_vus,
            show_toasts: !self.flash.hidden,
            song_view: self.song_view_open(),
            piano_roll: self.piano_roll_open(),
            synth: self.synth_open(),
            can_save_song: self.saving.songs_dir.is_some(),
            saved_songs: self
                .saving
                .songs_dir
                .as_ref()
                .is_some_and(|d| store::list_songs(d).is_ok_and(|s| !s.is_empty())),
            kit_sample: self.kit_list().is_some_and(|(s, n)| s < n),
            checking_updates: self.updates.checking(),
            newer_release: self.updates.newer_release().map(|r| r.version.clone()),
            hosted: self.hosted,
            has_demo: self.has_demo(),
            user_folder: self.user_folder().is_some(),
        }
    }

    pub fn menus(&self) -> Vec<menu::Menu> {
        menu::menus(&self.menu_state())
    }

    /// The open menu, if any.
    pub fn open_menu(&self) -> Option<menu::Open> {
        self.chrome.menu
    }

    pub fn set_menu(&mut self, open: Option<menu::Open>) {
        self.chrome.menu = open;
    }

    /// File > Quit was chosen.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Run a menu command (or its shortcut).
    pub fn run(&mut self, command: Command) {
        use Command as C;
        self.chrome.menu = None;
        if let Some(refusal) = command.hosted_refusal().filter(|_| self.hosted) {
            self.message = refusal.into();
            return;
        }
        match command {
            C::Save => self.save(),
            C::ImportMidi => self.open_picker(Purpose::ImportMidi),
            C::ExportWav => self.open_export(Export::PatternWav),
            C::ExportSongWav => self.open_export(Export::SongWav),
            C::ExportMidi => self.open_export(Export::PatternMidi),
            C::Quit => self.quit = true,
            C::Undo => self.undo(),
            C::Redo => self.redo(),
            C::CopyStep => self.copy(false),
            C::CopyTrack => self.copy(true),
            C::Paste => self.paste(false),
            C::PasteTrack => self.paste(true),
            C::ClearStep => self.delete_step(),
            C::ToggleAccent => self.toggle_accent(),
            C::Deselect => self.deselect(),
            C::NewPattern => self.new_pattern(),
            C::DuplicatePattern => self.duplicate_pattern(),
            C::RenamePattern => self.text_entry(Id::SongPattern(self.pattern_index)),
            C::PatternDescription => self.text_entry(Id::PatternDescription),
            C::PreviousPattern => self.step_pattern(-1),
            C::NextPattern => self.step_pattern(1),
            C::TempoDown => self.step_tempo(-1.0),
            C::TempoUp => self.step_tempo(1.0),
            C::LengthDown => self.change_length(-1),
            C::LengthUp => self.change_length(1),
            C::AddTrack => self.add_track(),
            C::ChooseTrack => self.add_track_menu_at_button(),
            C::RemoveTrack => self.remove_track(),
            C::NextGrid => self.next_grid(),
            C::ShortenLoop => self.shorten_loop(),
            C::CompactTracks => self.compact_tracks(),
            C::ClearPattern => self.clear_pattern(),
            C::GeneratePattern => self.regenerate_pattern(),
            C::GenerateDrums => self.toggle_generation(None),
            C::GenerateBass => self.toggle_generation(Some(MelodicKind::Bass)),
            C::GenerateChords => self.toggle_generation(Some(MelodicKind::Chords)),
            C::TrackAuto => self.toggle_auto(),
            C::RandomTrack => self.randomize_track(),
            C::PlayPattern => self.toggle_play(),
            C::PlaySong => self.toggle_play_song(),
            C::PreviousSong => self.step_song(-1),
            C::NextSong => self.step_song(1),
            C::Mute | C::Solo => {
                if let Some(t) = self.focus_track() {
                    self.toggle_row(t, if command == C::Mute { "mute" } else { "solo" });
                }
            }
            C::NewGeneratedPattern => self.generate_pattern(),
            C::AutoAll => self.toggle_pattern_auto(),
            C::LockTrack => self.toggle_lock(),
            C::Kit(i) => self.use_kit(i),
            C::PreviousKit => self.step_kit(-1),
            C::NextKit => self.step_kit(1),
            C::KitEditor => self.toggle_kit_editor(),
            C::TrackEditor => self.toggle_track_editor(),
            C::NoteEditor => self.toggle_note_editor(),
            C::SongView => self.toggle_song_view(),
            C::PianoRoll => self.toggle_piano_roll(),
            C::Grid => self.show_grid(),
            C::SynthEditor => self.toggle_synth(),
            C::NewSong => self.song_button("new"),
            C::RemoveSong => self.song_button("delete"),
            C::RenameSong => self.text_entry(Id::SongName),
            C::SongDescription => self.text_entry(Id::SongDescription),
            C::SaveSong => self.song_button("save"),
            C::SaveSongAs => self.song_button("saveas"),
            C::LoadSong => self.song_button("load"),
            C::ExportSong => self.song_button("export"),
            C::ImportSong => self.song_button("import"),
            C::NormalizeKit => self.kit_normalize(),
            C::ExportMapping => self.open_export(Export::KitMapping),
            C::ExportKitFolder => self.open_export(Export::KitFolder),
            C::LoadMapping => self.open_picker(Purpose::LoadMapping),
            C::AddSample => self.open_picker(Purpose::AddSample),
            C::ReplaceSample => self.open_picker(Purpose::ReplaceSample),
            C::ImportFolder => self.open_picker(Purpose::ImportFolder),
            C::SelectRandomPattern => self.select_random_pattern(),
            C::SelectRandomDrumkit => self.select_random_drumkit(),
            C::ToggleVus => self.toggle_vus(),
            C::ToggleToasts => self.toggle_toasts(),
            C::NextColorScheme => self.next_color_scheme(),
            C::ZoomIn => self.zoom(1),
            C::ZoomOut => self.zoom(-1),
            C::ZoomReset => self.zoom(0),
            C::ZoomToFit => self.zoom_to_fit(),
            C::ExportPatternJson => self.open_export(Export::PatternJson),
            C::ConvertToSynth => self.convert_to_generated_sounds(self.pattern_index),
            C::AutoAssignAll => self.auto_assign_all_tracks(self.pattern_index),
            C::RandomSamples => self.assign_random_samples(self.pattern_index),
            C::PreviewTrack => {
                if let Some(t) = self.focus_track() {
                    self.audition_row(t);
                }
            }
            C::Shortcuts => self.chrome.overlay = Some(Overlay::Shortcuts { scroll: 0 }),
            C::About => self.chrome.overlay = Some(Overlay::About),
            C::Manual => self.open_manual(),
            C::FactoryReset => self.chrome.overlay = Some(Overlay::FactoryReset),
            C::ExportLibrary => self.open_export(Export::Library),
            C::ImportLibrary => self.open_picker(Purpose::ImportLibrary),
            C::DemoLibrary => self.ask(Pending::DemoLibrary),
            C::BackUpUserData => self.open_export(Export::UserData),
            C::RestoreUserData => self.open_restore(),
            C::ChangeUserFolder => self.open_user_folder_picker(),
            C::DefaultUserFolder => self.use_default_user_folder(),
            C::OpenUserFolder => self.open_user_folder(),
            C::CheckUpdates => self.check_updates(true),
            C::DownloadPage => self.open_download_page(),
        }
    }
}
