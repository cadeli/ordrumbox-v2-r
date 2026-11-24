# Changelog

## Unreleased

### New
- **New kits rendered by the synth**, from new factory presets kept in the synth editor:
  `808_like`, `909_like` (`TR808_*`, `TR909_*`), `606_like`, `cr78_like`, `simmons_like`,
  `latin_perc`, `trap_808`, `minimal` (`TR606_*`, `CR78_*`, `SDS_*`, `LAT_*`, `TRAP_*`, `MIN_*`),
  `vintage60` (`SR601_*`, the early 1960s rhythm boxes, with organ bass, lead and pad) and
  `punch_synth` (`PUNCH_*`). A sound a kit lacks may be borrowed from them.
- **Kits 8bits and electro rendered again**: 8bits gains melodic sounds (bass, lead, piano, brass,
  organ, pad), electro two rototoms (LO_TOM, HI_TOM). The human kit has a new kick and snare.
- **Mixer** (F12, View > Mixer, the MIX button): a strip a track (echo and reverb sends, pan, mute,
  solo, fader, meter), a shared tape echo and spring reverb, and the master bus, in place of the
  master strip along the bottom.
- **Song automation**: lanes under the song's patterns for a track's mute, solo, volume, pan,
  cutoff, Q, sends, effects, transpose, probability and swing; drawn with the mouse or recorded
  from the mixer (REC). A control moved by hand takes its lane back until re-enabled.
- **Song loop**: drag on the song panel's ruler (Ctrl+L turns it off and on); the song panel zooms
  like a DAW timeline.
- **Edit > Humanize...**: notes a little early or late, softer or louder, at random.
- **Export loops**: a pattern's WAV or MIDI export asks how many loops it holds (1 to 64).
- **Where you left off**: the app starts on the pattern, arrangement, mode and view you left.
- A second orDrumbox started warns that one is already running.
- A double-click on a track's name opens the piano roll on it; on a song clip, its pattern.
- **Sample editing**: the start, the end, the loop's points and the end of the decay are
  dragged on the waveform (kit editor and Sound tab), and land on the nearest zero crossing.
- **Loop in / Loop out**: a loop of its own inside the sample's region; the sound plays its
  way in (its attack once), then loops. Kept in `kits.json` (`loopStart`, `loopEnd`, ms).
- **Grid lane**: a **PITCH** tab, a bubble with the value being painted or under the pointer, and a
  right-click menu to reset a step's notes, or all the track's, to the setting's default. Drag the
  lane's top edge to set its height (the menu's **Automatic height** undoes it).
- **Piano roll zones**: press on an empty cell and drag to select a zone (a plain click still adds
  a note). Ctrl+C, Ctrl+X, Ctrl+V and Delete take the zone's notes, and dragging one of them moves
  the whole zone in time and pitch (one undo step). Shift+drag still works.
- **Simple view** (View > Simple view): the grid alone, the track and note editors folded; again,
  both shown. The first start opens in it, saying how to show the rest; later starts open as
  the window was left.
- **Choke groups**: in the track editor's Sound tab, **Choke** puts a track in a group (1 to 8);
  a note of a track of the group cuts the sound of every track of it, as a closed hi-hat cuts an
  open one. A hi-hat track added or renamed joins group 1; the patterns already made are unchanged.
  MCP: `chokeGroup` in `updateTrack`.

### Changed
- New pattern (Ctrl+N) has a track for each instrument of the kit in use.
- The kits `real`, `open`, `ropen`, `punchy` and `vintage` are removed; the `matt` crash is now
  played by the synth (`CRASH_SYNTH`).
- The kit list follows the folders of `assets/kits/`: once you had edited a kit, new shipped kits
  and samples no longer appeared; they do now.
- The kit chosen is kept with the library and its projects, and comes back at the next start
  and with a project opened; choosing one is an undo step. In the plugin, the host's Kit
  parameter still chooses it.
- The project's name and description are undo steps, as a pattern's or an arrangement's.
- Deleting a synth preset is done at once: it was already an undo step, the confirmation said
  otherwise.
