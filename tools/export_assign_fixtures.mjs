#!/usr/bin/env node
// Instrument catalog and golden fixtures for sound assignment, song playback
// and WAV encoding, produced by the JS app's own modules.
//
//   node tools/export_assign_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes:
//   crates/ordrumbox-core/data/instruments.json   the instrument catalog as
//       InstrumentsManager holds it once loaded (GM keys/programs filled in),
//       plus the GM drum and program names. This is source data for the port,
//       not a fixture: regenerate it when instruments_data.js changes.
//   crates/ordrumbox-core/tests/fixtures/instruments.json   lookups
//       (findById, findByName, findInstrumentFromFileName, MIDI) for many names.
//   crates/ordrumbox-core/tests/fixtures/song_sources.json   resolveSongSources()
//       of the demo song and of a hand-made one, tick by tick.
//   crates/ordrumbox-engine/tests/fixtures/assign.json   AutoAssign results for
//       every track name of song.json (and more) in every kit, with Math.random
//       pinned to 0 (first candidate) and to 0.9999 (last candidate).
//   crates/ordrumbox-engine/tests/fixtures/wav.json   bufferToWav() bytes.
//   crates/ordrumbox-engine/tests/fixtures/drumkits.json   the kits used (a copy).

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_assign_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}
const print = (text) => process.stdout.write(text + '\n')

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href
const readJson = async (rel) => JSON.parse(await readFile(resolve(jsRepo, rel), 'utf8'))

const { instrumentsManager, GM_DRUM_NAMES, GM_PROGRAM_NAMES } = await import(
    srcUrl('logic/services/instruments_manager/index.js')
)
const { default: AutoAssign } = await import(srcUrl('logic/services/auto_assign.js'))
const { resolveSongSources } = await import(srcUrl('logic/song_playback.js'))
const { bufferToWav } = await import(srcUrl('audio/export/wav_encoder.js'))
const { fixSongs, fixPatterns } = await import(srcUrl('patterns/fixer.js'))

// ─── Catalog ─────────────────────────────────────────────────────────────────

const catalog = {
    instruments: [...instrumentsManager.instrumentById.values()].map((inst) => ({
        id: inst.id,
        drum: inst.drum,
        pan: String(inst.pan),
        synonyms: inst.synonyms,
        subst: Object.values(inst.subst ?? {}),
        midi: inst.midi.map((m) => ({
            channel: m.channel,
            name: m.name,
            key: m.key,
            program: m.program,
            keyBased: m.keyBased,
        })),
    })),
    gmDrumNames: GM_DRUM_NAMES,
    gmProgramNames: GM_PROGRAM_NAMES,
}

// ─── Lookups ─────────────────────────────────────────────────────────────────

const song = await readJson('assets/data/song.json')
const kits = await readJson('assets/data/drumkits.json')
const trackNames = [...new Set(song.patterns.flatMap((p) => p.tracks.map((t) => t.name)))]
const sampleKeys = [...new Set(kits.flatMap((k) => k.instruments.map((i) => i.key)))]
const extraNames = [
    'kick_01', 'Kick 2', 'BD', 'bd808', 'HH open', 'hat', 'open-hat', 'Tambourine', 'xyz', '', 'GUITAR', 'e guitar',
    'Snare Rim', 'LOW TOM', 'tom_hi', 'conga low', 'Shaker', 'piano chords', 'SUB BASS', 'clap_2', 'Maracas',
    'woodblock', 'COW BELL', 'Ride', 'crash cymbal', 'china', 'FX riser', 'vox', 'Électro kick', 'SAX', 'mt',
]
const names = [...new Set([...trackNames, ...sampleKeys, ...extraNames])]
const id = (inst) => inst?.id ?? null
const lookups = names.map((name) => ({
    name,
    byId: id(instrumentsManager.findById(name)),
    byIdDrum: instrumentsManager.findById(name).drum,
    byName: id(instrumentsManager.findByName(name)),
    fromFileName: id(instrumentsManager.findInstrumentFromFileName(name)),
}))
const midi = []
for (const channel of [9, 0]) {
    for (let key = 30; key < 90; key++) {
        midi.push({ channel, key, id: id(instrumentsManager.findInstrumentFromMidi(channel, key)) })
    }
}
const programs = []
for (let program = 0; program < 128; program++) {
    programs.push({ program, id: id(instrumentsManager.findInstrumentFromMidiProgram(program)) })
}

