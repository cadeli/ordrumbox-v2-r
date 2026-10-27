#!/usr/bin/env node
// Golden fixtures for Standard MIDI Files, produced by the JS app's own
// modules (logic/midi/midi_parser.js, midi_exporter.js,
// logic/services/midi_import_service.js).
//
//   node tools/export_midi_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes crates/ordrumbox-logic/tests/fixtures/midi.json:
//   - sounds: the sample keys and root notes the exporter and the import
//     read (drumkits.json, kit after kit);
//   - sort: Array.prototype.sort with the exporter's note on/off comparator
//     (inconsistent for two ons or two offs at one tick, so the order is
//     V8's TimSort's own) on event lists, as index permutations; and
//     (chaosSorts, blockSorts) a self-contradicting comparator and
//     interleaved runs, which pin every step of the algorithm;
//   - files: MIDI files (hand-made ones covering the parser's paths, and
//     exports of song.json patterns) with parseMidi(), findAllNotes() and
//     extractProgramChanges();
//   - exports: MidiExporter.export() bytes of every song.json pattern (and
//     edge cases), 1 and 3 loops, for the patterns the pattern engine plays
//     the same with any Math.random stream;
//   - imports: the patterns MidiImportService.importFile() creates from the files.

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_midi_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href
const readJson = async (rel) => JSON.parse(await readFile(resolve(jsRepo, rel), 'utf8'))

const { soundRegistry } = await import(srcUrl('state/sound_registry.js'))
const { appState } = await import(srcUrl('state/app_state.js'))
const { parseMidi, findAllNotes, extractProgramChanges } = await import(srcUrl('logic/midi/midi_parser.js'))
const { default: MidiExporter } = await import(srcUrl('logic/midi/midi_exporter.js'))
const { default: MidiImportService } = await import(srcUrl('logic/services/midi_import_service.js'))
const { fixPatterns } = await import(srcUrl('patterns/fixer.js'))
const { importPatternFromJson } = await import(srcUrl('logic/commands/pattern_import.js'))
const { normalizeTrack, clampStepsPerBeat } = await import(srcUrl('model/track_schema.js'))
const { NOTE_DEFAULTS } = await import(srcUrl('core/note_schema.js'))
const { getPanFromTrackName } = await import(srcUrl('core/drum_taxonomy.js'))

soundRegistry.scales = await readJson('assets/data/scales.json')
const kits = await readJson('assets/data/drumkits.json')
const sounds = []
soundRegistry.sounds = {}
for (const kit of kits) {
    for (const inst of kit.instruments ?? []) {
        const url = `${kit.name}/${inst.key}#${sounds.length}`
        soundRegistry.sounds[url] = { key: inst.key, kitName: kit.name, url, rootMidi: inst.rootMidi ?? null }
        sounds.push({ key: inst.key, rootMidi: inst.rootMidi ?? null })
    }
}

// ─── Random streams ──────────────────────────────────────────────────────────

const mulberry32 = (seed) => () => {
    seed = (seed + 0x6d2b79f5) >>> 0
    let t = seed
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}
const RANDOM_STREAMS = [() => () => 0, () => () => 0.999999, () => mulberry32(99)]

// ─── Sort ────────────────────────────────────────────────────────────────────

const onOff = (a, b) => a.tick - b.tick || (a.type === 'off' ? -1 : 1)
const sort = []
{
    const rnd = mulberry32(7)
    for (const n of [1, 2, 3, 5, 8, 16, 31, 32, 33, 63, 64, 65, 100, 200, 400, 1000, 3000]) {
        for (let k = 0; k < 3; k++) {
            const span = [2, 8, 64][k]
            const items = Array.from({ length: n }, () => ({
                tick: Math.floor(rnd() * span) * 24,
                type: rnd() < 0.5 ? 'on' : 'off',
            }))
            const order = items.map((item, i) => ({ ...item, i })).sort(onOff).map((e) => e.i)
            sort.push({ items: items.map((e) => [e.tick, e.type === 'on']), order })
        }
    }
    // The exporter's own shape: ons sorted by tick, each followed by its off.
    for (const n of [10, 40, 70, 150]) {
        const ticks = Array.from({ length: n }, () => Math.floor(rnd() * 32) * 24).sort((a, b) => a - b)
        const items = ticks.flatMap((t) => [
            { tick: t, type: 'on' },
            { tick: t + 24, type: 'off' },
        ])
        const order = items.map((item, i) => ({ ...item, i })).sort(onOff).map((e) => e.i)
        sort.push({ items: items.map((e) => [e.tick, e.type === 'on']), order })
    }
}

