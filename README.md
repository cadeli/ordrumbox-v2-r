# orDrumbox v2 (Rust)

Note: the legacy orDrumbox V1, the old Java drum machine, has been largely
improved upon and superseded; it is no longer current. This Rust port is the
active version.

The JavaScript/web version (<https://github.com/cadeli/ordrumbox-v2>) is a web demo of this Rust version: it is less complete than the downloadable (native) version.

## Download

- Download page: <https://www.ordrumbox.com/download/download.html>
- GitHub releases: <https://github.com/cadeli/ordrumbox-v2-r/releases>

The app is a single executable with its `assets/` folder next to it.

On Windows, a release holds a setup and a zip:

- `ordrumbox-<version>-windows-x64-setup.exe` installs orDrumbox in `Program Files`, with a
  Start menu shortcut, and the CLAP plugin in `C:\Program Files\Common Files\CLAP\orDrumbox\`
  (an option, on by default). Running the setup of a newer version updates it. Uninstalling
  (Windows settings, or the Start menu) removes what the setup installed, never your library
  and settings in the user data folder. The setup is not signed: Windows may warn about an
  unknown publisher (More info > Run anyway).
- `ordrumbox-<version>-windows-x64.zip` is the same app without setup: unzip it anywhere
  and start `ordrumbox.exe`.
- On Linux, `ordrumbox-<version>-linux-x64.zip` holds the same files as the Windows zip:
  unzip it anywhere and run `./ordrumbox`. The binary is built against the system's ALSA
  and X11 libraries; on a Debian-like distribution, `sudo apt install libasound2
  libxkbcommon-x11-0 libxkbcommon0` installs what it links at run time.

The CLAP plugin comes with each release:

- **Windows**: the setup installs it. Without the setup, `orDrumbox.clap` is in the app's
  zip, next to `ordrumbox.exe`. It reads the `assets/` folder next to it: copy both,
  `orDrumbox.clap` and `assets/`, into a folder of their own, such as
  `C:\Program Files\Common Files\CLAP\orDrumbox\` (or any folder your host scans for CLAP
  plugins).
- **Linux**: `ordrumbox-<version>-linux-clap.zip` holds `orDrumbox.clap`, to use as on
  Windows: put it with an `assets/` folder in a folder your host scans for CLAP plugins,
  such as `~/.clap/`.
- **macOS**: `ordrumbox-<version>-macos-clap.zip` holds `orDrumbox.clap`, its assets inside.
  Unzip it into `~/Library/Audio/Plug-Ins/CLAP/`. It is not notarized: if the host refuses
  to load it, run `xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/CLAP/orDrumbox.clap`.

## Documentation

- User manual: [`docs/user-manual.html`](docs/user-manual.html), with its
  screenshots in `docs/images/`.
- AI clients (MCP): [`docs/mcp-guide.html`](docs/mcp-guide.html).
- Building from source, tests: [`DEVELOPMENT.md`](DEVELOPMENT.md).

## The library

User data is saved under the OS user data folder
(`%APPDATA%\orDrumbox` on Windows, `~/Library/Application Support/orDrumbox` on
macOS, `~/.local/share/ordrumbox` elsewhere).

The library's name and whether its edits are saved show at the right of the
menu bar: *saved*, *modified* (saved a moment after the last edit), *new*
(a copy of the demo, written at the first edit), *save failed*, *not saved*
(no user data folder), or *in host project* (the plugin). Help > About gives
the file's path.

## Plugin

The plugin follows the host transport and saves its library in the host project.
Its window has no play button and no tempo knob (the host's transport plays, at the host's
tempo): in their place, **HOST** shows whether the host plays (a triangle) or is stopped (a
square), and the tempo played. Its menus leave out what the host or the app does: save,
quit, play, tempo, songs, the user data folder, updates, factory reset. A host tempo
outside 20–300 BPM plays at the nearest bound, the tempo shown, and the status line says so.
Playback starts on the host's first block, on its first frame. An offline render (a
bounce, or a host computing ahead of real time) plays as in real time, every note in its
place: the plugin then waits for its pattern, kit and passes before each block.
Its window opens compact, the grid alone (1072×388 at scale 1), to leave the host's
arrangement in view: **F2** shows or folds the track editor, **F7** the note editor, and the
window grows or shrinks with them when it showed the layout whole. The grip in its
bottom-right corner resizes the window (down to 320×200, where the layout scrolls, its
scrollbars stopping short of the grip); the project keeps the size.

Parameters the host automates: **Pattern** and **Kit** (by number, from 1), **Volume**
(master, -60 to +6 dB), **Swing** (50 % straight to 75 %: in each pair of steps, where the
second one starts, on every track with an even number of steps per beat, on top of the
track's own swing), and for tracks 1–16 **Mute**, **Volume** (a gain on the track's own,
-60 to +6 dB), **Pan** (added to the track's own), **Cutoff** (moves the filter's
cutoff by up to ±4 octaves), **Q** (added to the filter's), **Reverb**, **Delay**,
**Saturation** and **Compressor** (added to the effect's amount, kept within 0 to 1; with
the effect's button off, the track's amount counts as 0), **Threshold** (dB added to the
compressor's threshold, kept within -60 to 0 dBFS), **Feedback** (added to the delay's, up
to 0.95; a slap delay keeps its single echo) and **Transpose** (±24 semitones on the notes played,
MIDI input included, not on the MIDI notes sent out). At their defaults they change
nothing. A filter of type `allpass`, and a reverb or delay of type `none`, stay off.
They change what plays, not the library.

The plugin has 17 stereo outputs: **Mix**, and **Track 1** to **Track 16**. Every track plays
in the mix until its **Output** parameter is set to *Own*: track 3 then leaves the mix and
plays on **Track 3**, with its own volume, pan, filter and effects, for the host to mix and
process apart. The master's compressor, filters, limit and volume act on the mix only. An
output with no track on it is silent. A host that gives the plugin a single stereo output
keeps every track in the mix, whatever the parameter says; tracks past the 16th always are.

MIDI goes both ways, with one place per track of the pattern playing (or selected):
a drum track is on channel 10 at its instrument's GM key (KICK 36, SNARE 38…), whatever
the note's pitch. Any other track (melodic, unknown name, a second track of one drum) has a
channel of its own (1–9, 11–15, in track order), its notes on the key of their pitch: the
track's pitch 0 is its sample's root note, C3 (48) without one and for a synth. Past the 14
channels, the tracks left take keys of channel 10 from 84 up.

- **In**: a note on a track's channel plays that track at the note's pitch; a note on any
  other channel plays the drum track with that key. At its velocity, whether the host plays
  or not.
- **Pattern by note** (in, the app too): a note on channel 16 chooses a pattern: key 0 (C-2,
  or C-1 by the keyboard's naming) is pattern 1, key 1 pattern 2… up to 128. It applies at
  once: a pattern playing is left for the new one from its first beat (a plugin keeps to the
  host's place in the pass). No track is on channel 16, so the melodic tracks use channels
  1–9 and 11–15, and the tracks left past them take keys of channel 10. A pattern the library
  does not have is refused, and the status line says so.
- **Program change** (in): program 1–128, on any channel, chooses pattern 1–128, as the
  **Pattern** parameter does: shown at once, played once the pattern playing ends its loop.
  A program the library has no pattern for is refused, and the status line says so. The
  host's Pattern parameter keeps its value. CLAP only: a VST3 host gives a plugin no program
  change (map a MIDI control to the Pattern parameter instead).
- **Out**: the notes the pattern plays go out where they come in, with their pitch (the
  track's transpose and pitch LFO included), as long as the note (a step of its track unless
  its **Length** says otherwise). Tracks the host mutes send nothing.
  A clip recorded from this output and played back into the plugin plays the same tracks at
  the same pitches.

## The grid

One row per track, a page of beats across: 4 beats, or 8 or 16 when the window is wide
enough for steps of a usable size. Under the rows, the **lane** shows one setting of the
track editor's track, a bar per note: **VELOCITY**, **CHANCE** (its probability), **RETRIG**
(how many times it plays), **PAN**, **LENGTH** (how long it is held, in steps) or **SHIFT**
(how early or late it plays, off the grid), picked by the tabs on its left. Click a bar to set
its value at that height, or drag across the bars to draw them; a drag is one undo step.

The **note editor** (the selected note's velocity, pitch, pan, trigger, retrigger,
euclidean, arpeggio and timing settings) is a strip under the grid and under the piano roll, the
views that edit notes. The track editor, on the right, holds the track's settings only.

A note's **Length** (a knob beside Velocity, Pitch and Pan, 1 step by default, a quarter of a step at least) is how
long a synth note is held before its release, and how long the MIDI note sent or exported
lasts; a sample plays to its own end whatever the length. In the piano roll a note is as wide
as it is long: drag its right edge to draw it out. Its **Shift** moves it off the grid, up
to half a step early or late, with its retriggers.

## Keys

Press **F1** in the app for the full keyboard shortcut list. Keys are the ones the
keyboard's layout marks (Ctrl Z is the key marked Z on AZERTY too), except the two rows
that play: 1–9 (mute, Shift: solo) and Q–P (hear a track) are read by their place. Common keys:

- **Space**: play / stop. A song stops where it is and **Space** goes on from that
  measure; **Home** (or the keypad's **Enter**) goes back to the first beat, or measure.
  **Shift + click** on the song panel's ruler loops the song from the cursor's measure to
  the one clicked (the ruler shows it; Play > Clear the loop ends it).
- **F9**: metronome (a click on every beat, the first of a measure higher), **F8**: tap
  the tempo (tap on the beat; a pause of 2 seconds starts the count again)
- **Ctrl R**: record. While the pattern plays, **Q–P** also put a note on their track, on
  the step nearest to the playhead (one undo step each; a step that has a note keeps it).
  With **Play > Record off the grid** on, a recorded note also keeps how early or late it
  was played, as its Shift
- **F11**: full screen, **Ctrl W**: close the window (as **Ctrl Q**)
- **F1–F7** (also **Ctrl 1–7**, for the keyboards whose F keys want Fn): the shortcut list,
  track editor, song panel, piano roll, grid, synth editor, note editor (shown or folded)
- **Ctrl K**: kit editor, **Ctrl Shift M**: export MIDI (**Ctrl E**: export WAV)
- **Ctrl -** / **Ctrl =**: zoom out / in a quarter at a time (×1, ×1.25, ×1.5… up to ×4,
  the keypad's keys too), **Ctrl 0**: the display's scale, **View > Zoom: fit to
  window**: the biggest of these scales the window shows whole. The zoom does not depend
  on the window's size: a window too small for it gets scrollbars (drag them,
  click beside the thumb, or turn the wheel where nothing else takes it). A
  window goes down to 320×200, the app's and the plugin's. The
  zoom is kept in `settings.json` (`uiZoom`, scale units added to the
  display's, such as `0.5`; whole numbers from earlier versions still read). A
  display at 150 % shows at ×1.5. In the plugin, Alt stands for Ctrl (the host
  keeps Ctrl).
- **M** / **S**: mute / solo the track under the cursor. The letters that change
  every track want **Shift** (B: the pattern again in its style, C: the next colour
  scheme, D: drums in auto, G, H, J, K, V: the VU meters, X: random notes), so a
  slip off the play rows does nothing; the pattern's length has no key (the
  Song panel and the menus change it).
- **Ctrl A**: select the whole pattern as a zone. **Shift + arrows** grow a zone from the
  cursor, **Ctrl + click** on a step takes it into the zone (it stays a rectangle); copy,
  cut and delete then act on it.
- **Edit > Quantize...** moves notes towards the lines of a grid (1/4, 1/8, 1/16 or 1/32 of a
  note) by a strength of 25, 50, 75 or 100 %. It takes the zone selected, else the track under
  the cursor, else the whole pattern. At 100 % on the track's own grid it takes the shift off
  notes recorded with *Record off the grid*; a coarser grid also moves their steps (a step
  halfway between two lines goes to the later one); a lower strength keeps what is left as a
  shift. A note pushed past the pattern's end or start wraps round it. One undo step.
- **Alt + arrows** on a selected note in the grid: up / down a semitone of pitch, left /
  right one step in time (in the piano roll, **Shift + arrows**, and its drag follows the
  same axes: up / down the pitch, sideways the velocity).
- **Edit > Preferences...** (**Ctrl ,**) gathers the sound output, the MIDI ports and clock,
  the metronome, the VU meters and the toasts in one window. Up / Down choose a line, Enter or
  Left / Right change it, **OK** applies and **Esc** leaves everything as it was. The sound
  output and the MIDI input are kept in `settings.json` (`audioDevice`, `midiInput`: empty
  is the system's device or the first port, `"off"` no MIDI). A new output starts the sound
  again; a device gone gives way to the system's, and the status line names the one in use.
  The MIDI output (`midiOutput`: a port's name, none by default), the clock it sends
  (`midiClockOut`, `true` or `false`) and the clock followed (`midiClockIn`) are kept there too.
- **MIDI keyboard or pad** (the app; a plugin takes its host's): the notes it plays sound
  the pattern's tracks, drums by their General MIDI key (kick 36, snare 38…), a melodic
  track on its own channel at the note's pitch. With **Ctrl R** on and the pattern playing,
  each note is also written to its track, on the step nearest to the playhead, at the
  velocity played (one undo step each).
- **MIDI output** (the app): the notes the pattern plays go out of the port chosen in the
  Preferences, as a plugin sends them: drums on channel 10 at their GM key, any other track
  on a channel of its own at its pitch, as long as the note. They leave when the block that
  plays them is rendered, so a sound device with a long buffer plays a few milliseconds after
  them.
- **MIDI clock out** (the app, with a MIDI output): the app sends 24 pulses a beat while it
  plays, **Start** from the first beat, and **Song position** then **Continue** when a song
  plays from a later measure, and **Stop**: a drum machine or a synth set to an external clock
  plays along. The clock follows the tempo, the pattern's length and the swing.
- **MIDI clock in** (the app, with a MIDI input): with *MIDI clock in: follow*, the machine
  sending to the input is the master. Its **Start**, **Continue**, **Stop** and **Song position**
  start, stop and place the transport; its pulses set the tempo, to a tenth of a beat a
  minute, and keep the playing in step with it. Nothing plays until it starts, so Space does
  nothing while following: choose *ignore* to play on its own again. A master that stops
  sending pulses for a second stops the transport.
- **Edit > MIDI controls...** binds the knobs of a MIDI controller (the app; a plugin has its
  host's parameters). Choose a control with Left / Right (the master volume, then each track
  of the pattern shown: **volume**, **pan**, **cutoff**, **mute**; the first 16 tracks),
  go to **Learn**, press Enter and turn the knob: its control change (number and channel) is
  bound to it. One knob has one control and a control one knob; **Clear** lets it go. The
  bindings are kept in `settings.json` (`midiControls`). A knob moves its control on top of
  what the track has, as a plugin host's mix does, so nothing is saved with the library and
  no undo step is made: volumes go from silence (knob down) to the one set; pan and cutoff
  go from the one set (knob in the middle: no change) up to the end of the knob, the cutoff
  by four octaves; a mute is on from the middle of the knob up, and shows on the track's mute
  box. A knob takes its control to its place as soon as it is turned, wherever the control was.
- **On macOS** the menus show **Cmd** in place of Ctrl (the same key here); redo is
  **Cmd Shift Z** (also Cmd Y), and export MIDI is **Cmd Shift M**: Cmd M alone
  minimizes the window.

## Files

The app writes `library.json`, `kits.json`, `settings.json`, `songs/`, `samples/` and
`exports/` in the user data folder. Pattern and track settings use camelCase keys; old (JS)
spellings are still read.

The synth presets edited (added, changed from the assets' ones, or deleted: `null`) are part
of the library, under `synthPresets` in `library.json`, in File > Export library, and in a
plugin's host project: each library, each project, has its own, and each change is an undo
step (a knob turned is one). A preset of the assets it does not name plays as the assets hold
it. A library saved before has none of its own: it takes those of `synth_presets.json` (or
`generated_sounds.json`), which earlier versions wrote and which is now only read. File >
Import library adds its presets, replacing those of the same name; a song loaded keeps them.

File > Export kit with its samples makes a folder named after the kit, holding `kit.json`
(the kit's mapping, each sample's path the name of its file there) and the samples' WAV
files. Kit > Import folder as kit (or the folder dropped on the window) reads it back: the
samples are copied to `samples/imported/<kit>/`, the kit replaces the kit of its name. A
folder without `kit.json` is imported as before, each WAV's instrument guessed from its name.

File > Back up user data copies all of it (the library with its synth presets, `kits.json`,
`settings.json`, `samples/`, `songs/`) to a folder `orDrumbox-backup-<date>-<time>` (UTC);
File > Restore user data puts such a folder back, the files there moved to `backups/` first.

## License

MIT: see `LICENSE`. The VST3 plugin links GPLv3 VST3 bindings, so that
`orDrumbox.vst3` is GPLv3 as a whole. The CLAP build stays
MIT.