#!/usr/bin/env node
// Golden fixtures for the DSP port, produced by the JS app's own processors.
//
//   node tools/export_dsp_fixtures.mjs <path-to-ordrumbox-v2>
//
// The AudioWorklet processors (strip, master bus, synth voice) are template
// strings in ordrumbox-v2; they are evaluated here with a minimal AudioWorklet
// scope (AudioWorkletProcessor, registerProcessor, sampleRate, currentFrame)
// and run quantum by quantum (128 frames), as a browser does. Writes:
//
// crates/ordrumbox-dsp/tests/fixtures/
//   renders.json  one entry per case: processor, sample rate, frames, the
//                 parameters (or synth messages) it ran with, and where its
//                 output sits in renders.bin
//   renders.bin   the outputs, f32 little-endian, left then right channel,
//                 thinned out to keep the file small: every frame below
//                 FULL_FRAMES, then one frame in STRIDE (see `kept`)
//
// crates/ordrumbox-engine/tests/fixtures/
//   strip_settings.json  the strip parameters strip_sync.js/strip.js set for
//                        every distinct track setup of song.json (+ edge cases)
//   lfo_values.json      computeLfoValue() for the track LFOs of song.json
//                        (+ edge cases) over a range of ticks
//   synth_patches.json   the `update` message WorkletSynthVoice sends for every
//                        preset of generated_sounds.json (+ note variants)
//
// Inputs are built from integer arithmetic only (an LCG and exact products),
// so the Rust tests regenerate them bit for bit.

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_dsp_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
console.warn = () => {}
console.error = () => {}

const here = dirname(fileURLToPath(import.meta.url))
const dspOut = join(here, '..', 'crates', 'ordrumbox-dsp', 'tests', 'fixtures')
const engineOut = join(here, '..', 'crates', 'ordrumbox-engine', 'tests', 'fixtures')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href
const QUANTUM = 128

// ─── AudioWorklet scope ──────────────────────────────────────────────────────

const processors = {}
globalThis.AudioWorkletProcessor = class {
    constructor() {
        this.port = { onmessage: null, postMessage() {} }
    }
}
globalThis.registerProcessor = (name, cls) => {
    processors[name] = cls
}
globalThis.sampleRate = 48000
globalThis.currentFrame = 0

for (const file of ['strip_source.js', 'master_bus_source.js', 'synth_voice_source.js']) {
    const { default: source } = await import(srcUrl(`audio/worklets/processors/${file}`))
    new Function(source)()
}

// ─── Inputs (mirrored in the Rust tests) ─────────────────────────────────────

/**
 * Bursts of decaying noise with a click every 3000 frames. Only exact
 * arithmetic: LCG integers, divisions by powers of two, repeated products.
 */
function burst(frames, seed, gain) {
    const out = new Float32Array(frames)
    let s = seed >>> 0
    let env = 0
    for (let i = 0; i < frames; i++) {
        s = (Math.imul(s, 1664525) + 1013904223) >>> 0
        if (i % 3000 === 0) env = 1
        const noise = ((s >>> 8) / 16777216) * 2 - 1
        let x = noise * env * 0.6
        if (i % 3000 === 0) x += 0.9
        out[i] = x * gain
        env *= 0.9993
    }
    return out
}

// ─── Rendering ───────────────────────────────────────────────────────────────

const index = []
const chunks = []
let offset = 0

const FULL_FRAMES = 1024
const STRIDE = 4
const kept = (i) => i < FULL_FRAMES || i % STRIDE === 0

function record(entry, left, right) {
    const thin = (channel) => Float32Array.from(channel.filter((_, i) => kept(i)))
    const [l, r] = [thin(left), thin(right)]
    entry.offset = offset
    entry.stored = l.length
    index.push(entry)
    chunks.push(Buffer.from(l.buffer), Buffer.from(r.buffer))
    offset += l.length * 2
}

/** Parameters as AudioParam arrays (k-rate: one float32 value). */
const paramArrays = (values) => Object.fromEntries(Object.entries(values).map(([k, v]) => [k, new Float32Array([v])]))

const defaultsOf = (name) =>
    Object.fromEntries(processors[name].parameterDescriptors.map((d) => [d.name, d.defaultValue ?? 0]))

/**
 * Run an effect processor (strip, master-bus) on a stereo input. `changes`
 * is a list of `{ quantum, params }`: from that quantum on, `params` replace
 * the previous values.
 */
