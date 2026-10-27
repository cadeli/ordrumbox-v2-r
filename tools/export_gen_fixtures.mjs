#!/usr/bin/env node
// Generator tables and golden fixtures for the pattern generators, produced by
// the JS app's own modules with Math.random replaced by a seeded stream.
//
//   node tools/export_gen_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes:
//   crates/ordrumbox-logic/data/generators.json   the variant tables of every
//       generator and the SongStructure tables. Source data for the port, not
//       a fixture: regenerate it when a generator's configs change.
//   crates/ordrumbox-logic/tests/fixtures/generators.json
//       - tracks: every variant of every generator (plus an unknown name and
//         none) on several track setups, densities and seeds;
//       - random: RandomGenerator.generateRandom;
//       - patterns: AutoGenerator.generatePattern on a new pattern and on
//         existing ones;
//       - loops: the player's loop-start regeneration (Player#handleLoopStart)
//         loop after loop, `track.auto` tracks and an `autoGen` pattern;
//       - toggles: PatternAutoGen.toggleDrums / toggleMelodic;
//       - scales: assets/data/scales.json, which the generators read.
//     Each case gives its seed and the number of Math.random() draws.
//
// The random stream is mulberry32 (the Rust test has the same 10 lines).

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_gen_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href
const readJson = async (rel) => JSON.parse(await readFile(resolve(jsRepo, rel), 'utf8'))

// ─── Random stream ───────────────────────────────────────────────────────────

let draws = 0
const mulberry32 = (seed) => () => {
    seed = (seed + 0x6d2b79f5) >>> 0
    let t = seed
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}
const seedRandom = (seed) => {
    const next = mulberry32(seed)
    draws = 0
    Math.random = () => {
        draws++
        return next()
    }
}

// ─── The app ─────────────────────────────────────────────────────────────────

const { soundRegistry } = await import(srcUrl('state/sound_registry.js'))
const { appState } = await import(srcUrl('state/app_state.js'))
const { serviceRegistry, getAutoGeneratorService } = await import(srcUrl('state/service_loader.js'))
const { normalizeTrack } = await import(srcUrl('model/track_schema.js'))
const { getPanFromTrackName, isMelodicTrack } = await import(srcUrl('core/drum_taxonomy.js'))
const { NOTE_DEFAULTS } = await import(srcUrl('core/note_schema.js'))
const { PatternAutoGen } = await import(srcUrl('logic/services/pattern_auto_gen.js'))
const gen = async (name) => (await import(srcUrl(`logic/generators/${name}.js`))).default
const KickGenerator = await gen('kick_generator')
const SnareGenerator = await gen('snare_generator')
const HatGenerator = await gen('hat_generator')
const ClapGenerator = await gen('clap_generator')
const CowbellGenerator = await gen('cowbell_generator')
const PercGenerator = await gen('perc_generator')
const MelodyGenerator = await gen('melody_generator')
const BassGenerator = await gen('bass_generator')
const RandomGenerator = await gen('random_generator')
const SongStructure = await gen('song_structure')

soundRegistry.scales = await readJson('assets/data/scales.json')

// NoteCommands.addNote and TrackCommands.createTrack, without the undo
// records and persistence (the AutoGenerator routes its notes through them).
const addNote = (track, beat, beatStep, pitch = 0) => {
    const stepPercent = Math.round((beatStep * 100) / track.stepsPerBeat)
    const note = { ...NOTE_DEFAULTS, beatStep, stepPercent, beat, pitch }
    track.notes.push(note)
    return note
}
const createTrack = (beatCount, name, stepsPerBeat = 4) =>
    normalizeTrack({ name, beatCount, stepsPerBeat, loopAtStep: beatCount * stepsPerBeat, pan: getPanFromTrackName(name) })
const addTrack = (pattern, name) => {
    const track = createTrack(pattern.beatCount, name, 4)
    pattern.tracks.push(track)
    return track
}
serviceRegistry.cmd = {
    addNote,
    addTrack,
    addPattern: () => {
        throw new Error('addPattern is not expected here')
    },
    beginGenerationUndo: () => true,
    commitGenerationUndo: () => {},
    cancelGenerationUndo: () => {},
}
serviceRegistry.flatNotes = { applyFlatNotes: () => {} }
serviceRegistry.autoAssign = { autoAssignSounds: () => {} }
const autoGen = await getAutoGeneratorService()

// ─── Output shapes ───────────────────────────────────────────────────────────

