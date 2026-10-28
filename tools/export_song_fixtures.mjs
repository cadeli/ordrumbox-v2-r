#!/usr/bin/env node
// Golden fixture for the arrangement commands (song panel), produced by the
// JS Commander, SongCommands and HistoryManager themselves.
//
//   node tools/export_song_fixtures.mjs <path-to-ordrumbox-v2>
//
// Loads assets/data/song.json as the app boots (fixPatterns, fixSongs), runs
// the arrangement edits below (OPS, also read by the Rust test), then undoes
// them all and redoes them all. After every operation it records the undo and
// redo stack sizes, the newest step's description, what the command returned,
// the selected arrangement and every arrangement. It also records
// songMeasureAtTick() for the arrangements over a range of ticks.
// Writes crates/ordrumbox-logic/tests/fixtures/song_edits.json.

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_song_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
console.warn = () => {}
console.error = () => {}
console.info = () => {}
console.debug = () => {}

const here = dirname(fileURLToPath(import.meta.url))
const outDir = join(here, '..', 'crates', 'ordrumbox-logic', 'tests', 'fixtures')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href

const { appState } = await import(srcUrl('state/app_state.js'))
const { default: Commander } = await import(srcUrl('logic/commands/commander.js'))
const { default: HistoryManager } = await import(srcUrl('logic/history_manager.js'))
const { fixPatterns, fixSongs } = await import(srcUrl('patterns/fixer.js'))
const { songMeasureAtTick } = await import(srcUrl('logic/song_playback.js'))
const { songLengthMeasures } = await import(srcUrl('model/song_schema.js'))

// song 0 is "demo" (24 clips, loop 66), song 1 "rock-8-measures" (2 clips, loop 8).
const OPS = [
    { op: 'addPatternAtMeasure', ref: 'rock', start: 70 },
    { op: 'addPatternAtMeasure', ref: '  ROCK8A ', start: 3, measureCount: 0.75 },
    { op: 'addPatternAtMeasure', ref: 'no such pattern', start: 1 },
    { op: 'addPatternAtMeasure', ref: '', start: 1 },
    { op: 'repeatPatternAtMeasure', start: 3 },
    { op: 'repeatPatternAtMeasure', start: 999 },
    { op: 'removeSongClips', indices: [0, 2, 2, -1, 999] },
    { op: 'removeSongClips', indices: [] },
    { op: 'removePatternAtMeasure', start: 70 },
    { op: 'removePatternAtMeasure', start: 71 },
    { op: 'removePatternClips', ref: 'basrock' },
    { op: 'removePatternClips', ref: 'ghost' },
    { op: 'setSelectedSongIdx', songIdx: 1 },
    { op: 'addPatternAtMeasure', ref: 'rock', start: 0, songIdx: 0 },
    { op: 'removePatternClips', ref: 'rock8a' },
    { op: 'removePatternClips', ref: 'rock8b' },
    { op: 'addPatternAtMeasure', ref: 'smrock', start: 2 },
    { op: 'addSong', name: '  New one ' },
    { op: 'addSong', name: '' },
    { op: 'addSong', name: 'Untitled' },
    { op: 'addPatternAtMeasure', ref: 'rock', start: 1 },
    { op: 'repeatPatternAtMeasure', start: 1 },
    { op: 'setSelectedSongIdx', songIdx: 99 },
    { op: 'removeSong', songIdx: 1 },
    { op: 'removeSong', songIdx: 42 },
    { op: 'setSelectedSongIdx', songIdx: 0 },
    { op: 'removeSong' },
]

const lib = JSON.parse(await readFile(resolve(jsRepo, 'assets', 'data', 'song.json'), 'utf8'))
appState.patterns = fixPatterns(structuredClone(lib.patterns))
appState.songs = fixSongs(structuredClone(lib.songs), appState.patterns).songs
appState.selectedSongIdx = 0

const history = new HistoryManager(50)
let lastDesc = null
const record = history.record.bind(history)
history.record = (command) => {
    lastDesc = command.meta?.desc ?? null
    record(command)
}
const cmd = new Commander({ history })

const opts = (o) => (o.songIdx === undefined && o.measureCount === undefined ? undefined : {
    ...(o.songIdx === undefined ? {} : { songIdx: o.songIdx }),
    ...(o.measureCount === undefined ? {} : { measureCount: o.measureCount }),
})
/** What the command returned, as the Rust test compares it. */
const clipOut = (clip) => (clip ? { pattern: clip.pattern, startMeasure: clip.startMeasure, measureCount: clip.measureCount } : null)
function run(o) {
    switch (o.op) {
        case 'addPatternAtMeasure': return clipOut(cmd.addPatternAtMeasure(o.ref, o.start, opts(o)))
        case 'repeatPatternAtMeasure': return clipOut(cmd.repeatPatternAtMeasure(o.start, opts(o)))
        case 'removeSongClips': return cmd.removeSongClips(o.indices, o.songIdx)
        case 'removePatternAtMeasure': return cmd.removePatternAtMeasure(o.start, opts(o)).length
        case 'removePatternClips': return cmd.removePatternClips(o.ref, opts(o)).length
        case 'setSelectedSongIdx': return cmd.setSelectedSongIdx(o.songIdx)
        case 'addSong': return cmd.addSong({ name: o.name })?.id ?? null
        case 'removeSong': return cmd.removeSong(o.songIdx)
        default: throw new Error(`unknown op ${o.op}`)
    }
}

/** The arrangements as the app holds them (loopMeasureCount may be fractional). */
const songs = () =>
    appState.songs.map((s) => ({
        id: s.id,
        name: s.name,
        description: s.description,
        bpm: s.bpm,
        loopMeasureCount: s.loopMeasureCount ?? null,
        length: songLengthMeasures(s),
        clips: s.clips.map(clipOut),
    }))

const snapshot = () => ({
    undo: history.pastLength,
    redo: history.futureLength,
    selected: appState.selectedSongIdx,
    songs: songs(),
})

const steps = []
for (const o of OPS) {
    lastDesc = null
    const result = run(o)
    steps.push({ after: o, desc: lastDesc, result: result ?? null, ...snapshot() })
}
while (history.canUndo) {
    history.undo()
    steps.push({ after: { op: 'undo' }, desc: null, result: null, ...snapshot() })
}
while (history.canRedo) {
    history.redo()
    steps.push({ after: { op: 'redo' }, desc: null, result: null, ...snapshot() })
}

// songMeasureAtTick (TICK = 32 per beat) on the file's arrangements and on a
// fractional loop.
const measureSongs = [
    ...fixSongs(structuredClone(lib.songs), appState.patterns).songs,
    { clips: [{ pattern: 'rock', startMeasure: 0, measureCount: 4.75 }], loopMeasureCount: 4.75 },
    { clips: [] },
]
const ticks = [undefined, -5, 0, 1, 127, 128, 129, 1000, 8 * 128, 66 * 128 + 3, 1e6, 4.75 * 128, 600]
const measures = measureSongs.map((song) => ({
    loopMeasureCount: song.loopMeasureCount ?? null,
    clips: song.clips.map(clipOut),
    at: ticks.map((tick) => ({ tick: tick ?? null, measure: songMeasureAtTick(song, tick, 32) })),
}))

await mkdir(outDir, { recursive: true })
await writeFile(join(outDir, 'song_edits.json'), JSON.stringify({ ops: OPS, steps, measures }) + '\n')
process.stdout.write(`song edits: ${OPS.length} operations, ${steps.length} states\n`)