function renderEffect(name, sr, frames, input, changes) {
    globalThis.sampleRate = sr
    globalThis.currentFrame = 0
    const proc = new processors[name]()
    const outL = new Float32Array(frames)
    const outR = new Float32Array(frames)
    let params = defaultsOf(name)
    for (let q = 0; q * QUANTUM < frames; q++) {
        for (const change of changes) if (change.quantum === q) params = { ...params, ...change.params }
        const from = q * QUANTUM
        const n = Math.min(QUANTUM, frames - from)
        const inputs = [[input.left.subarray(from, from + n), input.right.subarray(from, from + n)]]
        const outputs = [[new Float32Array(n), new Float32Array(n)]]
        globalThis.currentFrame = from
        proc.process(inputs, outputs, paramArrays(params))
        outL.set(outputs[0][0], from)
        outR.set(outputs[0][1], from)
    }
    return { left: outL, right: outR }
}

// ─── Strip ───────────────────────────────────────────────────────────────────

const STRIP_FRAMES = 8192
const stripInput = { left: burst(STRIP_FRAMES, 1, 1), right: burst(STRIP_FRAMES, 2, 1) }
const stripCases = [
    ['dry', [{}]],
    ['lowpass', [{ cutoff: 800, q: 2, filterMode: 0 }]],
    ['highpass', [{ cutoff: 2000, q: 0.707, filterMode: 1 }]],
    ['bandpass', [{ cutoff: 1000, q: 5, filterMode: 2 }]],
    ['notch', [{ cutoff: 1000, q: 1, filterMode: 3 }]],
    ['sat_soft', [{ satType: 0, satDrive: 4, satOut: 0.925, satMix: 1 }]],
    ['sat_hard', [{ satType: 1, satDrive: 7, satOut: 0.85, satMix: 1 }]],
    ['sat_tape', [{ satType: 2, satDrive: 2.2, satOut: 0.97, satMix: 1 }]],
    ['reverb_room', [{ revRoom: 0.5, revDamp: 0.5, revWidth: 0.8, revMix: 0.4 }]],
    ['reverb_hall', [{ revRoom: 0.85, revDamp: 0.3, revWidth: 1, revMix: 0.6 }]],
    ['delay_slap', [{ dlyTimeL: 0.03, dlyTimeR: 0.03, dlyMode: 0, dlyMix: 0.5 }]],
    ['delay_pingpong', [{ dlyTimeL: 0.02001, dlyTimeR: 0.03, dlyMode: 2, dlyMix: 0.7 }]],
    [
        'everything',
        [
            {
                cutoff: 3000, q: 4, filterMode: 0, satType: 0, satDrive: 2.5, satOut: 0.96, satMix: 1,
                revRoom: 0.7, revDamp: 0.4, revWidth: 0.9, revMix: 0.5,
                dlyTimeL: 0.025, dlyTimeR: 0.0375, dlyMode: 1, dlyMix: 0.4, volume: 1.5, pan: 0.5,
            },
        ],
    ],
    ['pan_left_quiet', [{ pan: -1, volume: 0.5 }]],
    [
        'changes',
        [
            { revRoom: 0.5, revMix: 0.3 },
            { quantum: 24, cutoff: 500, q: 8, revMix: 0, dlyTimeL: 0.01, dlyTimeR: 0.01, dlyMix: 0.6 },
            { quantum: 40, filterMode: 1, cutoff: 4000, pan: 0.3, dlyMix: 0 },
        ],
    ],
]
for (const [name, steps] of stripCases) {
    const changes = steps.map(({ quantum = 0, ...params }) => ({ quantum, params }))
    const out = renderEffect('strip', 48000, STRIP_FRAMES, stripInput, changes)
    record({ name, processor: 'strip', sampleRate: 48000, frames: STRIP_FRAMES, input: [1, 2, 1], changes }, out.left, out.right)
}
{
    const frames = 8192
    const input = { left: burst(frames, 3, 1), right: burst(frames, 4, 1) }
    const changes = [{ quantum: 0, params: { revRoom: 0.85, revDamp: 0.3, revMix: 0.5, dlyTimeL: 0.02, dlyTimeR: 0.02, dlyMix: 0.5, cutoff: 15000 } }]
    const out = renderEffect('strip', 44100, frames, input, changes)
    record({ name: 'rate_44100', processor: 'strip', sampleRate: 44100, frames, input: [3, 4, 1], changes }, out.left, out.right)
}

// ─── Master bus ──────────────────────────────────────────────────────────────