// Any comparator, even one that contradicts itself everywhere, must give the
// same permutation: it pins every step of the algorithm (runs, binary
// insertion, merges, galloping).
const chaos = (a, b) => ((a * 7919 + b * 104729 + ((a * b) % 13)) % 5) - 2
const chaosSorts = []
for (const n of [2, 7, 31, 32, 33, 64, 65, 127, 128, 300, 1000, 2500, 5000]) {
    const order = Array.from({ length: n }, (_, i) => i).sort(chaos)
    chaosSorts.push({ n, order })
}
// Runs that trigger galloping: long ascending blocks interleaved.
const blockSorts = []
for (const [n, block] of [[200, 10], [1000, 50], [4000, 300], [4000, 7]]) {
    const values = Array.from({ length: n }, (_, i) => (Math.floor(i / block) % 2 ? i : n - i) % 97)
    const order = values.map((v, i) => [v, i]).sort((a, b) => a[0] - b[0]).map((e) => e[1])
    blockSorts.push({ values, order })
    // The same with ties broken the exporter's inconsistent way.
    const flags = values.map((_, i) => (i * 31) % 7 < 3)
    const tieOrder = values
        .map((v, i) => [v, i])
        .sort((a, b) => a[0] - b[0] || (flags[a[1]] ? -1 : 1))
        .map((e) => e[1])
    blockSorts.push({ values, flags, order: tieOrder })
}

// ─── Hand-made files ─────────────────────────────────────────────────────────

const vlq = (v) => {
    const out = [v & 0x7f]
    v >>>= 7
    while (v > 0) {
        out.unshift((v & 0x7f) | 0x80)
        v >>>= 7
    }
    return out
}
const chunk = (tag, body) => [
    ...Array.from(tag, (c) => c.charCodeAt(0)),
    (body.length >>> 24) & 0xff,
    (body.length >>> 16) & 0xff,
    (body.length >>> 8) & 0xff,
    body.length & 0xff,
    ...body,
]
const header = (format, tracks, division) =>
    chunk('MThd', [0, format, (tracks >> 8) & 0xff, tracks & 0xff, (division >> 8) & 0xff, division & 0xff])
const text = (s) => Array.from(s, (c) => c.charCodeAt(0))
const meta = (delta, type, data) => [...vlq(delta), 0xff, type, ...vlq(data.length), ...data]
const name = (s) => meta(0, 0x03, text(s))
const end = () => meta(0, 0x2f, [])
const tempo = (bpm) => {
    const us = Math.round(60000000 / bpm)
    return meta(0, 0x51, [(us >> 16) & 0xff, (us >> 8) & 0xff, us & 0xff])
}
/** Notes [tick, note, velocity] on `channel`, each 1/8 beat long, with running status. */
const notes = (channel, list, division) => {
    const events = []
    for (const [tick, note, vel] of list) {
        events.push([tick, 0x90 | channel, note, vel])
        events.push([tick + Math.max(1, division >> 3), 0x80 | channel, note, 0])
    }
    events.sort((a, b) => a[0] - b[0])
    const out = []
    let cursor = 0
    let status = -1
    for (const [tick, st, a, b] of events) {
        out.push(...vlq(tick - cursor))
        cursor = tick
        if (st !== status) out.push(st)
        status = st
        out.push(a, b)
    }
    return out
}
const program = (channel, p) => [0, 0xc0 | channel, p]
const grid = (division, beats, every, note, vel = 100) =>
    Array.from({ length: Math.floor(beats / every) }, (_, i) => [Math.round(i * every * division), note, vel])

