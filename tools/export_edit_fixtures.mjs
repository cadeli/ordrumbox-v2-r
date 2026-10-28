#!/usr/bin/env node
// Golden fixture for the editing commands, produced by the JS Commander and
// HistoryManager themselves.
//
//   node tools/export_edit_fixtures.mjs <path-to-ordrumbox-v2>
//
// Loads assets/data/song.json as the library tests do (fixPatterns), runs a
// list of edits on it (OPS below, also read by the Rust test), then undoes
// them all and redoes them all. After every operation it records the undo
// stack size, the description of the newest step, the pattern ids, and the
// exported (Exporter.export) patterns the edits touch. Writes
// crates/ordrumbox-logic/tests/fixtures/edits.json.

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_edit_fixtures.mjs <path-to-ordrumbox-v2>\n')
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
const { fixPatterns } = await import(srcUrl('patterns/fixer.js'))
const { Exporter } = await import(srcUrl('patterns/exporter.js'))

// Every edit touches pattern 0 ("rock", 8 beats, 8 tracks) or the pattern list.
const OPS = [
    { op: 'addNote', p: 0, t: 0, beat: 1, beatStep: 2, pitch: 0 },
    { op: 'addNote', p: 0, t: 0, beat: 7, beatStep: 3, pitch: 5 },
    { op: 'deleteNote', p: 0, t: 0, beat: 0, beatStep: 0, pitch: 0 },
    { op: 'updateTrack', p: 0, t: 1, updates: { velocity: 3, filterFreq: 50000, scaleName: 'major', mute: true } },
    { op: 'updateTrack', p: 0, t: 2, updates: { velocity: 0.2 }, coalesce: true },
    { op: 'updateTrack', p: 0, t: 2, updates: { velocity: 0.35 }, coalesce: true },
    { op: 'updateTrack', p: 0, t: 2, updates: { velocity: 0.5 }, coalesce: true },
    { op: 'setStepsPerBeat', p: 0, t: 1, value: 3 },
    { op: 'setStepsPerBeat', p: 0, t: 1, value: 8 },
    { op: 'incrStepsPerBeat', p: 0, t: 3 },
    { op: 'decrLoopPoint', p: 0, t: 0 },
    // On CHH, which no snapshot-based command (steps per beat, clean…) touches:
    // those put back copies of the notes, after which the JS note edits'
    // undo/redo closures change detached objects (nothing happens).
    { op: 'updateNote', p: 0, t: 5, n: 0, updates: { velocity: 0.3, pitch: 2 } },
    { op: 'updateNote', p: 0, t: 5, n: 1, updates: { velocity: 0.45 }, coalesce: true },
    { op: 'updateNote', p: 0, t: 5, n: 1, updates: { velocity: 0.5 }, coalesce: true },
    { op: 'pasteStepNotes', p: 0, t: 2, beat: 3, beatStep: 1, from: { t: 1, n: 0 } },
    { op: 'pasteTrack', p: 0, index: 1, from: 0 },
    { op: 'pasteTrack', p: 0, index: 99, from: 0 },
    // OHH: a track no earlier edit touched. Undoing the removal of an edited
    // track (SNARE here) breaks the undo of its earlier note edits in JS:
    // removeTrack's undo puts back COPIES of the notes, and the note edits'
    // undo closures then change the detached originals (nothing happens).
    { op: 'removeTrack', p: 0, t: 7 },
    { op: 'addTrack', p: 0, name: 'CHH' },
    { op: 'changeTrackName', p: 0, t: 0, name: 'BD' },
    { op: 'changeTrackSound', p: 0, t: 0, sampleId: 'real/kick.wav' },
    { op: 'cleanTrack', p: 0, t: 4 },
    { op: 'setPatternBpm', p: 0, bpm: 133 },
    { op: 'setPatternBeatCount', p: 0, beats: 3 },
    { op: 'setPatternDescription', p: 0, description: 'edited' },
    { op: 'renamePattern', p: 0, name: '  Rock II ' },
    { op: 'addPattern', name: 'Fresh' },
    { op: 'addPattern' },
    { op: 'removePattern', p: 1 },
]