// ─── Song sources ────────────────────────────────────────────────────────────

const TICK = 32
const patterns = fixPatterns(structuredClone(song.patterns))
const patternsById = new Map(patterns.map((p) => [p.id, p]))
const { songs } = fixSongs(structuredClone(song.songs ?? []), patterns)
const handMade = {
    id: 'hand',
    name: 'hand',
    bpm: 100,
    loopMeasureCount: 5,
    clips: [
        { pattern: 'rock', startMeasure: 0, measureCount: 3 }, // 8 beats: 2 measures per loop
        { pattern: 'disco', startMeasure: 1, measureCount: 1.5 }, // overlaps, fractional length
        { pattern: patterns.find((p) => (p.beatCount ?? 4) === 3)?.id ?? 'rock', startMeasure: 3, measureCount: 2 },
        { pattern: 'nowhere', startMeasure: 4, measureCount: 1 },
    ],
}
const sourceCases = []
for (const s of [songs[0], handMade].filter(Boolean)) {
    const ticks = []
    const total = (s.loopMeasureCount ?? 70) * 4 * TICK
    for (let tick = 0; tick < total + 3 * 4 * TICK; tick += 1) {
        if (tick % 8 !== 0 && tick % 128 !== 127) continue
        const sources = resolveSongSources(s, patternsById, tick, TICK).map((src) => ({
            pattern: src.pattern.id,
            clipIndex: src.clipIndex,
            loop: src.loop,
            localStep: src.localStep,
            patternTicks: src.patternTicks,
        }))
        ticks.push({ tick, sources })
    }
    sourceCases.push({ song: s, ticks })
}

// ─── Auto-assign ─────────────────────────────────────────────────────────────

const sounds = {}
for (const kit of kits) {
    for (const sample of kit.instruments) {
        sounds[sample.url] = { kitName: kit.name, url: sample.url, key: sample.key }
    }
}
const soundRegistry = { sounds, drumkitList: kits, generatedSounds: {} }
const assignCases = []
const realRandom = Math.random
for (let kitIdx = 0; kitIdx < kits.length; kitIdx++) {
    const assign = new AutoAssign({ appState: { selectedDrumkitIdx: kitIdx }, soundRegistry })
    for (const name of names) {
        const result = { kit: kitIdx, name }
        for (const [label, value] of [['first', 0], ['last', 0.9999]]) {
            Math.random = () => value
            const track = { name, useAutoAssignSound: true, useSoftSynth: false, sampleId: 'NOT_DEFINED' }
            assign.autoAssignTrackSounds(track)
            result[label] = track.sampleId
        }
        assignCases.push(result)
    }
}
Math.random = realRandom

// ─── WAV ─────────────────────────────────────────────────────────────────────

const frames = 40
const left = Float32Array.from({ length: frames }, (_, i) => Math.fround(Math.sin(i * 0.7) * 1.3))
const right = Float32Array.from({ length: frames }, (_, i) => Math.fround((i % 7) / 3.5 - 1))
const fakeBuffer = { numberOfChannels: 2, length: frames, sampleRate: 44100, getChannelData: (c) => (c ? right : left) }
const wavBytes = Buffer.from(await bufferToWav(fakeBuffer).arrayBuffer())

// ─── Write ───────────────────────────────────────────────────────────────────

const write = async (rel, value) => {
    const path = join(root, rel)
    await mkdir(dirname(path), { recursive: true })
    await writeFile(path, JSON.stringify(value) + '\n')
}
await write('crates/ordrumbox-core/data/instruments.json', catalog)
await write('crates/ordrumbox-core/tests/fixtures/instruments.json', { lookups, midi, programs })
await write('crates/ordrumbox-core/tests/fixtures/song_sources.json', sourceCases)
await write('crates/ordrumbox-engine/tests/fixtures/assign.json', assignCases)
await writeFile(join(root, 'crates/ordrumbox-engine/tests/fixtures/drumkits.json'), JSON.stringify(kits, null, 2) + '\n')
await write('crates/ordrumbox-engine/tests/fixtures/wav.json', {
    left: [...left],
    right: [...right],
    sampleRate: 44100,
    bytes: wavBytes.toString('base64'),
})
print(
    `instruments: ${catalog.instruments.length}, lookups: ${lookups.length}, song ticks: ` +
        `${sourceCases.map((c) => c.ticks.length).join('+')}, assign cases: ${assignCases.length}`,
)
