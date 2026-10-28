# Tests JS d'ordrumbox-v2 face au portage Rust

Inventaire au 2026-10-28 (commit Rust `d84686d`, plus l'output panel,
les contrôles de l'en-tête et Compact tracks). Aucun test JS n'est lancé
ni porté tel quel : le portage se vérifie contre le **code source** JS, que
les scripts `tools/export_*_fixtures.mjs` exécutent pour produire les
fixtures que les tests Rust doivent reproduire.

Statuts :

- **Couvert** — la fonctionnalité est portée et un test Rust vérifie le même
  comportement (souvent plus finement, par fixture).
- **Partiel** — portée, mais le scénario du test JS n'est pas vérifié tel
  quel (souvent : la logique est couverte, pas le parcours dans l'interface).
- **Non porté** — la fonctionnalité n'existe pas encore en Rust.
- **Sans objet** — propre au navigateur (DOM, service worker, IndexedDB,
  écran d'accueil, mobile…).

## Specs e2e Playwright (`e2e/`, 35 specs, 175 tests)

| Spec | Tests | Statut | Détail |
|---|---|---|---|
| `auto-generation.spec.js` | 8 | Couvert | ↻ Drum/Bass/Chords et leur bascule : `generators.json` (toggles, notes, mode auto). Les boutons de barre d'outils sont des entrées de menu (D, S, A). |
| `auto-generation.mobile.spec.js` | 4 | Sans objet | Barre d'outils mobile. |
| `canvas-rendering.spec.js` | 1 | Partiel | Le spectre (output panel) est porté : la FFT et l'analyseur ont leurs tests (`spectrum.rs`), pas « des pixels non vides pendant la lecture ». |
| `cold-start.spec.js` | 3 | Partiel | Assignation des sons au premier lancement : `assign.json` ; premier tick : tests du `Controller`. L'AudioContext est sans objet. |
| `color-scheme.spec.js` | 4 | Non porté | Thèmes de couleur. |
| `gen-sliders.spec.js` | 14 | Partiel | Les curseurs de l'onglet gen sont portés ; les variantes sont couvertes par `generators.json`, mais pas l'effet de chaque `prob_*` à 0 et 100 %. |
| `kit-change.spec.js` | 2 | Partiel | Réassignation au changement de kit : `assign.json` ; pas de test « en pleine lecture ». |
| `layout.spec.js` | 3 | Sans objet | Disposition DOM (les tests Rust vérifient dessin et hit-test ensemble). |
| `live-vs-export.spec.js` | 2 | Partiel | Lecture et export partagent le même constructeur de passes ; `render.rs` teste l'export, pas l'égalité avec la lecture en direct. |
| `mobile-layout.mobile.spec.js` | 4 | Sans objet | Mobile. |
| `offline.spec.js` | 1 | Sans objet | Service worker. |
| `page-matrix.spec.js` | 1 | Sans objet | Pagination de la grille : la grille Rust montre tout le pattern. |
| `panels.spec.js` | 4 | Partiel | About et track editor (F2) portés, non testés ; Soft Synth non porté. |
| `pattern-grid-drag.spec.js` | 9 | Partiel | Glisser une note (vélocité, hauteur, jauge) non porté ; Shift+flèches porté (`nudge_*`). |
| `pattern-lifecycle.spec.js` | 5 | Partiel | Chaque étape existe et sa sortie est couverte (sauvegarde `library.json`, octets MIDI `midi.json`, octets WAV `wav.json`, fichier song `.odbox`) ; le parcours complet n'est pas testé. |
| `persistence.spec.js` | 3 | Partiel | Sauvegarde et rechargement : test `save_then_load` du store. Version de cache IndexedDB et `clearAllCache` : sans objet. |
| `piano_roll.spec.js` | 7 | Partiel | Glisser (vélocité, hauteur, axe verrouillé), clic qui sélectionne puis supprime, annuler en une étape, Shift+flèches : tests `session_roll` et `piano_roll.json` (`NoteDrag`). La jauge est une bulle de texte. |
| `piano-roll.mobile.spec.js` | 4 | Sans objet | Piano roll sur mobile. |
| `playback.spec.js` | 5 | Partiel | La tête de lecture avance : tests du `Controller` ; clic sur une case qui joue la note : non testé. Écran d'accueil et 404 : sans objet. |
| `playback_tempo.spec.js` | 3 | Partiel | Tempo du pattern en vue grille, de la song en vue song : porté, non testé au niveau session. Vue synthé : non portée. |
| `select_dropdown.spec.js` | 2 | Couvert | `Dropdown` : tests du panel (clavier, molette, clic, fermeture). |
| `song-arrangement.spec.js` | 34 | Partiel (élevé) | Commandes : `song_edits.json` ; menus, clics, Next/Delete/Add here/Delete row, annuler : tests `session_song` ; lignes, rectangles, règle, hit-test : tests `song_view`. Non vérifiés tels quels : curseur qui revient dans la grille en boucle, boucle d'animation arrêtée en quittant la vue. |
| `song-playback.spec.js` | 6 | Partiel | Clips superposés résolus ensemble : `song_sources.json` ; démarrage à une mesure et saut : test du `Controller`. Bascule de mode en changeant de vue : non testée. |
| `song-view.spec.js` | 4 | Partiel | Vue, liste d'arrangements, liste de patterns qui défile : portés. Différence : en Rust le track editor est masqué dans la vue song. |
| `song-wav-export.spec.js` | 5 | Partiel | `render_song` : test de longueur et de refus sans clip jouable (`offline.rs`) ; effet d'un second clip et tempo de la song : non testés. |
| `synth-modulators.spec.js` | 3 | Couvert (DSP) | Chaque cible de LFO : `renders.bin` (processeurs DSP). L'éditeur de synthé n'est pas porté. |
| `synth-params.spec.js` | 4 | Couvert (DSP) | Presets, formes d'onde, algorithmes FM, filtres : `renders.bin`, `synth_patches.json`. |
| `synth_group_titles.spec.js` | 1 | Non porté | Éditeur de synthé. |
| `synth_implicit_enable.spec.js` | 1 | Non porté | Éditeur de synthé. |
| `tools-import-export.spec.js` | 8 | Partiel | Import/export MIDI et export WAV (menus, fixtures d'octets), notes aléatoires (X) : portés. « Compact Tracks » : Pattern › Compact tracks, `compact.json`. « Exporting... » : sans objet. |
| `track-editor-loop.spec.js` | 3 | Couvert | Steps/beat et point de boucle, bornage du point de boucle : `edits.json`, `controls.json`. |
| `track-editor.mobile.spec.js` | 4 | Sans objet | Mobile. |
| `undo_redo.spec.js` | 3 | Couvert | `edits.json`, `song_edits.json` (tout annuler, tout refaire). |
| `viewport-fit.mobile.spec.js` | 1 | Sans objet | Mobile. |
| `waiting-screen.spec.js` | 9 | Sans objet | Écran d'accueil du navigateur. |

## Tests unitaires vitest (`tests/`, 159 fichiers, ~3100 tests)

### Couverts par les fixtures (même code JS exécuté par `tools/`)

| Sujet | Fichiers JS | Fixture(s) Rust |
|---|---|---|
| Format JSON, chargement, export | `exporter`, `pattern_fixer`, `defaults`, `model_schemas`, `serialize_roundtrip`, `roundtrip_integration`, `pattern_import_validation`, `multi_note_step` | `song.json`, `song.min.json`, `library.export.json`, `edge_library*.json`, `slugify.json` |
| Moteur de patterns | `pattern_engine`, `pattern_manager`, `euclidean`, `notes`, `variation2` | `flat_notes/` |
| Songs | `song_schema`, `song_playback`, `player_song_mode`, `song_roundtrip` | `song_sources.json`, `song_edits.json` |
| Commandes, historique | `cmd`, `cmd_update`, `cmd_transaction`, `history_manager`, `undo_roundtrip`, `arrangement_section` (partie commandes) | `edits.json`, `song_edits.json` |
| Générateurs | `generators`, `base_generator`, `pattern_auto_gen`, `song_structure`, `composition` | `generators.json`, `composition.json` |
| MIDI | `midi_exporter`, `midi_exporter_functional`, `midi_json_roundtrip`, `midi_import_service` | `midi.json` |
| Instruments, assignation | `instrument_manager`, `auto_assign`, `tracks`, `drumkits_validation` | `instruments.json`, `assign.json` |
| DSP | `worklet_synth_voice`, `worklet_master_bus`, `strip_sync`, `fx_values`, `lfo_engine`, `math` | `renders.{json,bin}`, `strip_settings.json`, `lfo_values.json`, `synth_patches.json` |
| WAV, analyse | `wav_encoder`, `sample_analyzer`, `hz_to_note` | `wav.json`, `analysis.json` |
| Contrôles | `base_control`, `or_knob_log`, `ui_utils` (formatage) | `controls.json` |

### Partiels (porté, non testé tel quel)

`song_panel`, `arrangement_section` (partie interface), `song_service`,
`ui_pattern_grid`, `keyboard_shortcuts`, `note_editor_sliders`,
`note_editor_layout`, `track_editor`, `track_editor_sliders`,
`track_editor_waveform`, `sound_section`, `generation_section_auto`,
`generation_section_lfo`, `lfo_velocity_sync`, `drumkit_manager_e2e`,
`drumkit_manager_autoassign`, `drumkit_service`, `audio_import_service`,
`select_lists`, `select_dropdown`, `sync_helpers`, `note_nudge`, `seq`,
`player_cache`, `voices`, `sound`, `wav_export_functional`,
`wav_export_transport`, `wav_midi_roundtrip`, `wav_analysis`, `wav_render`,
`e2e_user_flow`, `e2e_audio_comparison`, `export_section_midi`,
`random`, `numbers`, `pixel_font`, `view_switch`,
`about_panel_lifecycle`, `tools_panel_clear_rnd`, `output_panel_sliders`,
`undo_policy` (les réglages du master ne sont pas des étapes d'annulation,
comme en JS), `pattern_settings_panel` (ses contrôles sont dans l'en-tête), `piano_roll`, `ghost_sync`
(les fantômes : `sub_positions.json`).

### Non portés (fonctionnalité absente)

- Éditeur de synthé : `synth_editor`, `synth_editor_display`,
  `synth_editor_implicit`, `synth_editor_import`,
  `synth_editor_preset_section`, `synth_editor_sliders`, `synth_preset_model`.
- MIDI en direct : `midi_manager`, `midi_out`, `midi_indicator_view`.
- Thèmes : `color_scheme`, `mcp_color_scheme`.
- Glisser des notes sur la grille, onglets, outils : `pattern_grid_drag`,
  `or_tab`, `tools_panel_sliders`, `toolbar_overflow`,
  `playback_overlay_section`, `page_nav_e2e`.
- Serveur MCP : `mcp_tools`, `mcp_arrangements`, `mcp_alignment`,
  `mcp_disk_roundtrip`, `mcp_selection_playback`.

### Sans objet (navigateur, architecture JS)

IndexedDB et cache : `idb`, `idb_cache`, `resources_loader`.
Service worker : `service_worker`, `sw_build_id`. Mobile :
`mobile_panel_display`, `mobile_tab_bar`, `device_detection`,
`or_knob_touch`, `panel_visibility_matrix`. Web Audio et worklets :
`audio_graph`, `node_pool`, `strip`, `stall_detector`, `synth_voice_pool`,
`synth_pool_offline_race`, `worklets_loader`, `simplebeep_audio`,
`startup_audio`, `transport_scheduler`, `wav_export_silent_retry`.
Démarrage et câblage JS : `bootstrap_*`, `service_registry`,
`decoupling_boundaries`, `module_graph`, `base_panel_lifecycle`,
`lifecycle`, `granular_events`, `playback_events`, `cmd_mixin_contract`,
`cmd_selection`, `app_state_selected_pattern`, `lfo_ui_bridge`,
`p1a_silent_guards`, `p2_concurrency`. DOM et notifications :
`cls_prevention`, `html_escaping`, `toast`, `logger`, `download`,
`midi_exporter_download`, `ui_toolbar`.

## Ce qui manquerait le plus

Les scénarios e2e des fonctionnalités **déjà portées** qui ne sont vérifiés
nulle part en Rust : la lecture (clic sur une case qui joue la note, tempo
selon la vue, bascule de mode en changeant de vue), le changement de kit en
pleine lecture, l'égalité lecture directe / export WAV, l'effet des curseurs
`prob_*` à 0 et 100 %, l'export WAV d'une song (tempo, second clip), et le
parcours complet de `pattern-lifecycle`. Ils peuvent devenir des tests au
niveau de la `Session` ou du `Controller`.