const NOTE_KEYS = Object.keys(NOTE_DEFAULTS).filter((k) => !k.startsWith('_'))
// [velocity, beat, beatStep, pitch, {other non-default keys, stepPercent}]
const noteOut = (note) => {
    const extra = {}
    for (const key of NOTE_KEYS) {
        if (['velocity', 'beat', 'beatStep', 'pitch'].includes(key)) continue
        if (JSON.stringify(note[key]) !== JSON.stringify(NOTE_DEFAULTS[key])) extra[key] = note[key]
    }
    if (note.stepPercent !== undefined) extra.stepPercent = note.stepPercent
    for (const key of Object.keys(note)) {
        if (!(key in NOTE_DEFAULTS) && key !== 'stepPercent') throw new Error(`unexpected note key ${key}`)
    }
    const out = [note.velocity, note.beat, note.beatStep, note.pitch]
    if (Object.keys(extra).length > 0) out.push(extra)
    return out
}
const TRACK_KEYS = [
    'name', 'beatCount', 'stepsPerBeat', 'loopAtStep', 'swingAmount', 'swingResolution', 'velocity', 'auto',
    'useSoftSynth', 'useAutoAssignSound', 'synthSoundKey', 'auto_variant', 'auto_density', 'auto_variation',
    'auto_variation2', '_toolbarAuto',
]
const trackOut = (track) => {
    const out = {}
    for (const key of TRACK_KEYS) if (track[key] !== undefined) out[key] = track[key]
    out.notes = track.notes.map(noteOut)
    return out
}
const PATTERN_KEYS = ['beatCount', 'autoGen', '_autoGenGenre', '_autoGenKeyOffset', '_autoGenScale']
const patternOut = (pattern) => {
    const out = {}
    for (const key of PATTERN_KEYS) if (pattern[key] !== undefined) out[key] = pattern[key]
    out.tracks = pattern.tracks.map(trackOut)
    return out
}

// ─── Tables ──────────────────────────────────────────────────────────────────

const tables = {
    kick: KickGenerator.KICK_GENERATION_CONFIGS,
    snare: SnareGenerator.SNARE_GENERATION_CONFIGS,
    hat: HatGenerator.HAT_GENERATION_CONFIGS,
    clap: ClapGenerator.CLAP_GENERATION_CONFIGS,
    cowbell: CowbellGenerator.COWBELL_GENERATION_CONFIGS,
    perc: PercGenerator.PERC_GENERATION_CONFIGS,
    melody: MelodyGenerator.MELODY_GENERATION_CONFIGS,
    bass: BassGenerator.BASS_GENERATION_CONFIGS,
    random: RandomGenerator.RANDOM_CONFIG,
    structure: {
        genres: SongStructure.GENRES,
        bassVariantsByGenre: SongStructure.BASS_VARIANTS_BY_GENRE,
        styleToGenre: SongStructure.STYLE_TO_GENRE,
        chordProgressions: SongStructure.CHORD_PROGRESSIONS,
        harmonicTable: SongStructure.HARMONIC_TABLE,
        swingByGenre: SongStructure.SWING_BY_GENRE,
        structures: SongStructure.STRUCTURES,
        variantPools: SongStructure.VARIANT_POOLS,
        variantPoolsByType: SongStructure.VARIANT_POOLS_BY_TYPE,
        coreTracks: SongStructure.CORE_TRACKS,
        dropChance: SongStructure.DROP_CHANCE,
        optionalExtras: SongStructure.OPTIONAL_EXTRAS,
        keyOffsets: SongStructure.KEY_OFFSETS,
        scales: SongStructure.SCALES,
        sections: new SongStructure().structure,
    },
}

// ─── Generators, one track at a time ─────────────────────────────────────────