const FILES = {
    'drums-format0.mid': [
        ...header(0, 1, 96),
        ...chunk('MTrk', [
            ...name('Drums'),
            ...tempo(98),
            ...notes(
                9,
                [
                    ...grid(96, 4, 1, 36, 110),
                    ...grid(96, 4, 2, 38, 90).map(([t, n, v]) => [t + 96, n, v]),
                    ...grid(96, 4, 0.25, 42, 64),
                    [348, 46, 80],
                    [0, 49, 127],
                    [200, 75, 40],
                ],
                96,
            ),
            ...end(),
        ]),
    ],
    'band-format1.mid': [
        ...header(1, 6, 480),
        ...chunk('MTrk', [...name('Song'), ...tempo(132), ...meta(0, 0x58, [4, 2, 24, 8]), ...end()]),
        ...chunk('MTrk', [
            ...name('Bass'),
            ...program(0, 33),
            ...notes(0, grid(480, 40, 0.5, 36).map(([t, n], i) => [t, n + (i % 4) * 2, 96 + (i % 3)]), 480),
            ...end(),
        ]),
        ...chunk('MTrk', [
            ...name('Piano'),
            ...program(1, 0),
            ...notes(1, [...grid(480, 8, 2, 60), ...grid(480, 8, 2, 64), ...grid(480, 8, 2, 67)], 480),
            ...end(),
        ]),
        ...chunk('MTrk', [
            ...name('Kit'),
            ...notes(
                9,
                [...grid(480, 8, 1, 35), ...grid(480, 8, 2, 36), ...grid(480, 8, 0.5, 42), ...grid(480, 8, 1, 44)],
                480,
            ),
            ...end(),
        ]),
        ...chunk('MTrk', [...name('FX'), ...program(2, 120), ...notes(2, grid(480, 4, 1, 50), 480), ...end()]),
        ...chunk('MTrk', [...name('Strings'), ...program(3, 48), ...notes(3, grid(480, 2, 1, 55, 70), 480), ...end()]),
    ],
    'sysex-and-oddities.mid': [
        ...header(1, 2, 96),
        ...chunk('MTrk', [...tempo(120), ...end()]),
        ...chunk('MTrk', [
            ...name('Lead'),
            0, 0xf0, 3, 0x7e, 0x7f, 0xf7,
            0, 0xb0, 7, 100,
            0, 0xe0, 0, 64,
            0, 0xd0, 30,
            ...program(4, 80),
            ...notes(4, [[0, 72, 100], [48, 74, 0], [96, 76, 90]], 96),
            ...end(),
        ]),
    ],
    'truncated.mid': [
        ...header(1, 2, 96),
        ...chunk('MTrk', [...tempo(120), ...end()]),
        ...chunk('MTrk', [...name('Drums'), ...notes(9, grid(96, 4, 1, 36), 96), ...end()]),
    ].slice(0, -9),
    'long-format2.mid': [
        ...header(2, 2, 24),
        ...chunk('MTrk', [...name('Hats'), ...notes(9, grid(24, 700, 1, 42, 60), 24), ...end()]),
        ...chunk('MTrk', [...name('Kick'), ...notes(9, grid(24, 64, 4, 36), 24), ...end()]),
    ],
    'named-only.mid': [
        ...header(1, 2, 96),
        ...chunk('MTrk', [...end()]),
        ...chunk('MTrk', [...name('Organ'), ...program(5, 127), ...notes(5, grid(96, 4, 1, 60), 96), ...end()]),
    ],
    'no-notes.mid': [...header(1, 1, 96), ...chunk('MTrk', [...name('Empty'), ...tempo(90), ...end()])],
    'garbage.mid': [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
    'header-only.mid': header(0, 0, 96),
    'velocity-zero.mid': [
        ...header(0, 1, 120),
        ...chunk('MTrk', [
            0, 0x99, 38, 0,
            30, 38, 50,
            30, 0x89, 38, 64,
            0, 0x99, 37, 127,
            ...end(),
        ]),
    ],
}

// ─── Song patterns, as the app boots them ────────────────────────────────────

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

const song = await readJson('assets/data/song.json')
const edge = JSON.parse(await readFile(join(root, 'crates/ordrumbox-core/tests/fixtures/engine_cases.json'), 'utf8'))
const midiEdge = {
    patterns: [
        {
            id: 'midi-edge',
            name: 'MIDI edge',
            bpm: 97.3,
            beatCount: 4,
            tracks: [
                {
                    name: 'KICK',
                    pitch: 2,
                    velocityLfo: { freq: 1, min: 0.2, max: 1, type: 'sine' },
                    notes: [{ beat: 0 }, { beat: 1, velocity: 1 }, { beat: 2, pitch: 1.5 }, { beat: 3, velocity: 0.33 }],
                },
                {
                    name: 'PIANO',
                    pitchLfo: { freq: 2, min: -3, max: 3, type: 'triangle' },
                    notes: [{ beat: 0, pitch: 4 }, { beat: 1, beatStep: 2, pitch: 7 }, { beat: 3, pitch: 90 }],
                },
                { name: 'BASS', notes: [{ beat: 0, pitch: -40 }, { beat: 2, velocity: 0 }] },
                { name: 'BASS', notes: [{ beat: 1, pitch: 3 }] },
                { name: 'Lead synth', notes: [{ beat: 0 }, { beat: 0, pitch: 7 }] },
                { name: 'MY LOOP ☃', notes: [{ beat: 1 }, { beat: 1 }] },
                { name: 'SNARE', mute: true, notes: [{ beat: 1 }] },
                { name: 'CHH', notes: [] },
                { name: 'OHH', notes: [{ beat: 0, beatStep: 2 }, { beat: 1, beatStep: 2 }, { beat: 2, beatStep: 2 }] },
            ],
        },
        {
            id: 'midi-solo',
            name: 'solo',
            beatCount: 2,
            tracks: [
                { name: 'KICK', solo: true, notes: [{ beat: 0 }, { beat: 1 }] },
                { name: 'SNARE', notes: [{ beat: 1 }] },
            ],
        },
    ],
}
const sources = [...song.patterns, ...(edge.patterns ?? []), ...midiEdge.patterns]
const loadedPatterns = sources.map((p) => importAsApp(fixPatterns([structuredClone(p)])[0]))

const exporter = new MidiExporter()
const exports = []
const exportedFiles = {}
for (const [i, pattern] of loadedPatterns.entries()) {
    for (const loops of [1, 3]) {
        const runs = RANDOM_STREAMS.map((stream) => {
            Math.random = stream()
            return Array.from(exporter.export(pattern, { loops }))
        })
        const bytes = runs[0]
        const deterministic = runs.every((r) => r.length === bytes.length && r.every((b, j) => b === bytes[j]))
        if (!deterministic) continue
        exports.push({ source: sources[i], loops, bytes })
        if (loops === 1 && i % 9 === 0) exportedFiles[`export-${pattern.id || i}.mid`] = bytes
    }
}

// ─── Parse and import ────────────────────────────────────────────────────────

const allFiles = { ...FILES, ...exportedFiles }
const files = []
const imports = []
const registry = {
    cmd: {
        addPattern: (name) => {
            const pattern = { id: '', name, description: '', tags: [], tracks: [], bpm: 120, beatCount: 4 }
            appState.patterns.push(pattern)
            return pattern
        },
        addTrack: (pattern, name) => {
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
        },
        addNote: (track, beat, beatStep, pitch = 0) => {
            clampStepsPerBeat(track)
            const note = {
                ...NOTE_DEFAULTS,
                beatStep,
                stepPercent: Math.round((beatStep * 100) / track.stepsPerBeat),
                beat,
                pitch,
            }
            track.notes.push(note)
            return note
        },
        recordTransaction: (_desc, fn) => fn(),
        setSelectedPatternIdx: async () => {},
    },
}
const importer = new MidiImportService({ registry })
const noteOut = (n) => [n.velocity, n.beat, n.beatStep, n.pitch, n.stepPercent]
for (const [fileName, bytes] of Object.entries(allFiles)) {
    let parsed = null
    let error = null
    try {
        const midi = parseMidi(new Uint8Array(bytes))
        parsed = {
            header: midi.header,
            tracks: midi.tracks,
            trackNames: midi.trackNames,
            notes: findAllNotes(midi),
            programs: [...extractProgramChanges(midi).entries()],
        }
    } catch (e) {
        error = String(e)
    }
    files.push({ name: fileName, bytes, parsed, error })

    appState.patterns = []
    let result
    try {
        result = await importer.importFile({ name: fileName, size: bytes.length, arrayBuffer: async () => new Uint8Array(bytes).buffer })
    } catch (e) {
        result = { error: String(e) }
    }
    imports.push({
        name: fileName,
        result,
        patterns: appState.patterns.map((p) => ({
            name: p.name,
            bpm: p.bpm,
            beatCount: p.beatCount,
            tracks: p.tracks.map((t) => ({
                name: t.name,
                beatCount: t.beatCount,
                stepsPerBeat: t.stepsPerBeat,
                loopAtStep: t.loopAtStep,
                notes: t.notes.map(noteOut),
            })),
        })),
    })
}

const out = join(root, 'crates/ordrumbox-logic/tests/fixtures/midi.json')
await mkdir(dirname(out), { recursive: true })
await writeFile(out, JSON.stringify({ sounds, sort, chaosSorts, blockSorts, files, exports, imports }) + '\n')
process.stdout.write(
    `${sort.length} sorts, ${files.length} files, ${exports.length} exports (of ${loadedPatterns.length * 2}), ${imports.length} imports\n`,
)