const song = JSON.parse(await readFile(resolve(jsRepo, 'assets', 'data', 'song.json'), 'utf8'))
appState.patterns = fixPatterns(structuredClone(song.patterns))

const history = new HistoryManager(50)
let lastDesc = null
const record = history.record.bind(history)
history.record = (command) => {
    lastDesc = command.meta?.desc ?? null
    record(command)
}
const cmd = new Commander({ history })

const P = (i) => appState.patterns[i]
const T = (o) => P(o.p).tracks[o.t]
function run(o) {
    switch (o.op) {
        case 'addNote': return cmd.addNote(T(o), o.beat, o.beatStep, o.pitch)
        case 'deleteNote': return cmd.deleteNote(T(o), { beat: o.beat, beatStep: o.beatStep, pitch: o.pitch })
        case 'updateTrack': return cmd.updateTrack(T(o), o.updates, { coalesce: !!o.coalesce })
        case 'setStepsPerBeat': return cmd.setStepsPerBeat(T(o), o.value)
        case 'incrStepsPerBeat': return cmd.incrStepsPerBeat(T(o))
        case 'decrLoopPoint': return cmd.decrLoopPoint(T(o))
        case 'updateNote': return cmd.updateNote(T(o), T(o).notes[o.n], o.updates, { coalesce: !!o.coalesce })
        case 'pasteStepNotes': {
            const source = P(o.p).tracks[o.from.t].notes[o.from.n]
            return cmd.pasteStepNotes(T(o), o.beat, o.beatStep, [{ ...source }])
        }
        case 'pasteTrack': return cmd.pasteTrack(P(o.p), o.index, P(o.p).tracks[o.from])
        case 'removeTrack': return cmd.removeTrack(P(o.p), o.t)
        case 'addTrack': return cmd.addTrack(P(o.p), o.name)
        case 'changeTrackName': return cmd.changeTrackName(T(o), o.name)
        case 'changeTrackSound': return cmd.changeTrackSound(T(o), o.sampleId)
        case 'cleanTrack': return cmd.cleanTrack(T(o))
        case 'setPatternBpm': return cmd.setPatternBpm(P(o.p), o.bpm)
        case 'setPatternBeatCount': return cmd.setPatternBeatCount(P(o.p), o.beats)
        case 'setPatternDescription': return cmd.setPatternDescription(P(o.p), o.description)
        case 'renamePattern': return cmd.renamePattern(o.p, o.name)
        case 'addPattern': return cmd.addPattern(o.name)
        case 'removePattern': return cmd.removePattern(o.p)
        default: throw new Error(`unknown op ${o.op}`)
    }
}

/** What the Rust test compares after each operation. */
const snapshot = () => ({
    undo: history.pastLength,
    redo: history.futureLength,
    ids: appState.patterns.map((p) => p.id),
    // Pattern 0 and the patterns added at the end.
    patterns: [0, ...appState.patterns.map((_, i) => i).slice(42)].map((i) => Exporter.export(P(i))),
})

const steps = []
for (const o of OPS) {
    lastDesc = null
    run(o)
    steps.push({ after: o, desc: lastDesc, ...snapshot() })
}
while (history.canUndo) {
    history.undo()
    steps.push({ after: { op: 'undo' }, desc: null, ...snapshot() })
}
while (history.canRedo) {
    history.redo()
    steps.push({ after: { op: 'redo' }, desc: null, ...snapshot() })
}

await mkdir(outDir, { recursive: true })
await writeFile(join(outDir, 'edits.json'), JSON.stringify({ ops: OPS, steps }) + '\n')
process.stdout.write(`edits: ${OPS.length} operations, ${steps.length} states\n`)

