# ordrumbox-v2 (JS) tests against the Rust port

> **Archive.** This inventory dates from the port (2026-10-28) and is no longer
> kept up to date test by test. Since 2026-11-06, the JS app is no longer the
> reference: the `tools/*.mjs` scripts and all the expected outputs produced by
> the JS were deleted. The Rust tests check Rust snapshots (`*_rust.txt`,
> `*_renders.bin`), tables and properties; the list of snapshots and their
> inputs is in `DEVELOPMENT.md`, "Snapshots". Updated on 2026-11-11: names of
> the current fixtures, and features ported since (themes, MCP server, live
> MIDI in the plugin, synth editor, dragging in the piano roll).

Inventory as of 2026-10-28 (Rust commit `03003a7`, plus the scenario tests
ported since: `scenarios.rs`, `gen_sliders.rs`, `user_flow.rs`,
`session_scenarios.rs`). No JS test is run or ported as is. At the time of the
inventory, the port was checked against the JS **source code**, which the
`tools/export_*_fixtures.mjs` scripts ran to produce fixtures; these fixtures
have since been replaced by Rust snapshots.

Statuses:

- **Covered**: the feature is ported and a Rust test checks the same
  behaviour (often more finely, per fixture).
- **Partial**: ported, but the scenario of the JS test is not checked as is
  (often: the logic is covered, not the path through the interface).
- **Not ported**: the feature does not exist yet in Rust.
- **Not applicable**: specific to the browser (DOM, service worker, IndexedDB,
  splash screen, mobile…).

## Playwright e2e specs (`e2e/`, 35 specs, 175 tests)

