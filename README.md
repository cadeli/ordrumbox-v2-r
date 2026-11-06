# orDrumbox (Rust)

Native port of [ordrumbox-v2](https://github.com/cadeli/ordrumbox-v2), the web
drum machine. Target: a single Windows `.exe` (developed on macOS, built for
Windows by CI).

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `library.json` codec, pattern engine, song clips, instrument catalog, JSON, RNG, JS number formatting | none |
| `ordrumbox-logic` | the library being edited: commands with undo/redo; pattern generators (drum lines from a style, styles learned from the library's tags, bass and chord variants, auto mode loop by loop); Standard MIDI Files (read, import, export) | core |
| `ordrumbox-dsp` | sample voice, synth voice, channel strip (filter, saturation, reverb, delay), master bus; no allocation in render paths | none |
| `ordrumbox-engine` | WAV decoding/encoding, kits and their files, sample analysis, sound auto-assignment, synth presets, passes (pattern and song: events, strip settings, LFO steps), sample-accurate engine, UI ↔ audio link, offline renders | core, dsp |
| `ordrumbox-probe` | sound measurements of synth presets and WAV files, a command-line tool that does not ship with the app | core, dsp, engine |
| `ordrumbox-ui` | the UI without a window or a sound device: session, software-rendered views (text in the system monospace font, at its natural letter spacing: 6 × 8 pixel cells per character at scale 1), input, MCP link, update check | core, logic, engine, fontdue |
| `ordrumbox-app` | window, input translation, audio output → `ordrumbox.exe` | core, engine, ui, winit, softbuffer, cpal |
| `ordrumbox-plugin` | the CLAP and VST3 instrument (below, "Plugin") | engine, ui, nih-plug, baseview, softbuffer |
| `xtask` | `cargo xtask bundle`: the plugin bundles | nih-plug's bundler |

The domain crates and the probe have no third-party dependency. Only the app talks to
the OS, through small bridges (`winit`, `softbuffer`, `cpal`; no MIDI device yet); the UI
rasterises text with `fontdue`.

## Running

The app reads its `assets` folder, part of this repository: `data/library.json`
(the demo patterns and arrangement), `data/kits.json` (the kits and their
samples), `data/scales.json`, `data/synth_presets.json`, `data/styles.json`
(the styles the drum generation follows, below) and `kits/**.wav`. It
looks, in order, in `$ORDRUMBOX_ASSETS`, in `assets/` next to the program, in
`assets/` in the working folder, then in this repository's `assets/` (so
`cargo run` works from anywhere in the checkout).

The demo `library.json` is only the starting point: the library being edited is
saved (one second after each edit, and on quit) to `library.json` in the user
data folder — `%APPDATA%\orDrumbox` on Windows, `~/Library/Application
Support/orDrumbox` on macOS, `~/.local/share/ordrumbox` elsewhere, or the file
named by `$ORDRUMBOX_LIBRARY` — and loaded from there next time.

The user data folder from the File menu:

- **Export library...** writes the library to `ordrumbox-library.json` in the
  folder picked, as the app saves it; **Import library...** reads such a file
  (or any `library.json`) in the place of the library, and **Load the demo
  library...** the assets' one: each is one step of Edit > Undo.
- **Back up user data...** copies the user's files — `library.json`,
  `kits.json`, `settings.json`, `synth_presets.json` (and an older
  `generated_sounds.json`), `samples/` and `songs/` — to a new folder
  `orDrumbox-backup-<YYYYMMDD-HHMMSS>` (UTC) in the folder picked. `exports/`,
  `backups/` and `mcp.json` are not the user's work and are left out.
  **Restore user data...** picks such a folder: the user's files there are
  moved to `backups/<date>-before-restore/` first, then the backup's take
  their place and the app loads them, as at start. Help > **Factory reset...**
  moves them to `backups/<date>-reset/` the same way, and starts again from
  the assets: Restore user data brings them back.
- **Change user data folder...** uses another folder from now on: the user's
  files are copied there when it holds none (the current folder is left as it
  is), else the ones it holds are used. The choice is noted in
  `location.json` in the default folder; **Use the default user data
  folder...** removes it. A folder chosen that is not there any more (a drive
  unplugged) is said at start, and nothing is saved until it is back or
  another is chosen. `$ORDRUMBOX_LIBRARY`, when set, decides instead, and the
  folder cannot be changed from the app.
- **Open user data folder** shows it in the Finder or the Explorer. Help >
  About says which folder is used and where that comes from.

A plugin has none of the user data folder commands but the library ones.

Key names: the files this app writes (`library.json`, `kits.json`, pattern
exports) name their settings after what they hold, in camelCase, and differ
from the JS app's (old name in brackets):

- track: `volume`/`volumeLfo` (`velocity`/`velocityLfo`), `transpose`/
  `transposeLfo` (`pitch`/`pitchLfo`), `samplePath` (`sampleId`),
  `autoAssign` (`useAutoAssignSound`), `useSynth` (`useSoftSynth`),
  `synthPreset` (`synthSoundKey`), `loopLength` (`loopAtStep`),
  `filterCutoff`/`filterCutoffLfo` (`filterFreq`…), `filterSlope` (12 or
  24 dB/oct; new, the JS filter has 12 only), `delayFeedback` (new, 0.4 in
  the JS), `reverbMix`,
  `delayMix`, `saturationMix` (`reverbAmount`, `delayDepth`,
  `saturationAmount`), `delayBeats` (`delayTime`), `saturationOn` (`sat`),
  `hitVariation`/`noteVariation` (`variation`/`variation2`); auto mode:
  `autoMode` (`auto`), its chances (0–100) `autoPitchChance`,
  `autoVelocityChance`, `autoSilenceChance`, `autoFillChance`,
  `autoGhostChance`, `autoRetriggerChance`, `autoEuclideanChance`,
  `autoArpChance`, `autoNoteProbabilityChance`/`autoNoteProbabilityRange`
  (`prob_pitch`… `prob_note_depth`), `autoVariant`, `autoDensity`,
  `autoHitVariation`, `autoNoteVariation`, `pitchRange`, `pitchScaleLock`
  (`auto_variant`…); the drum generation (new, saved only when set):
  `composeHitsPerMeasure`, `composeComplexity`, `composeSyncopation`,
  `composeCoupling`, `composeVariation` (unset: the style's),
  `composeStyleWeight`, `composeSeed`; `fxSelected` (an editor tab) is dropped;
- note: `step` (`beatStep`), `cycle`/`cycleOffset` (`every`/`pos`),
  `probability` (`prob`), `retriggerRate` (`rate`), `retriggerProbability`
  (`arpTriggerProbability`), `euclideanPulses` (`euclideanFill`),
  `arpScale`, `arpDirection` (`_arpScale`, `_arpType`), and an arpeggio's
  `direction` (`mode`);
- pattern: `autoGenre`, `autoKeyOffset`, `autoScale` (`_autoGen…`), no
  more `application`/`url` header; song: `loopMeasures`
  (`loopMeasureCount`), a clip's `patternId` (`pattern`); library: `info`
  (`infos`), `selectedPattern`/`selectedSong` (`…Idx`);
- kit: `samples` (`instruments`), and for a sample `path`, `displayName`,
  `transpose` (`url`, `display_name`, `tune`).

The old names are still read, so the JS assets and older saves load as
before; the JS app does not know the new ones and drops those settings from
a file written here.

Sounds: a track with auto-assign gets a sample of the selected kit by name,
then of another kit, then a catalog substitute, then any sample (each a
random pick among the candidates, made at play and on a kit change); a synth
track without a known preset gets a random one.

WAV exports (16-bit, 44.1 kHz, the pattern once or the whole song, no tail) go
to the folder picked when exporting. Every export asks for its folder: the last
one picked, at first `exports/` next to `library.json` if it exists, else the
home folder; **Enter** on `[ Use this folder ]` writes there. A file of the same
name is replaced only once confirmed.

What removes or replaces work asks first (**Enter** goes on, **Esc** or a click
cancels): deleting a track, a clip, a row, an arrangement, a kit's sample or a
synth preset; clearing a track or a step of several notes; random notes over a
track's notes; compacting tracks; loading or importing a song. A single note goes
at once. While a question is open, the MCP tools are refused.

Generation, in three levels. **The header**: **STYLE** (the 14 styles, the
learned ones, or random — picking one makes the pattern again in it), **↻**
(**B**: the pattern made again in its style — its tracks, meter, tempo and
lines; a track the style names keeps its sound and settings, the others go),
**AUTO** (every track in auto mode, or out of it), and three knobs over the
whole pattern, 50 changing nothing: **ENERGY** (the hits and densities, ×0.5
to ×1.5), **COMPLEX** (the complexity and syncopation of every drum part, −100
to +100) and **VARY** (the variation of the lines and of the play, none to
twice). The knobs make the lines again at once from the pattern's seed
(`composeSeed`, drawn by ↻): the same values make the same lines.
**Shift+B** makes a new pattern in the chosen style. **The Gen tab** of a
track: Auto, **Lock** (↻, the knobs and auto mode leave the track as it is),
↻ for this track alone, and Hits, Complexity and Variation for a drum track
(Variant and Density for a bass or chord track). **Advanced**, folded: the
manual, Groove, Engine and full Compose settings. **D**, **S**, **A** (the
Generate menu) still make the drum, bass or chord lines alone, in auto mode.

A track in auto mode (name in green, **Z** toggles it) follows the song form
as it plays (intro, chorus, verse, break…): a line is new at the start of a
section (the drums thinner in quiet sections), a drum line then moves a few
hits from loop to loop (its variation) and the style's fill part fills its
last loop; bass and chord lines hold through the section. Those lines are
saved but are not undo steps. Exports regenerate them on their own copy.
**X** puts random notes on the focused track.

Styles: `assets/data/styles.json` holds 14 (techno, house, drum & bass,
hip-hop, rock, funk, disco, reggae, swing jazz, bossa nova, samba, salsa,
afrobeat, trap) and their aliases (`rap`, `dnb`, `ragga`…). A style gives its
meter (`stepsPerBeat`, `beatCount`: jazz and afrobeat are ternary, bossa and
salsa run over two measures for their clave), `tempo`, `swing`, `tracks`, the
`melodic` variants and chord `progression`, a `fill`, and for each drum part
`weights` (one per step of the measure: how typical a hit there is),
`required`, `forbidden` and `accents` steps, `hitsPerMeasure`, `complexity`,
`syncopation`, `coupling`/`couplingTo`, `variation`, `ghost`, `rolls` and
`chance` (that a new pattern has the track). A value that cannot be read is
reported on the status line with its path, and takes its default.

A drum line: each step of the measure weighs the style's weight against the
plain meter (`composeStyleWeight`), flattened toward the weak steps by the
complexity, tilted by the syncopation, raised or lowered where the coupled
part plays; exactly the asked hits are drawn in each measure, required steps
first, for 16 candidate lines, and the one whose syncopation (Longuet-Higgins
and Lee) sits where asked among them is kept. In the Gen tab a drum track's
settings show the track's value, else the style's (ending in `*`; a
double-click gives a value back to the style), and `4→6` when the header's
knobs make it play 6; a seed above 0 (Advanced) makes the same line every time.

Learned styles: the patterns tagged with a style's name (or alias) move it
toward what they play — each part's weights and hits, the required steps, the
swing — in proportion to the measures seen (8 count as much as the written
profile); a tag that names no style becomes a style of its own, its forbidden
steps those its patterns never play. Fewer than 4 measures teach nothing. The
Generate menu lists them with the patterns and measures they rest on.

MIDI files (ported from the JS exporter and import): **Ctrl M** writes the
pattern as a type 1 file (96 PPQN; drums on channel 10 at their GM keys,
melodic instruments on their own channels at the sample's root note) to a
folder picked. A `.mid` dropped on the window becomes new 32-beat patterns (up to
16), one track per instrument found (by GM program, drum key, then track
name), in one undo step. As in the JS app, the file's tempo is not read
(120 BPM). Departures from the JS: a file timed in SMPTE frames or with a
0 division is refused, the message says when notes past beat 512 (the
16th pattern) are left out, and a drum key named by its GM name after an
instrument another key plays keeps its notes on a numbered track ("Acoustic
Bass Drum" beside "Bass Drum 1" is `KICK 2`). The export writes the notes of
one tick note-offs first, then by key (the JS order of simultaneous note-offs
was whatever its sort left); it leaves out a note whose velocity rounds to 0
(the JS wrote a note-on at velocity 0, which MIDI reads as a note-off: the
triangle velocity LFO, which goes below its `min`, makes such notes); and it
writes track names in UTF-8 (the JS kept one byte per character), which the
import reads, or Latin-1 when a name is not UTF-8.

The window keeps the same three parts whatever is edited: one main editor in
the top-left (the grid, the piano roll, the song panel, the synth editor, or
the kit editor over any of them — the **VIEW** buttons and the View menu pick
one), the track editor down the right edge (**F2** shows or hides it), and the
output panel as a strip along the bottom, always shown. The window cannot be made
smaller than the layout needs (about 1100 × 690 at 100 %); the track rows
share the grid's height, so a taller window gives bigger cells. The UI scales
by whole steps: on a display at 150 %, which rounds to 2, a window too small
to hold the layout at 2 shows it at 1.

Kit editor (**Ctrl K**, the JS drumkit manager): the current kit's samples
(**↑↓**), and for the selected one its kit and instrument (lists), its gain
(dB), transpose (semitones) and decay (knobs), its note, length, peak and RMS
levels and envelope with the decay end; **Enter** plays it, **Delete**
removes it, **N** normalizes the kit to 0 dB, **E** exports the kit mapping
(`.json`). Dropped on the window, a
`.wav` joins the kit (**Shift**: replaces the selected sample), a folder of
WAV files becomes a kit, a kit mapping is loaded. Gain and transpose now play (the
JS sample voice uses them). Unlike the JS app, where these edits are lost on
reload, the kits are saved to `kits.json` and the audio of added, replaced or
normalized samples to `samples/` (32-bit float WAV), both in the user data
folder; `samples/` wins over the assets' `kits/`. A file added or imported is
copied, as it came, to `samples/imported/<kit>/` (the kit's name with
characters other than letters, digits, `_` and `-` as `_`), so two kits'
files of the same name stay apart; the folder it came from is not read again. Delete `kits.json` to start
again from the assets' kits.

Track editor (on the right, **F2** or **View › Track editor** shows or hides
it), for the track of the cursor or of the selected note: the sample bar
(note, length, peak, envelope with the decay end, **Load** replaces the
sample), the knob bar (volume, pan, transpose, the sample's decay) and the JS
tabs — **FX** (reverb, delay, saturation, filter, each with its on/off LED;
see "Effects" below),
**Sound** (mono, auto-assign, instrument, sample, synth preset; a track made or
renamed as a bass, such as `BASS` or `SYNTH BASS`, is set mono), **LFO** (an
LFO per setting: the target list and its LED, wave, frequency, range, phase), **Loop** (steps per
beat, loop length, swing), **Gen** (auto, lock, ↻ and three settings;
Advanced: the manual variation settings, or the Groove / Engine settings in
auto mode, and Compose on a drum track).
In auto mode, **Var Hits** (Groove) and **Var Notes** (Engine) are the
variation amounts the other sliders' chances draw on (16 units at 100: a
velocity or pitch change costs 1, a silence, fill or ghost 3); the generator
picks them while a track has none (a drum line 12–25 and 0, a bass or chord
line 0 and 0) and keeps them after; the header's VARY knob scales them. Variant
lists the bass variants on a bass track, the chord and melody ones on the others. Under it, the note editor: the
selected note (else the step under the cursor, else the track's first note)
— velocity, pitch, pan, and the Trigger, Retrigger, Euclidean and Arpeggio tabs. Settings are named in whole words (Transpose, Feedback, Hit Variation…), the text at the font's own letter spacing leaving room for them. While
playing, a setting with an LFO shows the LFO's value. The grid has a volume
slider per track, the same setting as the track editor's Volume knob (their tooltips say so). Right-click a track's name: rename, copy, paste, move up or
down, duplicate, delete, randomize or clear it, or add a track; Rename track
offers the instrument menus below, plus **Free name...**, which asks for any name; drag a track's name up or down
to move the track to another row (one undo step). Right-click a step: copy,
paste or delete its notes, or add a note at a random pitch. A note fills its cell as high as it is loud;
from velocity 0.9 it is an accent, topped with a bright bar. **L** accents
the notes of the cursor step (velocity 1), or, when they all are, sets them
back to 0.8 (one undo step). A ruler above the rows numbers the beats, the one
playing lit, with a dot per step of the focused track; **+ NEW TRACK** under
the rows adds a track: like **Pattern > Add track...** and **Add track** in a track's
right-click menu, it opens a menu of instrument categories, then one of the
category's instruments, which names the track. A submenu opens beside its menu,
which stays shown: a click on another line of it, **Left**, **Backspace** or
**Escape** goes back to it (**Right** or **Enter** opens a submenu). **Ctrl+T** adds a KICK track at once, and a track's name shows in a tooltip its
whole sound, mix and length. Above the track names, the ruler says which page of beats is shown when the pattern has more than one; the header's list is labelled PATTERN. In the kit editor, each sample names the pattern's tracks that play it. Problems found at start (a sample not loaded, a file that cannot be read) stay on a red band above the help line until quit.

The mouse over a tab, a knob, a slider or a setting's label of the track,
note and kit editors, or over a header control, shows its whole name and what
it does (Retrigger: repeats of the note, how fast, their chance…).

Knobs and sliders work as in the JS (`OrKnob`, `OrSlider`, checked against
them): drag a knob up or down (200 px for the whole range), click or drag a
slider; **Shift** drags finely (sliders too, which the JS range input cannot);
the mouse wheel and the arrow keys (or **- =**) step the value (**Shift** or
**Alt**: a tenth of a step, **Ctrl**: ten steps); a double-click restores the
default; a right-click (or **Enter**) asks for a number. **Tab** moves the
keyboard focus through the controls, **Esc** gives the keys back to the grid.
A list opens on a click (or **Enter**): arrows, a letter to jump, the wheel,
**Enter** to choose, **Esc** to close.

Song panel (**F3**, **View › Song** or **Song › Song view**, the JS song
view; **Esc** goes back to the grid): in the place of the grid, the
selected arrangement across the whole width — a ruler of measures, one row
per pattern it uses (named on the left; a pattern gone from the library keeps
its row, in red), each clip a bar over its measures — and under it the
library's patterns on the left and on the right the song's name and
description (a click edits them), its date, the arrangement list, and
**Save**, **Load**, **Save as**, **Export**, **Import**. A click on the ruler aims the cursor, where the song plays from
(**←→** move it a measure; while playing, it jumps there at once); while the
panel is shown **Space** plays the song. Right-click a pattern of the list:
add it at the playhead (stopped: the cursor), move it up or down the list,
rename it, or delete it with its clips (one undo step); a row's name: add its
pattern at the playhead, move the row up or down, or remove its row; a clip: **Next** (the same clip right after it) or **Delete**; the bare
grid: add the row's pattern there. Drag a pattern along the list to move it
there; drag it onto the arrangement to add it at that measure, on its own row
(a tooltip shows the measure); drag a clip sideways to
move it, whole measures at a time (the wheel scrolls during the drag; the grid
shows a page past the end to drop on); drag a row by its name up or down to
move it. Rows follow the order of the pattern list, so a row moved moves its
pattern in the list too. A click on a pattern selects it, a
double-click adds it at the playhead (**↑↓** step through them). The wheel
scrolls the list, the pattern's description, and the arrangement's
measures, sideways too (**Shift**: the rows); the scroll bars under and right
of the grid, clicked or dragged, reach every measure and row; while the song
plays the grid follows it page by page. Every edit is an undo step, and moves the loop onto the end of the
last clip, as in the JS (`song_commands.js`, checked against it). **Save**
writes the song (patterns, arrangements, what is selected) to
`songs/<name>.odbox` in the user data folder, where the JS app uses
IndexedDB; **Load** lists them; **Save as** and **Export** write the same file
to a folder picked; **Import** (or a `.odbox` dropped on the window) reads one; loading
replaces the library in one undo step. Beyond the JS panel, **New** and
**Delete** add and remove arrangements — the JS `addSong` / `removeSong`
commands, which only its MCP tools reach. The **Song** menu has all of these
too, with or without the panel shown: play the song, new, remove, previous and
next arrangement, rename the song, its description; save, load, export and
import are in the **File** menu.

Piano roll (**F4**, **View › Piano roll**, the JS piano roll; **Esc**
clears the selection, then goes back to the grid): in the place of the
grid, the track editor's track (the list next to the title picks another)
on a grid of steps, a page of 4 beats at a time (**< >**, or **Shift** +
wheel), and pitches from MIDI 12 to 108 with a keyboard on the left (the
wheel scrolls it; it opens on the track's pitch). Notes are drawn by
velocity, a bar on top marks a probability (purple) or a cycle (blue), the
retriggers, arpeggio steps and euclidean fills are ghosts at their steps
and pitches, and while playing the playhead moves (the page follows) and
the notes sounding light up. A click on an empty cell adds a note, on a
note selects it (and plays it), on the selected note deletes it; dragging a
note changes its velocity (up, down) or its pitch (sideways, a semitone
every 12 px), the first movement choosing which, one undo step per drag,
its value shown over the note. A click on a key plays it. Right-click a key:
**Clear all**, or **Add sequence** — the track replaced by a chord
progression from that key, one chord per measure, the next progression
each time (I-V-vi-IV, I-vi-IV-V, …); a cell: **Add note** or **Delete
note**, **Add minor chord**, **Add major chord**. **Arrows** move the
cursor (wrapping; the note there is selected), **Shift + arrows** change the
selected note's velocity (up, down) and pitch (left, right), **Enter**
adds, selects or deletes at the cursor, **Delete** deletes the selected
note. The note editor follows the selected note. The ghosts, the
progressions and the drag are the JS code's (checked against it). Unlike
the JS, a drag never counts as a click: there the window's mouseup resets
the drag before the click arrives, so `consumeClick()` never sees it.

Synth editor (**F6**, **View › Synth**, the JS soft-synth editor; **Esc**
goes back to the grid): the preset of the track editor's track (else the
first one), in the place of the grid, the track editor beside it. Right of
the tabs, a line says what that track plays: "KICK plays this preset", or
"KICK plays a sample" / "plays synth "BASS_SQUARE"" with a **USE ON KICK** button
that puts the track on the preset shown (one undo step). The preset
bar picks another preset and deletes, creates, duplicates, reverts (to the
preset as it was when opened), exports (`ordrumbox-synth-sounds.json`, to a
folder picked) and imports presets. Four tabs of cards — **OSC** (the scope: four cycles of
the oscillators, with their FM, sub and drive; VCO1–3; FM), **FLT** (the
filter, 12 or 24 dB/oct (`filter.slope`, not in the JS editor), and its response curve, the filter envelope, the mod envelope),
**MOD** (LFO1, LFO2, noise), **ENV** (master, the envelope and its shape) —
each with its wave, filter type or FM algorithm choices, its power button,
its knobs, lists and switches, as the JS lays them out. A knob an LFO moves
is purple and follows it. Touching a control of a group that is off switches
it on (an oscillator to 0.5, FM to 0.3, the noise to 0.15, an LFO onto the
filter, the mod envelope onto the filter), as the JS does. Every change
plays at once and is saved half a second later to `synth_presets.json`
in the user data folder (an older `generated_sounds.json` there is read when
it is missing; the JS keeps them in IndexedDB), read at start over the
assets' presets: a saved preset wins over the asset of the same name, and an
asset the save does not hold is added (so a deleted asset preset comes back at
the next start). A save that cannot be read is reported and never written
over: preset edits then say they are not saved. The assets hold one preset per base sound of the kits:
`KICK_SYNTH`, `SNARE_SYNTH`, `TOM_SYNTH`, `CLAP_SYNTH`, `COWBELL_SYNTH`,
`CHH_SYNTH`, `OHH_SYNTH`, `CRASH_SYNTH`. The pills under the cards show the modules in use.

Output panel (a strip along the bottom of the window, always shown — the JS
output panel): the master bus in three tabs — **Volume**: the master
volume and the spectrum of what plays (the JS analyser's
`getByteFrequencyData`, computed as the Web Audio specification describes
it: 4096 samples, Blackman window, smoothing 0.8, -100 to -30 dB, the first
256 bins); **Comp**: the pre-gain, the compressor's transfer curve
(-60 to 0 dB in, the unity line, the threshold), its on/off button and its
threshold, ratio, attack, release, knee and make-up; **Filter**: the low cut
and the high cut. They play at once and are saved half a second after the
last change to `settings.json` in the user data folder (`settings.master`,
as the JS keeps them; the file's other settings are kept), and loaded at
start.

The header, under the menu bar, is a row of labelled groups after the logo
(the JS toolbar and its pattern settings panel): **PLAY** / **STOP**, **BPM**
(the tempo knob, 20–250 as the JS slider), **PATTERN** (the pattern list,
and where it is in the library), **BEATS** (1–16), **KIT** (the kit list),
**STYLE**, **↻**, **AUTO** and the **ENERGY**, **COMPLEX**, **VARY** knobs
(above) and **VIEW** (**GRID**,
**ROLL**, **SONG**, **SYNTH**, **KIT**: the main editor shown is lit, as
F3–F6 do; **KIT** toggles the kit editor, as Ctrl+K does).
A list opens on a click; the wheel or the arrows move, **Enter** picks. The
line under it shows the last action, undo and redo, and the song.
A pattern chosen while a pattern plays (in the list, with **[ ]**, or by a
plugin host) waits for the one playing to end its loop: until then **NEXT**
blinks on the beat above the list, the grid shows the new pattern without a
playhead, and the line under the header names the one finishing.

The look (`theme.rs`, every colour of the app): a deep indigo ground,
panels a step lighter, and neon accents with one role each — pink for
titles, the selection and slider handles, cyan for labels, sliders and
knobs, lime for play, what is on and the playhead, yellow for values, the
active tab and button; each track has its colour (pink, cyan, yellow,
violet, orange, lime, round again): its stripe in the grid, its notes (by
velocity), its notes in the piano roll, its clips in the song panel. Flat
colours, no glow. This is a redesign: the JS app looks otherwise. **Pattern › Rename pattern...** and **Pattern
description...** edit the pattern's name and description; **Pattern ›
Compact tracks** (the JS tools panel's button) shortens every track whose
notes repeat to its shortest loop — the loop length set, the notes past it
removed, one undo step per track (`compactTrack`, checked against the JS).

Menus: the menu bar at the top of the window (File, Edit, View, Pattern,
Generate, Play, Song, Kit, Help) lists every command with its shortcut; commands that cannot
run are greyed out, on/off ones (auto modes, mute, solo, the current kit, the
kit editor) show a box. Every command that reads or writes a file (songs,
MIDI, WAV, samples, kit folders and mappings) is in **File**. Click a title, or press **F10** or **Alt** alone, then
**←→** between menus, **↑↓** between items, **Enter** to run, **Esc** to
close. **Help › Keyboard shortcuts** (**F1**) lists every key; **Help › About**
shows the build and the folders in use. A click on the status line (under
the controls) lists what was done since the app started, newest first; the
wheel, the arrows, **Page Up/Down** and **Home/End** scroll it.

Build: the window title, **Help › About**, the update messages and
`ordrumbox --version` name the build. A release, built from the commit tagged
`v<version>` with no local change, is its version alone (`0.1.0`); any other
build follows it with `dev`, the commit (`+dirty` with uncommitted changes)
and the commit's date (`0.1.1 dev f7b7e1b 2026-11-04`).
`crates/ordrumbox-ui/build.rs` reads them from git; built without git, it
is `dev unknown build`.

Updates: at start, at most once a day, the app asks GitHub for the latest
release of this repository (with the system's `curl`, in Windows 10 and
later and in macOS; the request carries only `User-Agent:
orDrumbox/<version>`). A newer one is shown in the status lines, and **Help ›
Download orDrumbox …** opens its page; nothing is downloaded or installed by
the app. **Help › Check for updates** checks now. `settings.json` keeps it
under `updates`: `"check": false` stops the check at start, `lastCheck` is
when it last ran (seconds since 1970).

AI clients (MCP): an MCP client (Claude Desktop, Claude Code, Cursor…) can
drive the running app, as the JS app's `ordrumboxMcpserver.mjs` did its
files. The client starts `ordrumbox --mcp`, which speaks the Model Context
Protocol on its standard input and output and passes each tool call to the
window: the edit is shown at once, is one step of **Edit › Undo**, and is
saved with the library. The window listens on `127.0.0.1` only, on a port
it picks at start and writes to `mcp.json` in the user data folder with a
token a connection must give; with the window closed, a call answers that
orDrumbox is not running. In the client's settings (the path is the
program's, `ordrumbox.exe` on Windows):

```json
{ "mcpServers": { "ordrumbox": { "command": "/path/to/ordrumbox", "args": ["--mcp"] } } }
```

The tools keep the JS names: `listPatterns`, `loadPattern`,
`createNewPattern`, `selectPattern` (shows it in the window),
`addNotesToPattern`, `updateTrack`, `setPatternBpm`, `setPatternBeatCount`,
`setPatternTags`, `setPatternDescription`, `savePatternToJson` (to
`exports/`), `listAllInstrumentsNames`, `listKitSamples`, `analyzeSamples`,
`listArrangements`, `createArrangement`, `addPatternToArrangement`,
`removePatternFromArrangement`; and for the live app `selectKit`, `play`,
`stop`, `undo`. Beyond the JS server: `deleteNotes`, `removeTrack`,
`renamePattern`, `duplicatePattern`, `removePattern` (with its clips),
`updateArrangement`, `removeArrangement`, `redo`, `copyTrack`, `moveClip`,
`renameArrangement`, `exportPatternWav`, `exportPatternMidi`,
`exportArrangementWav` (to `exports/`), `importMidi`, `listStyles`,
`listScales`, `listSynthPresets`, `getMaster`, `setMaster` (saved with the
settings, not an undo step), `getSynthPreset`, `createSynthPreset`,
`updateSynthPreset`, `removeSynthPreset` (settings by path, `filter.freq`,
checked against the synth editor's controls; saved with the presets, not an
undo step). Pattern and track: `renameTrack`, `compactPattern`,
`auditionTrack`, `assignSounds` (kit, synth or random, every track).
Generation: `generatePattern`, `regeneratePattern` (in a style of
`listStyles`), `toggleGeneration` (drums, bass, chords), `togglePatternAuto`,
`setPatternMacro` (energy, complexity, variation), `regenerateTrack`,
`randomizeTrack`. Arrangements: `selectArrangement`, `repeatClip`. The song
file: `setSongInfo` (not an undo step), `saveSong` (to `songs/`),
`exportSong` (to `exports/`), `listSavedSongs`, `loadSong` (one undo step).
Kits, saved with them, not undo steps: `normalizeKit`, `addSampleToKit`,
`replaceKitSample`, `importKitFolder`, `loadKitMapping`,
`exportKitMapping` (to `exports/`). `getPlaybackState` tells what plays and
where. `setColorScheme` is not a tool: the colour scheme is the user's choice (**C**, `settings.json`). Track and
note settings take the key names of the files (`volume`, `transpose`,
`cycle`, `probability`…; the JS names are read too), and a value outside its
range is refused (notes) or brought inside it, the answer saying the value
used (tracks, tempo).

Commands ending in "..." (import a MIDI file or a song, add or replace a
sample, import a folder as a kit, load a kit mapping) open the built-in file picker: **↑↓**,
**PageUp/PageDown**, **Home/End** or the mouse wheel to move, a letter to jump,
**Enter** (or a second click) to open a folder or choose, **Backspace** for the
parent folder, **Esc** to cancel. It lists only the files the command takes;
on Windows the drives are listed above a drive's root. It starts in the home
folder, then where it last chose.

Under each track's name, the grid shows what it plays: its sample (`kit/file`,
as assigned, perhaps from another kit) or its synth preset (`synth BASS_FM`).

Mouse: a click switches a step on or off: an empty step gets a note (selected,
and played), a step with notes loses them all, in one undo step and without a
question; **Shift**+click selects the step's note for the note editor (and
plays it) without changing it; **M**/**S** boxes mute/solo; a click on
a name selects the track (and plays it). Over a note, a bubble shows its
pitch, MIDI number and the settings that differ from the defaults (the JS
`formatNoteTooltip`, checked against it).

| Keys | |
|---|---|
| **Space** | play / stop the pattern (**Shift**, or in the song panel: the song, from its cursor) |
| **[ ]** | previous / next pattern (**Shift**: song) |
| **- =** | tempo −1 / +1 (**Shift** ×10) |
| **, .** | previous / next kit |
| **B** | make the pattern again in its style |
| **Shift B** | a new pattern in the chosen style |
| **F**, **Shift G** | a random pattern, a random kit |
| **Shift H** | every track on the synth patch that matches its name, like auto-assign (KICK → KICK_SYNTH, SNARE → SNARE_SYNTH…; random when none does) |
| **Shift J** | every track back on auto-assign |
| **Shift K** | a random sample (of any kit) on every track |
| **G**, **H**, **J**, **K** alone | nothing but a reminder to hold **Shift**: these change every track, and sit next to the play keys (**Q**–**P**, **1**–**9**) |
| **V** | VU meters on the track stripes (on / off) |
| **C** | the next colour scheme (Indigo, Amber, Deep sea, Crimson, Toxic; also **View › Next colour scheme**), kept in `settings.json` under `colorScheme` (1 to 5; the web version knows 1 to 3 and takes 1 for the others) |
| **D**, **S**, **A** | drums, bass, chords: generate in auto mode / auto mode off |
| **Z**, **X** | auto mode of the focused track, random notes on it |
| **1–9** | mute track 1–9 (**Shift**: solo), in every view |
| **Q–P** | play track 1–10, in every view (the kit editor keeps N, R, E) |
| **arrows** | move the grid cursor |
| **Enter** | like a click at the cursor |
| **Shift + ↑↓ / ←→** | velocity ±0.05 / pitch ±1 of the selected note |
| **Delete** | clear the cursor step |
| **Esc** | deselect |
| **Ctrl Z**, **Ctrl Y** (or **Ctrl Shift Z**) | undo, redo (50 steps) of the library's edits. What Ctrl Z undoes is done at once, without a question, and its message ends with *(Ctrl+Z: undo)*: removing a track, a pattern, an arrangement, a clip or a row, clearing a step or a track, random notes, compacting. Only what cannot be undone asks first: replacing the library (import, demo, song files), removing a kit's sample or a synth preset, restoring a backup, changing the user data folder. The master, the synth presets and the kits are saved as they change, outside the history: their messages end with *(not undoable)* |
| **Ctrl C / V** | copy / paste the cursor step (**Shift**: the track) |
| **Ctrl T**, **Ctrl Backspace** | add a track, remove the focused track |
| **Ctrl G**, **Ctrl L** | next grid (steps per beat), loop one step shorter |
| **Ctrl - =** | pattern length −1 / +1 beat |
| **Ctrl +** (**Ctrl Shift =**), **Ctrl Shift -**, **Ctrl 0** | zoom the UI in, out by a whole scale (×1 to ×4), back to the display's scale; also **Ctrl** with the keypad's **+ -** (**View › Zoom in**…). A window too small for the scale asked shows the largest that fits and says so. Kept in `settings.json` (`uiZoom`, steps from the display's scale) |
| **Ctrl N**, **Ctrl D** | new pattern, duplicate pattern |
| **Ctrl S** | save the library now, the line under the header saying where (**File › Save**); it is also saved on its own a moment after each edit |
| **Ctrl Shift S** | export the pattern as JSON (`ordrumbox-<name>.json` in a folder picked; the JS app's **Ctrl S**) |
| **Ctrl E** | export the pattern to WAV (**Shift**: the song) |
| **Ctrl M** | export the pattern to a MIDI file |
| **Ctrl K** | kit editor (open / close) |
| **Ctrl O** | import a MIDI file (file picker) |
| **Ctrl Q** | save and quit |
| **F10**, **Alt** | open the menus |
| **F1** | the list of keys (the wheel, the arrows, Page Up/Down, Home/End scroll it when it is longer than the window) |
| **F2** | track editor (shown / hidden) |
| **F3** | song panel (shown / hidden) |
| **F4** | piano roll (shown / hidden) |
| **F5** | grid (back to the main editor) |
| **F6** | synth editor (shown / hidden) |
| **Tab**, **Shift Tab** | focus the next / previous control of the editors |
| drop a `.mid` file on the window | import it (also a `.mid` given on the command line) |

Keys are read by position (like the JS app's shortcuts): on an AZERTY
keyboard, **[ ]** are the two keys right of **P**, **Q–P** start at **A**,
**A** and **Z** are **Q** and **W**.
**PageUp/PageDown** and the keypad **- +** work too. **Cmd** works for
**Ctrl** on macOS.

## Commands

```sh
cargo run -p ordrumbox-app            # the app (debug, with console)
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo build --release -p ordrumbox-app  # target/release/ordrumbox(.exe)
# a pattern (2 loops) or a song rendered offline to a WAV file, with its speed
cargo run --release -p ordrumbox-engine --example render -- assets rock 2 rock.wav
cargo run --release -p ordrumbox-engine --example render -- assets song:demo demo.wav
```

Windows `.exe`: CI builds it for a release tag only, or when the workflow is
run by hand (Actions > CI > Run workflow, on any branch): artifact
`ordrumbox-windows-x64`, the program with its `assets` folder next to it.
A local build needs the `assets` folder copied next to it.

A release: set `version` in the root `Cargo.toml` (`0.2.0`), commit, then
`git tag v0.2.0 && git push origin v0.2.0`. CI checks that the tag is the
version and that the program built names itself by that version alone (not
as a dev build), then publishes a GitHub release with
`ordrumbox-v0.2.0-windows-x64.zip`; the apps already installed find it
through their update check.

Locally from
macOS: `cargo install cargo-xwin`, then
`cargo xwin build --release -p ordrumbox-app --target x86_64-pc-windows-msvc`.

## Plugin

orDrumbox also builds as an instrument plugin, CLAP and VST3: no input, a
stereo output. It follows the host's transport: it plays and stops with it,
plays the pass the host's position falls in (a loop of the pattern, or a
measure of the song) and plays at the host's tempo, not the pattern's. The
library is saved in the host's project; a new instance starts from the demo
library. The update check and the MCP link are off in the plugin.

```sh
cargo xtask bundle ordrumbox-plugin --release                 # target/bundled/orDrumbox.clap
cargo xtask bundle ordrumbox-plugin --release --features vst3 # and orDrumbox.vst3
```

Each bundle holds `assets/` in `Contents/Resources/`; on Windows, the
`.clap` is one file and `assets/` is written next to it. Install:

- macOS: `orDrumbox.clap` into `~/Library/Audio/Plug-Ins/CLAP/`,
  `orDrumbox.vst3` into `~/Library/Audio/Plug-Ins/VST3/`;
- Windows: a folder `orDrumbox` holding `orDrumbox.clap` and `assets` into
  `C:\Program Files\Common Files\CLAP\`, `orDrumbox.vst3` into
  `C:\Program Files\Common Files\VST3\`.

The plugin looks for its assets in `$ORDRUMBOX_ASSETS`, in its bundle, next
to its binary, then in this repository. For a release tag, or when the
workflow is run by hand, CI builds both bundles on Windows and macOS (artifact
`ordrumbox-plugin-<os>`) and runs `clap-validator` on the CLAP.

The plugin's window is the app's UI (`ordrumbox-ui`). It edits the project's
library; kits, synth presets and settings are the user's files, shared with
the app. What the host does is greyed out in its menus, tagged *host's*
(File > Save and Quit, Tempo -1/+1) or *app only* (Help > Check for updates
and Factory reset, the user data folder commands, the Song menu), and its
shortcuts are refused with a message: the host saves the library with the
project and sets the tempo. The header shows the host's tempo, under HOST, in
place of the pattern's BPM knob; its tooltip says so. A plugin plays patterns
only: the SONG view button is greyed out (a click says why), and there are no
song files (the songs of the library stay in it, for the app). It
has no master strip either: the host mixes, and the master bus keeps its
default settings, not those of `settings.json`.

The plugin's keys leave the host's alone. Space (the host's transport) and
every Ctrl/Cmd key (its save, undo, zoom...) go on to the host, even with a menu
or the file picker open; only a text entry keeps Space. **Alt** takes the
place of Ctrl for the plugin's own commands: Alt+Z undo, Alt+C and Alt+V copy
and paste, Alt+K the kit editor, Alt+N a new pattern, and so on; the menus show
these Alt shortcuts. The keys of the commands it leaves out (F3, Shift+[ ], -
and = for the tempo, Alt+S, Alt+Q) go to the host too, as does any key the
plugin does not use and every key release.
Help > Keyboard shortcuts lists the plugin's keys. A key passed on reaches the
host on macOS; on Windows, baseview does not pass it on, and what the host gets
depends on the host.

The host can automate these parameters (`ordrumbox-plugin/src/params.rs`; a
host saves automation under the ids, which never change):

| Id | Parameter | Values |
|---|---|---|
| `pattern` | the pattern played | 1–128: the library's order, shown with its name |
| `kit` | the kit used | 1–64: the kits' order, shown with its name |
| `volume` | a gain on the master volume | silence to +6 dB, 0 dB by default |
| `mute_1` … `mute_16` | track 1–16 of the pattern muted | off by default |

The pattern and the kit are applied by the player thread (within 10 ms) when
their value changes, and when a project is opened. A pattern or kit chosen in
the window is given back to the host as a change of the parameter (one past
128 patterns or 64 kits cannot be, and the window says so). A number past the
library's patterns or the kits is refused with a message, and applied once the
library has it, unless a choice is made in the window first. In song mode the
pattern is shown, the song plays on, and the window says so.

The volume and the mutes are read every block on the audio thread and go
through the engine's volume ramps (no click); a muted track is silenced, its
notes still play, its effect tails are cut. They add to the track's own mute,
solo and volume, which stay in the library. A track the host mutes has its
`M` box lit in orange (red: its own mute). The plugin's window has no master
strip, so the host's volume shows only in the host.

The names next to the pattern and kit numbers follow the library, but nih-plug
cannot tell the host that they changed: a host that keeps the texts it read
shows the old names until it reads them again.

## Sound measurements

`ordrumbox-probe` measures a sound: a preset rendered by the real synth voice
(before the channel strip and the master bus), a WAV file as it is, a whole kit,
or an exported mix.

```sh
cargo run --release -p ordrumbox-probe -- preset BASS_ACID --note C2 --gate 1
cargo run --release -p ordrumbox-probe -- preset --all --json before.json --svg pictures
cargo run --release -p ordrumbox-probe -- wav assets/kits/electro/acid-synth-c4.wav
cargo run --release -p ordrumbox-probe -- kit electro real --json kits.json
cargo run --release -p ordrumbox-probe -- mix rock.wav --bpm 120 --svg pictures
cargo run --release -p ordrumbox-probe -- compare before.json after.json
```

A preset note is held for one step (120 BPM, 4 steps per beat), as the app
plays it, unless `--gate` gives its length. `--help` lists the options.

- **Levels:** peak, RMS, DC, crest factor, frames at or over full scale, stereo
  balance.
- **Envelope:** attack (10% to 90% of the peak), time of the peak, decay to
  -20 dB, end at -60 dB, from a 10 ms RMS.
- **Spectrum:** energy centroid, 85% rolloff, flatness (tone near 0, noise
  higher), strongest peaks.
- **Pitch:** fundamental (YIN, 25 to 4000 Hz), its note, the level of its
  harmonics and of its lower octave.
- **Over time:** how far pitch and centroid move; the report holds their curves,
  the picture a spectrogram.
- **Bands:** share of the energy and level in sub, low, lowMid, mid, highMid and
  high (20, 60, 250, 500, 2000, 6000 Hz); a full-scale sine reads 0 dB.
- **Repeat (presets):** how much a later note differs from the first: `identical`
  means the same noise and LFO position on every hit.

`kit` reviews each folder of `assets/kits` (or `--kits`): a summary of its levels,
then the samples worth a look, with what is measured: frames at full scale, DC
over 0.02, last 5 ms louder than -40 dB from the peak, over 5 ms before the sound
starts, peak over 6 dB from the kit's median, over 25 cents from the note in its
name (`piano-c4`; another octave of that note does not count). A flag is not a
fault: a sample may be cut on purpose. Samples are named `kit/sample`.

`mix` measures an export without what only a single note has (attack, pitch):

- **Loudness:** integrated, momentary and short-term LUFS and loudness range
  (ITU-R BS.1770, EBU R128, EBU Tech 3342).
- **True peak:** with the points between samples, 4× oversampled; a sample peak
  at 1.000 with frames at full scale means the signal was clipped there.
- **Hits:** peaks of the spectral flux, each with its time, peak and main band;
  per band, their number and level spread. Notes that start together are one
  hit; a delay's echoes and a sample's own second burst count as hits, and a hit
  is timed where its sound starts, after any silence at the head of its sample.
- **Grid (`--bpm`):** how far hits sit from the steps. The median offset is the
  samples' own lead-in; swing, delays and retriggers leave the grid on purpose.

It cannot tell which sound masks which: that takes each track's own render.

`--json` writes the report and `compare` lists what changed between two reports,
sound by sound. Limits: a pitch frame lasts 85 ms, so a shorter pitch move is
blurred and a sound under 85 ms has no pitch curve. A pitch range near a multiple
of 1200 cents can be the detector jumping between octaves: check the curve.

## Effects

Each track plays through its strip: filter, saturation, reverb, delay, then
volume and pan; the master bus follows. They follow the JS processors, except
where a name did not hold what it said:

- **Filter**: LP, HP, BP, notch, 12 or 24 dB/oct (`filterSlope`; two stages,
  a Butterworth pair whose resonance peaks as high as one stage's). Off
  (`allpass`) skips it (the JS left a 20 kHz lowpass). Switched on from its
  20 Hz default, the cutoff goes to 1 kHz.
- **Saturation**: soft (tanh), hard (clip), tape (an asymmetric tanh: even
  harmonics; an `atan` in JS).
- **Reverb**: a Freeverb whose times follow the sample rate (the JS keeps the
  44.1 kHz lengths). `hall` and `plate` have a pre-delay, `spring` a
  dispersion, `gated` a large room cut 250 ms after the hit.
- **Delay**, in beats: `slap` is one clean echo, `tape` repeats darker with
  wow, `pingpong` starts on the left and alternates (the JS crossed two
  times); `delayFeedback` sets the repeats. Longer than 2 s at the tempo, it
  plays at the longest halving of its beats that fits, shown on the Beats
  choice.
- **Master bus**: a compressor, low and high cut, then a soft limit (unchanged
  under 0.9, rounded toward ±1; a hard clip in JS).

## Instrument catalog

`crates/ordrumbox-core/data/instruments.json` is the instrument catalog, built
into the program and edited by hand: everything the app knows of instrument
names is there, and `ordrumbox-core/src/instruments.rs` only applies it. It
started as an export of the JS catalog; it is this app's own now, and no
script writes it.

Each of `instruments`, in order (the first that fits wins):

| Key | What |
|---|---|
| `id` | `KICK`, `HI_TOM`… |
| `drum`, `melodic` | a drum sound (channel 10 in a MIDI file); a melodic one (with no sample of its own, a synth preset plays it) |
| `pan` | default pan, as written |
| `variantOf`, `qualifier` | the instrument it is a variant of and the qualifier that picks it (`TOM`, `HI`) |
| `synonyms` | name patterns, whole name, any case (`.` any character, `*` and `?` after an atom) |
| `words` | words read as its id: other names, short spellings, phrases (`RHODES`, `SHKR`, `BASS DRUM`) |
| `weakWords` | words that name it only when nothing else does (`STAB`) |
| `articulations` | ways of playing another instrument that name this one (`RIM`: "Snare Rim" is a rimshot) |
| `patchPrefixes` | synth patch name prefixes (`LD Supersaw`) |
| `subst` | what the auto-assignment takes when the kit lacks it, in order |
| `midi` | GM mappings: channel, name, key or program |

And at the top: `categories` (the "add track" menus, every id in one),
`qualifiers` (`LOW` is `LO`; MID wins, so "Low-Mid Tom" is the mid tom),
`openness` and `openClosed` (OPEN/CLOSED pick the hat; a GM name that says
neither is the closed one), `genericWords` (they count last, and some name an
id then: `BELL` a pitched bell, `808` a bass), `kitContext` (a kit's `CL.wav`
is the claves beside an `RS.wav`), `gmDrumNames`, `gmProgramNames`.

A name is read in this order: the id itself; a patch prefix, unless it is
also an id word or a pattern; a pattern of two words on the whole name
("Tubular Bells"); then the words but the generic ones, outside parentheses
and then inside ("Lead 8 (bass + lead)" is a lead): an articulation, an id or
an instrument's word, the GM name with the fewest other words ("conga low"
is the low conga), a weak word, the pattern with the most letters; then the
generic words. Digits do not count but in a word the catalog lists ("808"),
CamelCase is cut, and the qualifiers and OPEN/CLOSED only pick the variant of
what the other words found. In the auto-assignment, a substitute keeps to the
category of the instrument (a conga does not take a ride), and the grid shows
when a sample stands in (`for CONGAS: real/tom.wav`). The unit tests of
`instruments.rs` check that no word names two instruments and that every id
the catalog names exists.

One test follows the whole chain, `ordrumbox-ui/src/kit_import_tests.rs`:
five folders of WAV files named as drum machines and sample packs name them
(Roland's `BD`/`CL`/`RS`, short spellings, NI, Splice, Ableton) are imported as
kits, and 18 canonical tracks are played in each with the app's synth presets.
Its table of what each track gets, first and last candidate, and whether the
sample stands in, is written by hand: a change in it is a change of what a user
hears, to check by ear before the table is updated.

## Golden fixtures

The fixtures are committed: the tests need nothing outside this repository.
They were produced by the JS app (ordrumbox-v2), from which this one was ported,
with the scripts of `tools/`, which need a checkout of it only to make them
again. They hold the JS names of the keys; the tests compare after
`web_keys::current_keys` (`crates/ordrumbox-core/tests/support/`).

The JS app is no longer the reference: the fixtures only guard against
regressions, and are to be replaced over time by pure Rust tests. A test that
fails signals a change of behaviour; if it is not meant, the code is wrong. A
change meant on purpose moves the cases concerned to a Rust snapshot or a
property test, and is noted here, as the synth voice's departures are below.

`tools/export_fixtures.mjs` runs the JS app's code on
`assets/data/song.json` and writes `crates/ordrumbox-core/tests/fixtures/`:

```sh
node tools/export_fixtures.mjs ../ordrumbox-v2
```

- `song.json` / `song.min.json`: the JSON writer must reproduce both byte for byte.
- `library.export.json`: `song.json` loaded by the app (`fixPatterns`, `fixSongs`)
  and saved by its exporter; `Library` must hold the same values (compared
  after `web_keys::current_keys` (`crates/ordrumbox-core/tests/support/`), the JS key names being the web version's).
- `edge_library.json` (hand-written, kept by the script) and
  `edge_library.export.json`: the same check on cases `song.json` does not reach.
- `slugify.json`: pattern/song id slugs.
- `flat_notes/<id>.json`: `recomputeFlatNotes()` for passes 0–3 of every
  pattern of `song.json` and of `engine_cases.json` (hand-written, kept by the
  script), loaded the way the app boots (`fixPatterns` then
  `importPatternFromJson`). A pass is deterministic when three different
  `Math.random` streams give the same notes: Rust must give exactly those notes
  and draw no randomness. The other passes are checked by properties.

Re-run the script only when `song.json` changes; this app no longer follows
changes of the JS engine.

The audio processors have their own script, which runs the JS AudioWorklet
processors in Node (a minimal worklet scope, 128-frame quanta):

```sh
node tools/export_dsp_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-dsp/tests/fixtures/renders.{json,bin}`: strip, master bus and
  synth voice outputs (every preset of `generated_sounds.json`, every LFO
  target, FM algorithm and modulation target) for inputs both sides compute
  bit for bit. The Rust processors must match within 2e-5, except the synth
  cases that use what the voice does differently on purpose (below).
- `ordrumbox-dsp/tests/fixtures/synth_renders.bin`: those synth cases as the
  Rust voice renders them, one frame in 16. The voice departs from the JS
  processor on its sub (an octave below oscillator 1, no DC), its pitch punch
  (two octaves, not 24 cents), its FM (following the note, through zero) and
  its noise (each note its own); the patch's level also counts the noise in
  the mix. This file is the one fixture the JS app does not produce:
  `cargo test -p ordrumbox-dsp --test processors -- --ignored` writes it, only
  when the voice's sound changes on purpose.
- `ordrumbox-dsp/tests/fixtures/effect_renders.bin`: the same for the strip
  and master bus cases that use what they do differently on purpose (see
  "Effects"): the reverb at 48 kHz (its tunings follow the rate), the `tape`
  saturation, a master output above the limit's knee. The synth snapshot also
  covers its filter opening to 0.45 × the rate (a quarter in JS). The same
  command writes both files.
- The strip settings the JS lacks (24 dB/oct filter, pre-delay, dispersion,
  gate, wow, feedback tone, ping-pong input) default to what the JS does, so
  the fixtures play them unset; unit tests (`strip.rs`, `synth.rs`) measure
  them. `strip_settings.json` is checked on every track but those whose
  settings depart on purpose (a `gated` reverb, a sounding `slap`, `pingpong`
  or too long delay), which the unit tests of `strip_settings.rs` cover.
- `ordrumbox-engine/tests/fixtures/strip_settings.json`: the strip parameters
  `applyTrackToStrip()` sets for every track setup of `song.json`.
- `ordrumbox-engine/tests/fixtures/lfo_values.json`: `computeLfoValue()` over
  a range of ticks.
- `ordrumbox-engine/tests/fixtures/synth_patches.json`: the message
  `WorkletSynthVoice` sends to its processor for each preset.

The editing commands have a third script, which drives the JS `Commander`
and `HistoryManager`:

```sh
node tools/export_edit_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/edits.json`: 29 edits on `song.json`
  (notes, tracks, patterns), then undo all, then redo all; after each, the
  undo/redo sizes, the step description, the pattern ids and the saved
  patterns.
- `ordrumbox-logic/tests/fixtures/compact.json`: `compactTrack` on every
  track of every pattern of `song.json` and of hand-made patterns that
  repeat, loaded as the app boots (`importPatternFromJson`: every note has a
  `stepPercent`, which the comparison of notes includes): the steps recorded,
  then each track's loop point and notes.

Sound assignment, song playback and WAV encoding have a fourth script (the
instrument catalog is not generated: "Instrument catalog", above):

```sh
node tools/export_assign_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-core/tests/fixtures/instruments.json`: catalog lookups
  (`findById`, `findByName`, `findInstrumentFromFileName`, MIDI note and program).
- `ordrumbox-core/tests/fixtures/song_sources.json`: `resolveSongSources()`
  tick by tick for the demo song and a hand-made one.
- `ordrumbox-engine/tests/fixtures/assign.json`: `AutoAssign` for every track
  name in every kit, with `Math.random` pinned to the first and to the last
  candidate.
- `ordrumbox-engine/tests/fixtures/wav.json`: `bufferToWav()` bytes.

The detection and the auto-assignment depart from `findInstrumentFromFileName`
and `autoAssignTrackSounds`: their rules are this app's, written in the catalog
("Instrument catalog", above).

The names the JS read otherwise are listed with what this app reads:
`FILE_NAME_DEPARTURES` in `ordrumbox-core/tests/instruments.rs`, `DEPARTURES`
in `ordrumbox-engine/tests/assign.rs` (kit `real`; those names are left out of
the JS comparison). The MIDI import names GM key 44, "Pedal Hi-Hat", a CHHK
track, not a high bongo, and key 35, "Acoustic Bass Drum", `KICK 2`, not a
bass; its Rust snapshot holds these names (below). The
JS fixtures hold no synth preset, so the synth fallback is checked by unit
tests, like the rules themselves (`ordrumbox-core/src/instruments.rs`,
`ordrumbox-engine/src/assign.rs`). The fixture script reads the JS catalog
for the JS side of these comparisons; the app reads its own.

The generators have a fifth script; it also writes the variant tables the
port reads (`ordrumbox-logic/data/generators.json`):

```sh
node tools/export_gen_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/generators.json`, with `Math.random`
  replaced by a seeded stream (mulberry32, also in the Rust test): every
  variant of every generator on several track setups, densities and seeds;
  `generateRandom`. Rust must give the same notes, track settings and number of
  draws for the melodic lines and `generateRandom`. For what departs on purpose
  (below) the file holds the inputs and seeds only: the drum lines,
  `generatePattern` on new and existing patterns, the player's loop start over
  30–46 loops (auto tracks, `autoGen` patterns), the ↻ Drum / Bass / Chords
  toggles. The bass lines sound an octave above the JS ones, whose variants sat
  an octave too low: each bass variant's `rootNote` in `data/generators.json` is
  12 above the JS table (re-apply it after running the script), and the test
  compares the bass cases with the JS notes raised by 12.
- `ordrumbox-logic/tests/fixtures/generators_rust.txt`: what departs from the
  JS, from the same inputs and seeds, as the Rust generators make it: whole
  patterns, loops and toggles. The drum lines no longer come from the JS
  variant tables but from the styles (`assets/data/styles.json`, above): the
  script no longer writes the drum and genre tables into
  `data/generators.json`. Bass and chord lines hold through their section, with no
  variation and no chord progression within it. A track's variation budget is for the whole pattern,
  not for each of its loops (`ordrumbox-core/src/variation.rs`).
  `cargo test -p ordrumbox-logic --test generators -- --ignored` writes this
  file, only when what the generators make changes on purpose.
- `ordrumbox-logic/tests/fixtures/compose_rust.txt`: one pattern per style
  (two seeds), its drum tracks, as `crate::compose` makes them; the properties
  of every style (exact hits, required and forbidden steps, the signature
  figures, closeness to the weights, coupling) are in `tests/compose.rs`.
  `cargo test -p ordrumbox-logic --test compose -- --ignored` writes it, only
  when the drum lines change on purpose.

MIDI files no longer have a script; their tests are Rust only
(`ordrumbox-logic/tests/midi.rs`):

- `ordrumbox-logic/tests/fixtures/midi/`: the MIDI files read, hand-made ones
  covering the parser's paths (running status, sysex, truncation, garbage) and
  exports of song.json patterns, first written by the JS app.
- `ordrumbox-logic/tests/fixtures/midi_exports.json`: the patterns exported
  (every song.json pattern the engine plays the same with any random stream,
  and edge cases) with their loop counts (1 and 3), and the samples' root
  notes; inputs only.
- `ordrumbox-logic/tests/fixtures/midi_rust.txt`: what the parser reads of
  each file (events, names, note-ons, program changes), the patterns the
  import makes of it or its warning, and the exporter's bytes of each pattern.
  Until it was written, all three matched the JS app, apart from the import
  departures above; the export then left the JS where the departures above
  say (the order of simultaneous note-offs, which depended on V8's TimSort;
  silent notes; names in UTF-8). It also departs on its own: a note ends
  where its key is struck again on its track, before its 16th is out, and
  the notes of one key at one tick are one note at the loudest velocity, so
  no key is struck while it sounds; the tempo track carries the text event
  "Made with orDrumbox". Two tests check every export without the
  snapshot: the notes of one tick come note-offs first, then by key; and the
  file, accepted by a strict Standard MIDI File validator
  (`ordrumbox-logic/tests/strict_smf/`, written from the spec apart from the
  parser: header, chunks, deltas, running status, data bytes, sysex, the
  meta events and their ranges, the tempo map on the first track, one end of
  track, every note released), and signed by orDrumbox, holds on each track
  exactly the notes the pattern engine plays, pass after pass (tick, channel,
  key, velocity), no key struck while it sounds, each note ending a 16th
  later or where its key is struck again.
  `cargo test -p ordrumbox-logic --test midi -- --ignored` writes this file,
  only when the parser, the import or the export changes on purpose.

The kit editor's sample analysis has a sixth script:

```sh
node tools/export_kit_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-engine/tests/fixtures/analysis.json`: `Analyzer.analyzeWavBuffer()`
  (envelope, length, peak and RMS dB, fundamental and its confidence) and the
  note of WAV files in every format the JS decodes (tones, noise, decays,
  silence, very short files, stereo, the kick.wav fixture). Rust decodes the
  same bytes and must report the same values exactly.

The knobs and sliders have a seventh:

```sh
node tools/export_control_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-ui/tests/fixtures/controls.json`: the JS `OrKnob` and `OrSlider`
  driven through a minimal fake DOM — drags (normal and fine, linear and
  logarithmic), arrow keys with every modifier, typed numbers, the knob's arc
  — and the value texts of every formatter (`fmt`, `knobFormat`, `fmtVal`,
  `formatSignedDb`, the loop point label), and `formatNoteTooltip` for a set of
  notes and track pitches.

The piano roll has a ninth:

```sh
node tools/export_piano_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-core/tests/fixtures/sub_positions.json`: `getNoteSubPositions()`
  (the ghosts) on every track of `song.json` and on hand-made notes
  (retriggers at every rate, euclidean fills and rotations, loop points,
  every arpeggio form; a random arpeggio's pitches are drawn, so only its
  steps are compared).
- `ordrumbox-logic/tests/fixtures/composition.json`: `getSequence()` and
  `buildSequenceNotes()` for every progression, tonic and length.
- `ordrumbox-ui/tests/fixtures/piano_roll.json`: `NoteDrag` driven through a
  fake window (velocity, pitch, the axis lock, the bounds), and `midiName()`.

The synth editor has a tenth:

```sh
node tools/export_synth_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-engine/tests/fixtures/synth_editor.json`: `SynthPresetModel`
  on every preset of `generated_sounds.json` and hand-made ones (the defaults
  filled in, the cards bypassed), `getOrderedGroupNames()`, the LFO targets
  and their scales, and `computeLfoMod()` for every target, wave and sync at
  several times.

The song panel's commands have an eighth:

```sh
node tools/export_song_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/song_edits.json`: the JS `SongCommands`
  (clips added by id or name, repeated, removed one by one, by measure or by
  row; arrangements added, removed and selected; what each one refuses) on
  `song.json`'s arrangements, then undo all, then redo all; after each, the
  undo/redo sizes, the step's description, what the command returned, the
  selected arrangement and every arrangement (with its loop length, which an
  edit may leave fractional). Also `songMeasureAtTick()`.

## License

MIT: see `LICENSE`. A plugin built with the `vst3` feature links the VST3
bindings (`vst3-sys`), which are GPLv3: that `orDrumbox.vst3` is GPLv3 as a
whole. The CLAP build stays MIT.