const MASTER_FRAMES = 8192
const masterInput = { left: burst(MASTER_FRAMES, 5, 2), right: burst(MASTER_FRAMES, 6, 2) }
const masterCases = [
    ['defaults', {}],
    ['bypass', { bypass: 1, master: 0.7 }],
    ['hard_knee_strong', { compThreshold: -30, compRatio: 20, compKnee: 0, preGain: 6 }],
    ['soft_knee_slow', { compThreshold: -12, compRatio: 3, compKnee: 12, compAttack: 0.03, compRelease: 0.4, compMakeup: 2 }],
    ['band', { lowcut: 200, hicut: 3000, master: 0.5, preGain: -6 }],
]
for (const [name, params] of masterCases) {
    const changes = [{ quantum: 0, params }]
    const out = renderEffect('master-bus', 48000, MASTER_FRAMES, masterInput, changes)
    record({ name, processor: 'master-bus', sampleRate: 48000, frames: MASTER_FRAMES, input: [5, 6, 2], changes }, out.left, out.right)
}

// ─── Synth voice (driven by the real WorkletSynthVoice) ──────────────────────

const WorkletLoader = (await import(srcUrl('audio/worklets/worklet_loader.js'))).default
WorkletLoader.ensureLoaded = async () => true
const { default: WorkletSynthVoice } = await import(srcUrl('audio/voices/worklet_synth_voice.js'))
const generatedSounds = JSON.parse(await readFile(resolve(jsRepo, 'assets', 'data', 'generated_sounds.json'), 'utf8'))

/** The messages WorkletSynthVoice posts for one note, in order. */
async function synthMessages(preset, note) {
    const messages = []
    const node = { port: { postMessage: (m) => messages.push(structuredClone(m)) }, connect() {}, disconnect() {} }
    const pool = { acquire: async () => node, release() {} }
    const ctx = { currentTime: 0 }
    const voice = new WorkletSynthVoice(ctx, { voicesInput: {} }, preset, 'x', null, pool, () => note.bpm)
    await voice.setup({ fpitch: note.ratio, pan: note.pan, note: { velocity: note.velocity }, track: { stepsPerBeat: note.stepsPerBeat } }, 0)
    voice.start(note.startTime)
    voice.cleanup() // drop the pool's cleanup timer
    return messages
}

const SYNTH_RATE = 48000
const START_FRAME = 300
const synthNotes = [{ ratio: 1, pan: 0, velocity: 0.8, bpm: 240, stepsPerBeat: 4 }]
const synthCases = Object.keys(generatedSounds).map((key) => [key, key, synthNotes[0]])
synthCases.push(['PIANO_variant', 'PIANO', { ratio: 1.5, pan: 0.5, velocity: 0.4, bpm: 180, stepsPerBeat: 3 }])
synthCases.push(['BASS1_low', 'BASS1', { ratio: 0.5, pan: -0.7, velocity: 1, bpm: 300, stepsPerBeat: 4 }])
// Every LFO target, wave, FM algorithm and modulation-envelope target, on a busy patch.
const busy = (lfo, lfo2, fm, modTarget, extra = {}) => ({
    ...generatedSounds.BASS1,
    vco3: { gain: 0.3, octave: 1, detune: -7, wave: 'triangle' },
    noise: { mix: 0.2, filterType: 'bandpass', filterFreq: 3000, filterQ: 2 },
    lfo,
    lfo2,
    fm,
    modEnvelope: { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05, target: modTarget },
    ...extra,
})
const lfoTargets = ['FLT', 'VCO1', 'VCO2', 'VCO3', 'masterVolume', 'vco1.gain', 'vco1.detune', 'vco1.octave', 'vco2.gain', 'vco2.detune', 'vco2.octave',
    'vco3.gain', 'vco3.detune', 'vco3.octave', 'filter.freq', 'filter.filterEnvelopeAmount', 'filter.Q', 'noise.mix', 'subGain', 'pitchPunch',
    'filter.drive', 'noise.filterFreq', 'noise.filterQ', 'fm.amount', 'fm.algo', 'envelope.attack', 'envelope.decay', 'envelope.sustain',
    'envelope.release', 'modEnvelope.attack', 'modEnvelope.decay', 'modEnvelope.sustain', 'modEnvelope.release']