| Spec | Tests | Status | Detail |
|---|---|---|---|
| `auto-generation.spec.js` | 8 | Covered | ↻ Drum/Bass/Chords and their toggle: snapshot `generators_rust.txt` (inputs `generator_cases.json`: toggles, notes, auto mode). The toolbar buttons are entries of the Generate menu (D, S; Chords has no key). |
| `auto-generation.mobile.spec.js` | 4 | Not applicable | Mobile toolbar. |
| `canvas-rendering.spec.js` | 1 | Partial | The spectrum (output panel) is ported: the FFT and the analyzer have their tests (`spectrum.rs`), not "non-empty pixels during playback". |
| `cold-start.spec.js` | 3 | Covered | Each track has a sound before the first playback, the first pass sounds: `scenarios.rs` (engine), assignment table of `assign_tests.rs` (ui). The AudioContext is not applicable. |
| `color-scheme.spec.js` | 4 | Partial | Six themes (View › Next colour scheme, C), saved in `settings.json`; unit tests of `theme.rs`. |
| `gen-sliders.spec.js` | 14 | Covered | `gen_sliders.rs` (core): each `prob_*` in isolation, never any effect at 0, at least once at 100; `pitch_range`, `pitch_scale_lock`, `prob_note_depth` (floor 0.2: test of `variation.rs`). The sliders themselves: tables of the `control.rs` tests. |
| `kit-change.spec.js` | 2 | Covered | Kit change during playback: all auto tracks switch to the new kit, playback continues and the next passes sound (`scenarios.rs`, engine). |
| `layout.spec.js` | 3 | Not applicable | DOM layout (the Rust tests check drawing and hit test together). |
| `live-vs-export.spec.js` | 2 | Covered | `scenarios.rs` (engine): playback and export identical to the sample for callbacks that are multiples of 128 frames, within the JS test's tolerances otherwise; export of a synth pattern audible on every render. |
| `mobile-layout.mobile.spec.js` | 4 | Not applicable | Mobile. |
| `offline.spec.js` | 1 | Not applicable | Service worker. |
| `page-matrix.spec.js` | 1 | Not applicable | Grid pagination: the Rust grid shows the whole pattern. |
| `panels.spec.js` | 4 | Partial | About, track editor (F2) and synth editor (F6) are ported; these toggles are not tested as is. |
| `pattern-grid-drag.spec.js` | 9 | Partial | Dragging a note on the grid is not ported: the lane under the grid sets velocity, chance, retrig and pan by click or drag; Shift+arrows ported (`nudge_*`). |
| `pattern-lifecycle.spec.js` | 5 | Partial | Condensed path in the session (`session_scenarios.rs`): pattern built in the grid, saved and reloaded identically, exported to MIDI (only the solo track, tempo, 4/4, paired note-on/off) and to WAV (length, sound). Not taken over: each track, note and synth setting through its control. |
| `persistence.spec.js` | 3 | Partial | Save and reload: `save_then_load` test of the store. IndexedDB cache version and `clearAllCache`: not applicable. |
| `piano_roll.spec.js` | 7 | Partial | Drag (velocity, pitch, locked axis), click that selects then deletes, undo in one step, Shift+arrows: `session_roll` tests and `note_drag.rs` (`NoteDrag`). The gauge is a text bubble. |
| `piano-roll.mobile.spec.js` | 4 | Not applicable | Piano roll on mobile. |
| `playback.spec.js` | 5 | Covered | `session_scenarios.rs`: Space starts, the playhead advances, Space stops; a click on a cell (empty, note, track name) plays the note while stopped. Splash screen and 404: not applicable. |
| `playback_tempo.spec.js` | 3 | Covered | `session_scenarios.rs`: pattern tempo in the grid (and changed during playback), unchanged and without restart when going through synth and piano roll, song tempo in the song view, back to the pattern. |
| `select_dropdown.spec.js` | 2 | Covered | `Dropdown`: panel tests (keyboard, wheel, click, closing). |
| `song-arrangement.spec.js` | 34 | Partial (high) | Commands: `edit_properties.rs` and unit tests of `editor.rs`; menus, clicks, Next/Delete/Add here/Delete row, undo: `session_song` tests; rows, rectangles, ruler, hit test: `song_view` tests. Not checked as is: cursor going back to the grid in a loop, animation loop stopped when leaving the view. |
| `song-playback.spec.js` | 6 | Covered | Overlapping clips: tables of `instruments.rs` (core) and the render (`scenarios.rs`); mode following the view, advancing measure by measure: `session_scenarios.rs`; each pattern of the arrangement has its sounds: `scenarios.rs`. |
| `song-view.spec.js` | 4 | Partial | View, arrangement list, scrolling pattern list: ported. Difference: in Rust the track editor is hidden in the song view. |
| `song-wav-export.spec.js` | 5 | Covered | `scenarios.rs` (engine): length at the song's tempo (120 and 240 bpm), 16-bit stereo header, sound present, a second clip changes more than half the samples, refusal without a playable clip, shortened arrangement loop. The button: Ctrl+Shift+E. |
| `synth-modulators.spec.js` | 3 | Covered (DSP) | Each LFO target: `synth_renders.bin` (DSP processors). The synth editor is ported (F6); its model: `synth_editor_rust.txt`. |
| `synth-params.spec.js` | 4 | Covered (DSP) | Presets, waveforms, FM algorithms, filters: `synth_renders.bin`; patches: Rust snapshot `mapping_rust.txt`. |
| `synth_group_titles.spec.js` | 1 | Not applicable | Title position in the DOM. |
| `synth_implicit_enable.spec.js` | 1 | Covered | `implicit_enable_and_cards` (`synth_model.rs`) and the editor's session test. |
| `tools-import-export.spec.js` | 8 | Partial | MIDI import/export and WAV export (menus, byte fixtures), random notes (X): ported. "Compact Tracks": Pattern › Compact tracks, `edit_properties.rs` (what plays does not change). "Exporting...": not applicable. |
| `track-editor-loop.spec.js` | 3 | Covered | Steps/beat and loop point, clamping of the loop point: `edit_properties.rs`, tests of `editor.rs`, tables of `control.rs`. |
| `track-editor.mobile.spec.js` | 4 | Not applicable | Mobile. |
| `undo_redo.spec.js` | 3 | Covered | `edit_properties.rs` (random sequences: each undo, undo everything, redo everything). |
| `viewport-fit.mobile.spec.js` | 1 | Not applicable | Mobile. |
| `waiting-screen.spec.js` | 9 | Not applicable | Browser splash screen. |