// ─── Compact Tracks (the tools panel's button: compactTrack on every track) ───
//
// On every pattern of song.json, and on hand-made patterns that repeat,
// loaded the way the app boots (fixPatterns, then importPatternFromJson, as
// tools/export_fixtures.mjs does): the notes are full objects, with a
// stepPercent, which compactTrack compares. Writes compact.json: the step
// descriptions and the tracks after (loop point, notes).

const { importPatternFromJson } = await import(srcUrl('logic/commands/pattern_import.js'))
const { normalizeTrack, clampStepsPerBeat } = await import(srcUrl('model/track_schema.js'))
const { NOTE_DEFAULTS } = await import(srcUrl('core/note_schema.js'))
const { getPanFromTrackName } = await import(srcUrl('core/drum_taxonomy.js'))

function importAsApp(fixed) {
    const addPattern = (name) => ({ id: '', name, description: '', tags: [], tracks: [], bpm: 120, beatCount: 4 })
    const addTrack = (pattern, name) => {
        const track = normalizeTrack({
            name,
            beatCount: pattern.beatCount,
            stepsPerBeat: 4,
            loopAtStep: pattern.beatCount * 4,
            pan: getPanFromTrackName(name),
        })
        clampStepsPerBeat(track)
        pattern.tracks.push(track)
        return track
    }
    const addNote = (track, beat, beatStep, pitch = 0) => {
        clampStepsPerBeat(track)
        const note = { ...NOTE_DEFAULTS, beatStep, stepPercent: Math.round((beatStep * 100) / track.stepsPerBeat), beat, pitch }
        track.notes.push(note)
        return note
    }
    return importPatternFromJson(fixed, addPattern, addTrack, addNote)
}

const rep = (period, steps, extra = {}) =>
    Array.from({ length: steps / period }, (_, k) => ({ beat: Math.floor((k * period) / 4), beatStep: (k * period) % 4, ...extra }))
const handPatterns = [
    { name: 'h1', beatCount: 4, tracks: [{ name: 'KICK', stepsPerBeat: 4, beatCount: 4, notes: rep(4, 16) }] },
    {
        name: 'h2',
        beatCount: 8,
        tracks: [
            { name: 'HH', stepsPerBeat: 4, beatCount: 8, notes: rep(2, 32, { velocity: 0.5 }) },
            { name: 'SNARE', stepsPerBeat: 4, beatCount: 8, notes: [...rep(8, 32), { beat: 7, beatStep: 3 }] },
            { name: 'BASS', stepsPerBeat: 4, beatCount: 8, notes: [...rep(16, 32, { pitch: 3 }), ...rep(16, 32, { pitch: 7 })] },
            { name: 'TOM', stepsPerBeat: 4, beatCount: 8, loopAtStep: 16, notes: rep(4, 16) },
            { name: 'EMPTY', stepsPerBeat: 4, beatCount: 8, notes: [] },
        ],
    },
]
const compacted = []
for (const [source, list] of [['song', song.patterns], ['hand', handPatterns]]) {
    appState.patterns = fixPatterns(structuredClone(list)).map(importAsApp)
    const compactHistory = new HistoryManager(500)
    const descs = []
    const recordCompact = compactHistory.record.bind(compactHistory)
    compactHistory.record = (command) => {
        descs.push(command.meta?.desc ?? null)
        recordCompact(command)
    }
    const compactCmd = new Commander({ history: compactHistory })
    appState.patterns.forEach((pattern, p) => {
        descs.length = 0
        pattern.tracks.forEach((track) => compactCmd.compactTrack(track))
        const tracks = pattern.tracks.map((t) => ({
            name: t.name,
            loopAtStep: t.loopAtStep,
            notes: t.notes.map((n) => [n.beat, n.beatStep, n.pitch, n.velocity]),
        }))
        compacted.push({ source, p, descs: [...descs], tracks })
    })
}
await writeFile(join(outDir, 'compact.json'), JSON.stringify({ hand: handPatterns, patterns: compacted }) + '\n')
process.stdout.write(`compact: ${compacted.length} patterns, ${compacted.filter((c) => c.descs.length).length} compacted\n`)