const waves = ['sine', 'triangle', 'sawtooth', 'square', 'random']
const modTargets = ['off', 'filter', 'pitch', 'fm', 'shape']
const busyNote = { ratio: 1.25, pan: 0.2, velocity: 0.9, bpm: 200, stepsPerBeat: 4 }
lfoTargets.forEach((target, i) => {
    const lfo = { target, wave: waves[i % 5], freq: 3 + i, depth: 0.3, sync: 'off' }
    const lfo2 = { target: lfoTargets[(i + 7) % lfoTargets.length], wave: waves[(i + 2) % 5], freq: 0, depth: 0.5, sync: i % 3 === 0 ? '1/16' : 'off' }
    const fm = { amount: (i % 4) * 0.1, algo: i % 5 }
    synthCases.push([`busy_${i}`, busy(lfo, lfo2, fm, modTargets[i % 5]), busyNote])
})
synthCases.push(['bypassed', busy({ target: 'FLT', wave: 'sine', freq: 2, depth: 0.5 }, null, { amount: 0.3, algo: 1 }, 'pitch', {
    bypassNoise: true, bypassFilter: true, bypassLfo1: true, bypassFm: true, bypassModEnv: true, bypassFilterEnv: true,
}), busyNote])
synthCases.push(['no_env', busy(null, null, null, 'off', { bypassEnv: true, filter: { type: 'notch', freq: 9000, Q: 0.5 } }), busyNote])

const synthPatches = []
for (const [name, presetOrKey, note] of synthCases) {
    const preset = typeof presetOrKey === 'string' ? generatedSounds[presetOrKey] : presetOrKey
    const startTime = START_FRAME / SYNTH_RATE
    const messages = await synthMessages(preset, { ...note, startTime })
    const update = messages.find((m) => m.type === 'update')
    synthPatches.push({ name, preset, note, update })

    globalThis.sampleRate = SYNTH_RATE
    const proc = new processors['synth-voice']()
    for (const m of messages) proc.port.onmessage({ data: m })
    const params = paramArrays(defaultsOf('synth-voice'))
    const left = []
    const right = []
    let frames = 0
    const maxQuanta = name.startsWith('busy_') ? 24 : 120
    for (let q = 0; q < maxQuanta; q++) {
        const outputs = [[new Float32Array(QUANTUM), new Float32Array(QUANTUM)]]
        globalThis.currentFrame = q * QUANTUM
        const alive = proc.process([], outputs, params)
        left.push(...outputs[0][0])
        right.push(...outputs[0][1])
        frames += QUANTUM
        if (!alive) break
    }
    record(
        { name, processor: 'synth-voice', sampleRate: SYNTH_RATE, frames, startFrame: START_FRAME, messages },
        Float32Array.from(left),
        Float32Array.from(right),
    )
}

// ─── Strip settings, LFO values (engine mapping) ─────────────────────────────

const { applyTrackToStrip } = await import(srcUrl('audio/strip_sync.js'))
const { default: Strip } = await import(srcUrl('audio/strip.js'))
const { computeLfoValue } = await import(srcUrl('audio/math.js'))
const { fixPatterns } = await import(srcUrl('patterns/fixer.js'))
const { normalizeTrack } = await import(srcUrl('model/track_schema.js'))

function stripSettingsOf(track, bpm) {
    const values = defaultsOf('strip')
    const param = (name) => ({
        setTargetAtTime: (v) => (values[name] = Math.fround(v)),
        setValueAtTime: (v) => (values[name] = Math.fround(v)),
    })
    const node = { parameters: { get: param }, connect() {}, disconnect() {} }
    WorkletLoader.createNode = () => node
    const fake = () => ({ connect() {}, disconnect() {}, fftSize: 0, frequencyBinCount: 128 })
    const ctx = { currentTime: 0, createGain: fake, createAnalyser: fake }
    const strip = new Strip(track.name, ctx, null)
    // #initNode is private: rebuild what it exposes.
    strip.stripNode = node
    strip.output = { gain: param('volume') }
    strip.pan = { pan: param('pan') }
    strip.setBpm(bpm)
    applyTrackToStrip(strip, track, 0)
    delete values.transportTime
    delete values.bpm
    return values
}

const song = JSON.parse(await readFile(resolve(jsRepo, 'assets', 'data', 'song.json'), 'utf8'))
const patterns = fixPatterns(structuredClone(song.patterns))
const STRIP_KEYS = ['filterType', 'filterFreq', 'filterFreqLfo', 'filterQ', 'filterQLfo', 'saturationType', 'sat', 'saturationAmount',
    'reverbType', 'reverbOn', 'reverbAmount', 'delayType', 'delayOn', 'delayTime', 'delayDepth', 'velocity', 'velocityLfo', 'pan', 'panLfo', 'mute']