## vitest unit tests (`tests/`, 159 files, ~3100 tests)

### Covered by Rust snapshots and tables

| Subject | JS files | Rust fixture(s) |
|---|---|---|
| JSON format, loading, export | `exporter`, `pattern_fixer`, `defaults`, `model_schemas`, `serialize_roundtrip`, `roundtrip_integration`, `pattern_import_validation`, `multi_note_step` | Rust only: round trip and idempotence on `song.json` and `edge_library.json` (`song_json.rs`), snapshots `library_rust.json`, `edge_library_rust.json`; slugs: table of `ids.rs` |
| Pattern engine | `pattern_engine`, `pattern_manager`, `euclidean`, `notes`, `variation2` | Rust only: snapshot `flat_notes_rust.txt` (passes 0–3 of `song.json` and `engine_cases.json`), properties of random passes (`flat_notes.rs`), tests of `pattern_engine.rs` |
| Songs | `song_schema`, `song_playback`, `player_song_mode`, `song_roundtrip` | Rust only: tables of played clips (`instruments.rs`); `measure_at_tick`: tests of `song.rs` |
| Commands, history | `cmd`, `cmd_update`, `cmd_transaction`, `history_manager`, `undo_roundtrip`, `arrangement_section` (commands part) | Rust only: `edit_properties.rs`, tests of `editor.rs` |
| Generators | `generators`, `base_generator`, `pattern_auto_gen`, `song_structure`, `composition` | `generators_rust.txt` (inputs `generator_cases.json`); progressions: `composition_rust.txt` |
| MIDI | `midi_exporter`, `midi_exporter_functional`, `midi_json_roundtrip`, `midi_import_service` | `midi/*.mid`, `midi_exports.json`; expectations in a Rust snapshot (`midi_rust.txt`) |
| Instruments, assignment | `instrument_manager`, `auto_assign`, `tracks`, `drumkits_validation` | Rust only: lookup tables, GM notes and programs (`instruments.rs`), assignment per kit, shipped kits and imported kits (`assign_tests.rs`, ui) |
| DSP | `worklet_synth_voice`, `worklet_master_bus`, `strip_sync`, `fx_values`, `lfo_engine`, `math` | `effect_renders.bin`, `synth_renders.bin`; strip settings, LFO and synth patches: Rust snapshot `mapping_rust.txt` (`mapping.rs`) |
| WAV, analysis | `wav_encoder`, `sample_analyzer`, `hz_to_note` | WAV: tests of `wav.rs` (header, round trip); analysis: Rust only, sine waves of known frequency and amplitude, and snapshot `analysis_rust.txt` of the edge cases (`analysis.rs`) |
| Controls | `base_control`, `or_knob_log`, `ui_utils` (formatting) | tables of the `control.rs` tests |
| Scenarios | `e2e_user_flow`, `wav_midi_roundtrip` (MIDI round trip), `e2e_audio_comparison` | `user_flow.rs`, `scenarios.rs`, tests of `wav.rs` |

### Partial (ported, not tested as is)