- Piano roll: dragging a note moves it, sideways in time and up / down in pitch, never onto
  another note (its velocity stays with the note editor and the grid's lane). A click on the
  selected note no longer deletes it: a double-click does, as Delete and the note's menu. The
  pointer shows what a drag does: sideways arrows on a note's right edge (its length), four
  arrows on its body (a move).
- Piano roll: the < > page buttons give way to a scroll bar under the grid, with the same thumb and
  zoom handles (Ctrl+wheel zooms, Shift+wheel scrolls). The roll opens on 4 beats as before.
- Grid: a scroll bar beside + NEW TRACK scrolls the beats shown and zooms them (drag the ends of its
  thumb, or Ctrl+wheel; Shift+wheel scrolls), in place of the fixed pages. The ruler says which beats
  are shown (`5-8/16`) instead of `page 2/4`. The view still follows the playhead and the cursor.
- The track editor is narrower (310 px instead of 360), leaving more room to the grid: the FX tab's
  compressor lays its knobs on two rows (Mix, Thresh, Ratio, then Attack, Release).
- A song file is now a **project**: File > Open project (Ctrl O), Save project, Save project as,
  Export project, Import project, Rename project and Project description, all in the File menu.
  A song is an arrangement, as before. The files stay `.odbox` in `songs/`: nothing to convert.
- A control taken back from its automation lane shows it (an orange mark), and its lane too (dim, with an
  orange OFF badge); a click on the badge, or Re-enable lane in the lane's menu, plays that lane alone again.
- The PATTERN list opens by genre, then the genre's patterns. A pattern's tags are edited on the
  song screen (the Tags line under its description) or with Pattern > Tags...
- Tags are genres or features (bass, chords, synth…): only genres become learned styles. The demo
  library's tags are cleaned (`default`, `song` and `sends` gone, `exp` now `experimental`).
- The help line at the bottom of the window follows the pointer: what a click, a drag and a
  right-click do on what is under it.
- Song panel: the left button acts, the right one opens the menus. A click on an empty cell adds
  the row's pattern; a click on a clip selects it (Delete removes it, Ctrl D repeats it).
- Under the pointer, a name that has a menu is drawn lighter with a `v` at its end, and a clip
  lighter; the context menus show the key of each line that has one.
- A seventh colour scheme, **High contrast** (Shift C): white text on black, every text at least
  7:1 against its ground.
- MCP: `saveProject`, `openProject`, `exportProject`, `listSavedProjects` (its list under `projects`)
  and `setProjectInfo`; the earlier `*Song*` names still work.

## 0.1.2

### New
- **Synth**: unison, modulation matrix, chip controls (pulse width, noise modes) and new factory presets.
- **Samples**: start, end, attack, hold, reverse, loop and gate.
- **Per-track sidechain**, and **parameter locks** (filter, reverb, delay) per note.
- **Quantize** (Edit > Quantize...) and **zone selection** in the grid.
- **MIDI**: input and output, MIDI learn, clock in and out, pattern choice by program change or note.
- **Stems export** of a pattern or a song (File menu, MCP).
- **Preferences** window (Ctrl ,), metronome, tap tempo.
- About twenty new demo patterns and a full demo song.
- Windows setup, CLAP plugin in the release, Linux build.
- VST3 plugin in the release, a separate GPLv3 zip for Windows, Linux and macOS.

### Changed
- New keyboard shortcuts (F1 lists them); PATTERN / SONG mode switch.
- MIDI import and export keep note lengths.
- The filter's highpass is now textbook: a little thinner and quieter.

## 0.1.1

### New
- **CLAP / VST3 plugin**: orDrumbox runs as an instrument in a DAW and follows the
  host's transport and tempo, with automatable parameters. It has 17 stereo outputs:
  the mix, plus one per track when its Output is set to *Own*.
- **Per-track compressor** (normal or parallel) in each track's effects.
- **Kit editor** (Ctrl+K): kit title and notes, sample table, gain, transpose and decay,
  analysis and envelope.
- **Chords**: add minor, major, 7th, suspended… chords from a cell.
- **Retrigger fade**: repeats step down in velocity (Fade slider, 0.6 by default).
- **Zoom and resizable window**; compact view.
- **Save status** in the menu bar (saved, modified, save failed…); Help > About shows
  the library's path.
- Instrument picker, toast messages, F1 lists every shortcut.

### Changed
- Auto-assign: a track missing from the kit now plays the synth preset of its name
  (e.g. `CLAP_SYNTH`) instead of borrowing another kit's sample.
- New colour scheme; track and note editors separated; reworked LFO and steppers.
- Shortcuts work on any keyboard layout (AZERTY included).

### Fixed
- MIDI import: sysex events no longer shift the notes after them; program changes in
  format 0 files are read; a second track of the same instrument is kept (`PERC 2`).
- The triangle LFO stays within its min–max range.
- Some bug fixes.

## 0.1.0

First release.