const seen = new Set()
const stripSettings = []
const addStripCase = (track, bpm) => {
    const picked = Object.fromEntries(STRIP_KEYS.filter((k) => track[k] !== undefined).map((k) => [k, track[k]]))
    picked.name = track.name
    const key = JSON.stringify([picked, bpm])
    if (seen.has(key)) return
    seen.add(key)
    stripSettings.push({ track: picked, bpm, settings: stripSettingsOf(picked, bpm) })
}
// A pattern without bpm plays at the default tempo (importPatternFromJson).
for (const p of patterns) for (const t of p.tracks) addStripCase(t, p.bpm ?? 120)
const edgeTracks = [
    { name: 'A', filterType: 'notch', filterFreq: 1200, filterQ: 3 },
    { name: 'B', filterType: 'bandpass', filterFreq: 600, filterQ: 9, filterFreqLfo: { freq: 1 }, filterQLfo: { freq: 1 } },
    { name: 'C', saturationType: 'hard', saturationAmount: 0.6, sat: true },
    { name: 'D', saturationType: 'tape', saturationAmount: 1.4, sat: false },
    { name: 'E', reverbType: 'gated', reverbAmount: 0.7, reverbOn: true },
    { name: 'F', reverbType: 'spring', reverbAmount: 0.5, reverbOn: false },
    { name: 'G', reverbType: 'plate', reverbAmount: 0.3 },
    { name: 'H', delayType: 'pingpong', delayTime: 0.75, delayDepth: 0.6, delayOn: true },
    { name: 'I', delayType: 'slap', delayTime: 0.25, delayDepth: 0.3 },
    { name: 'J', delayType: 'none', delayTime: 1, delayDepth: 0.9 },
    { name: 'K', delayType: 'tape', delayTime: 2, delayDepth: 0.5, delayOn: false },
    { name: 'L', velocity: 1.7, pan: -0.4, mute: true },
    { name: 'M', velocity: 0.6, velocityLfo: { freq: 2 }, panLfo: { freq: 1 }, pan: 0.5 },
    { name: 'N', filterType: 'weird', reverbType: 'cathedral', delayType: 'warp', saturationType: 'fuzz', saturationAmount: 0.2 },
    { name: 'O', mute: false, velocity: 0.3 },
]
// Completed with the track defaults, as every loaded track is.
for (const t of edgeTracks) addStripCase(normalizeTrack(t), 97)

const lfoSeen = new Set()
const lfos = []
for (const p of patterns)
    for (const t of p.tracks)
        for (const key of ['velocityLfo', 'panLfo', 'pitchLfo', 'filterFreqLfo', 'filterQLfo']) {
            const lfo = t[key]
            if (!lfo || lfoSeen.has(JSON.stringify(lfo))) continue
            lfoSeen.add(JSON.stringify(lfo))
            lfos.push(lfo)
        }
lfos.push(
    { freq: '0.5', min: '-1', max: 2, phase: 0.25, type: 'sawtooth' },
    { freq: 3, min: 0, max: 1, phase: 0, type: 'square' },
    { freq: 1.5, min: -3, max: 3, phase: 0.1, type: 'random' },
    { freq: 1, min: 0, max: 1, waveform: 'triangle' },
    { freq: 1, min: 0, max: 1, type: '2' },
    { freq: 1, min: 0, max: 1, type: 'bogus' },
    { freq: -1, min: 0, max: 10, phase: -0.3, type: 'sine' },
    { min: 0.2 },
    {},
)
const TICKS = []
for (let tick = 0; tick < 1200; tick += 7) TICKS.push(tick)
const lfoValues = lfos.map((lfo) => ({ lfo, values: TICKS.map((tick) => computeLfoValue(lfo, tick, 128, 'x')) }))

// ─── Write ───────────────────────────────────────────────────────────────────

await mkdir(dspOut, { recursive: true })
await mkdir(engineOut, { recursive: true })
await writeFile(join(dspOut, 'renders.json'), JSON.stringify({ fullFrames: FULL_FRAMES, stride: STRIDE, cases: index }, null, 1) + '\n')
await writeFile(join(dspOut, 'renders.bin'), Buffer.concat(chunks))
await writeFile(join(engineOut, 'strip_settings.json'), JSON.stringify(stripSettings, null, 1) + '\n')
await writeFile(join(engineOut, 'lfo_values.json'), JSON.stringify({ ticks: TICKS, lfos: lfoValues }) + '\n')
await writeFile(join(engineOut, 'synth_patches.json'), JSON.stringify(synthPatches, null, 1) + '\n')
process.stdout.write(
    `renders: ${index.length} (${(offset * 4) / 1e6} MB), strip settings: ${stripSettings.length}, ` +
        `lfos: ${lfos.length}, synth patches: ${synthPatches.length}\n`,
)