const generators = {
    kick: { make: () => new KickGenerator(), run: (g, t, v, d) => g.generateNewKick(t, v, d), names: ['KICK'] },
    snare: { make: () => new SnareGenerator(), run: (g, t, v, d) => g.generateNewSnare(t, v, d), names: ['SNARE'] },
    hat: { make: () => new HatGenerator(), run: (g, t, v, d) => g.generateNewHat(t, v, d), names: ['CHH', 'OHH'] },
    clap: { make: () => new ClapGenerator(), run: (g, t, v, d) => g.generateNewClap(t, v, d), names: ['CLAP'] },
    cowbell: {
        make: () => new CowbellGenerator(),
        run: (g, t, v, d) => g.generateNewCowbell(t, v, d),
        names: ['COWBELL'],
    },
    perc: {
        make: () => new PercGenerator(),
        run: (g, t, v, d) => g.generateNewPerc(t, v, d),
        names: ['CONGAS', 'HCONGA', 'LO_TOM', 'TAMBOURINE'],
    },
    melody: {
        make: () => new MelodyGenerator(),
        run: (g, t, v, d, h, p) => g.generateNewMelody(t, v, d, p, h),
        names: ['PIANO', 'ORGAN'],
        extra: ['intro', 'outro'],
    },
    bass: { make: () => new BassGenerator(), run: (g, t, v, d, h) => g.generateNewBass(t, v, d, h), names: ['BASS'] },
}
const SETUPS = [
    [4, 4],
    [3, 3],
    [8, 8],
    [1, 2],
]
const HARMONIES = [
    { root: 0, scale: null },
    { root: 5, scale: 'dorian' },
    { root: 10, scale: 'blues scale' },
]
const tracks = []
let seed = 1
for (const [kind, g] of Object.entries(generators)) {
    const variants = [...Object.keys(tables[kind]), ...(g.extra ?? []), 'unknownVariant', null]
    for (const variant of variants) {
        for (const [i, [beatCount, stepsPerBeat]] of SETUPS.entries()) {
            for (const density of [1, 0.5]) {
                const name = g.names[(i + (density === 1 ? 0 : 1)) % g.names.length]
                const harmony = HARMONIES[(i + tracks.length) % HARMONIES.length]
                // melody: half the cases with a bass line in the pattern (the chord pitches avoid its root)
                const bass = kind === 'melody' && i % 2 === 1 ? [-12, -5, -12, 0, -12] : null
                seed++
                seedRandom(seed)
                const generator = g.make()
                generator.addNoteFn = addNote
                const track = createTrack(beatCount, name, stepsPerBeat)
                const pattern = { tracks: [track] }
                if (bass) {
                    const bassTrack = createTrack(beatCount, 'BASS', 4)
                    bass.forEach((pitch, n) => addNote(bassTrack, n % beatCount, 0, pitch))
                    pattern.tracks.unshift(bassTrack)
                }
                g.run(generator, track, variant, density, harmony, pattern)
                tracks.push({
                    kind,
                    variant,
                    name,
                    beatCount,
                    stepsPerBeat,
                    density,
                    harmony,
                    bass,
                    seed,
                    draws,
                    track: trackOut(track),
                })
            }
        }
    }
}

const random = []
for (const [beatCount, stepsPerBeat] of [...SETUPS, [16, 4]]) {
    for (let k = 0; k < 3; k++) {
        seed++
        seedRandom(seed)
        const track = createTrack(beatCount, 'T1', stepsPerBeat)
        new RandomGenerator().generateRandom(track, { beatCount: 4 })
        random.push({ beatCount, stepsPerBeat, seed, draws, track: trackOut(track) })
    }
}

// ─── Whole patterns ──────────────────────────────────────────────────────────

const newPattern = (beatCount, names, extra = {}) => {
    const pattern = { name: 'P', beatCount, bpm: 120, tags: [], tracks: [], ...extra }
    for (const name of names) addTrack(pattern, name)
    return pattern
}
const select = (pattern) => {
    appState.patterns = [pattern]
    appState.selectedPatternIdx = 0
}
const EXISTING = ['KICK', 'SNARE', 'CHH', 'OHH', 'CLAP', 'BASS', 'PIANO', 'CONGAS', 'CRASH', 'COWBELL', 'HI_TOM']
const PATTERN_CASES = [
    { beatCount: 4, names: [] },
    { beatCount: 4, names: [] },
    { beatCount: 4, names: [] },
    { beatCount: 2, names: [] },
    { beatCount: 8, names: [] },
    { beatCount: 4, names: EXISTING },
    { beatCount: 4, names: EXISTING.filter((n) => n !== 'BASS') },
    { beatCount: 4, names: ['KICK', 'SNARE', 'T3'], extra: { _autoGenKeyOffset: 7, _autoGenScale: 'phrygian' } },
    { beatCount: 3, names: ['808 KICK', 'SD', 'OPEN HAT', 'SYNTH LEAD', 'ORGAN', 'TOM'] },
    { beatCount: 4, names: [], options: { genre: 'reggae', keyOffset: 2, scale: 'major' } },
]
const patterns = []
for (const c of PATTERN_CASES) {
    for (let k = 0; k < 2; k++) {
        seed++
        seedRandom(seed)
        const pattern = newPattern(c.beatCount, c.names, c.extra)
        const before = patternOut(pattern)
        select(pattern)
        await autoGen.generatePattern(c.options ?? {})
        patterns.push({ seed, draws, options: c.options ?? null, before, after: patternOut(pattern) })
    }
}

// ─── Loop after loop ─────────────────────────────────────────────────────────