`song_panel`, `arrangement_section` (interface part), `song_service`,
`ui_pattern_grid`, `keyboard_shortcuts`, `note_editor_sliders`,
`note_editor_layout`, `track_editor`, `track_editor_sliders`,
`track_editor_waveform`, `sound_section`, `generation_section_auto`,
`generation_section_lfo`, `lfo_velocity_sync`, `drumkit_manager_e2e`,
`drumkit_manager_autoassign`, `drumkit_service`, `audio_import_service`,
`select_lists`, `select_dropdown`, `sync_helpers`, `note_nudge`, `seq`,
`player_cache`, `voices`, `sound`, `wav_export_functional`,
`wav_export_transport`, `wav_analysis`, `wav_render`,
`export_section_midi`,
`random`, `numbers`, `pixel_font`, `view_switch`,
`about_panel_lifecycle`, `tools_panel_clear_rnd`, `output_panel_sliders`,
`undo_policy` (master settings are not undo steps, as in JS),
`pattern_settings_panel` (its controls are in the header), `piano_roll`,
`ghost_sync` (the ghosts: Rust snapshot `sub_positions_rust.txt`),
`synth_editor`, `synth_editor_display`, `synth_editor_implicit`,
`synth_editor_import`, `synth_editor_preset_section`, `synth_editor_sliders`,
`synth_preset_model` (the model: Rust snapshot `synth_editor_rust.txt`).

### Ported since the inventory (not checked test by test)

- Live MIDI: `midi_manager`, `midi_out`, `midi_indicator_view`. The CLAP /
  VST3 plugin receives and sends the MIDI of each track (`ordrumbox-plugin`,
  `host_sync.rs` in the engine).
- Themes: `color_scheme` (six themes, `theme.rs`). `mcp_color_scheme`: no
  MCP theme tool.
- MCP server: `mcp_tools`, `mcp_arrangements`, `mcp_alignment`,
  `mcp_disk_roundtrip`, `mcp_selection_playback`. Library tools:
  `ordrumbox-logic/src/tools.rs` (unit tests); the others: `session_mcp.rs`;
  the stdio bridge: `ordrumbox-app/tests/mcp_bridge.rs`, `mcp_afrobeat.rs`.

### Not ported (feature missing)

- Dragging notes on the grid, tabs, tools: `pattern_grid_drag`, `or_tab`,
  `tools_panel_sliders`, `toolbar_overflow`, `playback_overlay_section`,
  `page_nav_e2e`.

### Not applicable (browser, JS architecture)

IndexedDB and cache: `idb`, `idb_cache`, `resources_loader`.
Service worker: `service_worker`, `sw_build_id`. Mobile:
`mobile_panel_display`, `mobile_tab_bar`, `device_detection`,
`or_knob_touch`, `panel_visibility_matrix`. Web Audio and worklets:
`audio_graph`, `node_pool`, `strip`, `stall_detector`, `synth_voice_pool`,
`synth_pool_offline_race`, `worklets_loader`, `simplebeep_audio`,
`startup_audio`, `transport_scheduler`, `wav_export_silent_retry`.
JS startup and wiring: `bootstrap_*`, `service_registry`,
`decoupling_boundaries`, `module_graph`, `base_panel_lifecycle`,
`lifecycle`, `granular_events`, `playback_events`, `cmd_mixin_contract`,
`cmd_selection`, `app_state_selected_pattern`, `lfo_ui_bridge`,
`p1a_silent_guards`, `p2_concurrency`. DOM and notifications:
`cls_prevention`, `html_escaping`, `toast`, `logger`, `download`,
`midi_exporter_download`, `ui_toolbar`.

## What would be missing most

The e2e scenarios of the ported features are now checked at the level of the
engine (`ordrumbox-engine/tests/scenarios.rs`), of the pattern engine
(`ordrumbox-core/tests/gen_sliders.rs`), of the logic
(`ordrumbox-logic/tests/user_flow.rs`) and of the `Session`
(`ordrumbox-ui/src/session_scenarios.rs`, with a real `Controller` rendered
like an audio callback). Remaining: the full `pattern-lifecycle` path control
by control, and dragging notes on the grid.

Gap noted while porting `live-vs-export`: `Engine::render` splits each
callback into quanta of 128 frames starting at its beginning. With a callback
that is not a multiple of 128 (300 frames, for example), the strip parameters
are no longer read at the same frames as in the export: maximum difference of
0.04 on one sample, under the JS test's tolerances.