// Player#handleLoopStart, as written there.
const handleLoopStart = async (selectedPattern, loop) => {
    const tracks = selectedPattern.tracks
    const trackKeys = Object.keys(tracks)
    if (selectedPattern.autoGen) {
        const autoGen = await getAutoGeneratorService()
        const element = autoGen.structureGen.getElement(loop)
        const isSectionStart = element.loopInElement === 0
        const isSectionEnd = element.isLastLoopBeforeChange
        const isHarmonicBoundary = isSectionStart || isSectionEnd
        const promises = []
        for (let i = 0; i < trackKeys.length; i++) {
            const track = tracks[trackKeys[i]]
            if (!isHarmonicBoundary && !isMelodicTrack(track)) continue
            promises.push(autoGen.changeTrack(loop, selectedPattern, track))
        }
        await Promise.all(promises)
    } else {
        const promises = []
        for (let i = 0; i < trackKeys.length; i++) {
            const track = tracks[trackKeys[i]]
            if (track.auto === true) {
                promises.push(
                    (async () => {
                        const autoGen = await getAutoGeneratorService()
                        return autoGen.changeTrack(loop, selectedPattern, track)
                    })(),
                )
            }
        }
        await Promise.all(promises)
    }
}
const LOOP_TRACKS = ['KICK', 'SNARE', 'CHH', 'OHH', 'BASS', 'PIANO', 'CONGAS', 'CLAP', 'COWBELL']
const loopCase = (setup) => {
    const pattern = newPattern(4, LOOP_TRACKS, setup.pattern)
    for (const track of pattern.tracks) {
        track.auto = track.name !== 'CLAP'
        Object.assign(track, setup.tracks?.[track.name] ?? {})
    }
    return pattern
}
const LOOP_CASES = [
    { loops: 46, pattern: { _autoGenGenre: 'house', _autoGenKeyOffset: 3, _autoGenScale: 'dorian' } },
    { loops: 46, pattern: { autoGen: true } },
    {
        loops: 30,
        pattern: { _autoGenGenre: 'techno' },
        tracks: {
            KICK: { auto_density: 0.5 },
            SNARE: { auto_variant: 'ghost' },
            BASS: { auto_variant: 'acid', auto_variation: 0 },
            PIANO: { auto_variant: 'walking' },
            CHH: { auto_variant: 'chhRoll', stepsPerBeat: 3 },
        },
    },
    { loops: 30, pattern: { _autoGenGenre: 'funk', autoGen: true } },
]
const loops = []
for (const c of LOOP_CASES) {
    seed++
    seedRandom(seed)
    const pattern = loopCase(c)
    const before = patternOut(pattern)
    const after = []
    for (let loop = 0; loop < c.loops; loop++) {
        const start = draws
        await handleLoopStart(pattern, loop)
        after.push({ draws: draws - start, pattern: patternOut(pattern) })
    }
    loops.push({ seed, autoGen: !!c.pattern.autoGen, before, after })
}

// ─── Toolbar toggles ─────────────────────────────────────────────────────────

const toggler = new PatternAutoGen(serviceRegistry)
const TOGGLE_CASES = [
    { names: ['KICK', 'SNARE', 'CHH', 'BASS'], steps: ['drums', 'drums', 'drums'] },
    { names: ['KICK', 'SNARE', 'OHH', 'PIANO'], steps: ['bass', 'chords', 'bass', 'drums', 'chords'] },
    { names: ['KICK', 'BASS'], extra: { _autoGenGenre: 'disco' }, steps: ['bass', 'bass', 'chords'] },
    { names: [], steps: ['drums', 'bass', 'chords'] },
]
const toggles = []
for (const c of TOGGLE_CASES) {
    for (let k = 0; k < 2; k++) {
        seed++
        seedRandom(seed)
        const pattern = newPattern(4, c.names, c.extra)
        if (k === 1) for (const t of pattern.tracks) if (t.name === 'KICK') t._toolbarAuto = true
        const before = patternOut(pattern)
        select(pattern)
        const after = []
        for (const step of c.steps) {
            const start = draws
            if (step === 'drums') await toggler.toggleDrums()
            else await toggler.toggleMelodic(step === 'bass' ? 'BASS' : 'PIANO')
            after.push({ step, draws: draws - start, pattern: patternOut(pattern) })
        }
        toggles.push({ seed, before, after })
    }
}

// ─── Write ───────────────────────────────────────────────────────────────────

const write = async (rel, value) => {
    const path = join(root, rel)
    await mkdir(dirname(path), { recursive: true })
    await writeFile(path, JSON.stringify(value) + '\n')
    process.stdout.write(`${rel}\n`)
}
await write('crates/ordrumbox-logic/data/generators.json', tables)
await write('crates/ordrumbox-logic/tests/fixtures/generators.json', {
    scales: soundRegistry.scales,
    tracks,
    random,
    patterns,
    loops,
    toggles,
})
process.stdout.write(
    `${tracks.length} track cases, ${random.length} random, ${patterns.length} patterns, ` +
        `${loops.length} loop runs, ${toggles.length} toggle runs\n`,
)
