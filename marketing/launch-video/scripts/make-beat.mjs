// Synthesizes the promo soundtrack: an original 120 BPM track, timed to src/Promo.tsx.
// 120 BPM at 30 fps: one beat = 15 frames, one bar = 60 frames. Writes public/audio/promo-beat.wav.
//   STYLE=synthwave bun scripts/make-beat.mjs    Dm Bb F C, half-time drums, 8th-note bass, square lead (default)
//   STYLE=futurebass bun scripts/make-beat.mjs   Fmaj7 Am7 Dm7 Bbmaj7, wobbling supersaw stabs, 808, trap hats, vocal chops
//   STYLE=bach bun scripts/make-beat.mjs         solo piano: J. S. Bach, Prelude in C major BWV 846 (public domain)
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const STYLE = process.env.STYLE ?? 'synthwave'
if (!['synthwave', 'futurebass', 'bach'].includes(STYLE)) throw new Error(`unknown STYLE ${STYLE}`)
const SR = 44100
const BEAT = 0.5
const BAR = BEAT * 4
const LENGTH = 44 // seconds; the video is 1320 frames
const DROP = 4 // full beat starts (frame 120)
const ARP = 12 // arpeggio starts with the feature slides (frame 360)
const END = 39 // beat stops on the end card (frame 1170)

const N = Math.ceil(LENGTH * SR)
const L = new Float32Array(N)
const R = new Float32Array(N)

// ---------- helpers ----------
let seed = 7
const noise = () => {
  seed = (seed * 1664525 + 1013904223) >>> 0
  return seed / 2147483648 - 1
}

/** RBJ biquad; type 'lp' | 'hp' | 'bp'. */
function biquad(type, freq, q = 0.707) {
  let b0, b1, b2, a1, a2
  let x1 = 0, x2 = 0, y1 = 0, y2 = 0
  const set = (f) => {
    const w = (2 * Math.PI * Math.min(f, SR * 0.45)) / SR
    const alpha = Math.sin(w) / (2 * q)
    const cos = Math.cos(w)
    const a0 = 1 + alpha
    if (type === 'lp') [b0, b1, b2] = [(1 - cos) / 2, 1 - cos, (1 - cos) / 2]
    else if (type === 'hp') [b0, b1, b2] = [(1 + cos) / 2, -(1 + cos), (1 + cos) / 2]
    else [b0, b1, b2] = [alpha, 0, -alpha]
    ;[b0, b1, b2, a1, a2] = [b0 / a0, b1 / a0, b2 / a0, (-2 * cos) / a0, (1 - alpha) / a0]
  }
  set(freq)
  const run = (x) => {
    const y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
    x2 = x1; x1 = x; y2 = y1; y1 = y
    return y
  }
  run.set = set
  return run
}

/** Band-limited-ish saw from a few harmonics. */
const saw = (phase, harmonics = 8) => {
  let v = 0
  for (let h = 1; h <= harmonics; h++) v += Math.sin(phase * h) / h
  return v * 0.55
}

const add = (i, v, pan = 0) => {
  if (i < 0 || i >= N) return
  L[i] += v * (1 - pan) * 0.5 * 2
  R[i] += v * (1 + pan) * 0.5 * 2
}

const at = (seconds) => Math.round(seconds * SR)

// ---------- harmony ----------
const hz = (midi) => 440 * 2 ** ((midi - 69) / 12)
// Bass root and pad voicing (MIDI) per bar, and kick positions inside a bar (in beats).
const STYLES = {
  // Dm, Bb, F, C
  synthwave: {
    chords: [
      { bass: 38, pad: [62, 65, 69, 74] },
      { bass: 34, pad: [58, 62, 65, 70] },
      { bass: 41, pad: [57, 60, 65, 69] },
      { bass: 36, pad: [55, 60, 64, 67] },
    ],
    kicks: [0, 2, 2.5],
    padGain: 0.075,
    drive: 1.4,
  },
  // Fmaj7, Am7, Dm7, Bbmaj7
  futurebass: {
    chords: [
      { bass: 41, pad: [57, 60, 64, 65] },
      { bass: 45, pad: [55, 60, 64, 67] },
      { bass: 38, pad: [57, 60, 62, 65] },
      { bass: 34, pad: [57, 58, 62, 65] },
    ],
    kicks: [0, 1.5, 3.25],
    padGain: 0.05,
    drive: 2.6, // the 808 and kick set the peak, so push the soft clipper harder
  },
  // piano only: no pad, no kick, no sidechain
  bach: { chords: [{ bass: 36, pad: [] }], kicks: [], padGain: 0, drive: 1.1 },
}
const { chords: CHORDS, kicks: KICKS, padGain: PAD_GAIN, drive: DRIVE } = STYLES[STYLE]
const chordAt = (t) => CHORDS[Math.floor(t / BAR) % CHORDS.length]

// ---------- sidechain (ducks pad and bass under each kick) ----------
const kickTimes = []
for (let bar = DROP; bar < END - 1e-9; bar += BAR) {
  const preArp = (t) => t >= ARP - BEAT && t < ARP
  for (const t of KICKS.map((beat) => bar + beat * BEAT)) if (t < END - 1e-9 && !preArp(t)) kickTimes.push(t)
}
const duck = (t) => {
  let last = -1
  for (let k = kickTimes.length - 1; k >= 0; k--) if (kickTimes[k] <= t) { last = kickTimes[k]; break }
  if (last < 0) return 1
  return 1 - 0.7 * Math.exp(-(t - last) * 9)
}

// ---------- drums ----------
function kick(t0, gain = 1) {
  let phase = 0
  const start = at(t0)
  for (let i = 0; i < at(0.45); i++) {
    const t = i / SR
    const f = 46 + 120 * Math.exp(-t * 32)
    phase += (2 * Math.PI * f) / SR
    const env = Math.exp(-t * 6.5)
    const click = i < 90 ? noise() * 0.25 * (1 - i / 90) : 0
    add(start + i, (Math.tanh(Math.sin(phase) * 1.6) * env + click) * 0.95 * gain)
  }
}

/** A big gated synthwave snare: noise plus a pitched body, with a long tail. */
function clap(t0, gain = 1) {
  const bp = biquad('bp', 1800, 0.6)
  const start = at(t0)
  let phase = 0
  for (let i = 0; i < at(0.5); i++) {
    const t = i / SR
    phase += (2 * Math.PI * (180 + 60 * Math.exp(-t * 40))) / SR
    const body = Math.sin(phase) * Math.exp(-t * 22) * 0.6
    const tail = bp(noise()) * (t < 0.18 ? Math.exp(-t * 5) : Math.exp(-0.9 - (t - 0.18) * 25))
    add(start + i, (body + tail) * 0.85 * gain, Math.sin(t * 30) * 0.15)
  }
}

function hat(t0, open = false, gain = 1, pan = 0) {
  const hp = biquad('hp', 7500)
  const start = at(t0)
  const decay = open ? 10 : 55
  for (let i = 0; i < at(open ? 0.3 : 0.08); i++) add(start + i, hp(noise()) * Math.exp((-i / SR) * decay) * 0.3 * gain, pan)
}

function crash(t0, gain = 1) {
  const hp = biquad('hp', 4000)
  const start = at(t0)
  for (let i = 0; i < at(2.5); i++) {
    const t = i / SR
    const v = hp(noise()) * Math.exp(-t * 1.8) * 0.35 * gain
    add(start + i, v, Math.sin(t * 9) * 0.3)
  }
}

function riser(t0, t1) {
  const bp = biquad('bp', 500, 1.2)
  for (let i = at(t0); i < at(t1); i++) {
    const p = (i - at(t0)) / (at(t1) - at(t0))
    bp.set(400 + 7000 * p * p)
    add(i, bp(noise()) * p * p * 0.5)
  }
}

// ---------- tonal ----------
function bassNote(t0, midi, length) {
  const lp = biquad('lp', 900, 1.3)
  const f = hz(midi)
  const start = at(t0)
  for (let i = 0; i < at(length); i++) {
    const t = i / SR
    const phase = 2 * Math.PI * f * t
    const env = Math.min(1, t * 400) * (t < length * 0.85 ? 1 : Math.max(0, 1 - (t - length * 0.85) / (length * 0.15)))
    const v = lp(saw(phase, 10) + Math.sin(phase) * 0.8) * env * 0.55 * duck(t0 + t)
    add(start + i, v)
  }
}

/** Lead: a hollow square-ish tone with delayed vibrato. */
function lead(t0, midi, length, gain = 1) {
  const lp = biquad('lp', 2600, 1.4)
  const f = hz(midi)
  const start = at(t0)
  let phase = 0
  for (let i = 0; i < at(length + 0.05); i++) {
    const t = i / SR
    const vibrato = 1 + Math.min(1, t / 0.25) * 0.006 * Math.sin(2 * Math.PI * 5.5 * t)
    phase += (2 * Math.PI * f * vibrato) / SR
    let sq = 0
    for (let h = 1; h <= 9; h += 2) sq += Math.sin(phase * h) / h
    const env = Math.min(1, t * 150) * (t < length ? 1 : Math.max(0, 1 - (t - length) / 0.05)) * (0.8 + 0.2 * Math.exp(-t * 6))
    add(start + i, lp(sq + saw(phase * 1.003, 4) * 0.4) * env * 0.11 * gain, -0.1)
  }
}

/** Piano: inharmonic partials on two slightly detuned strings; low notes ring longer, high partials die first. */
function piano(t0, midi, length, velocity = 0.8) {
  const f0 = hz(midi)
  const ring = 3.2 * 2 ** ((60 - midi) / 24) // seconds until the fundamental is ~1/e
  const partials = []
  for (let n = 1; n <= 9; n++) {
    const f = n * f0 * Math.sqrt(1 + 0.0004 * n * n)
    if (f > 12000) break
    partials.push({ w: 2 * Math.PI * f, amp: (n === 1 ? 1 : velocity) / n ** 1.4, decay: (1 + n * 0.7) / ring })
  }
  const start = at(t0)
  const pan = Math.max(-0.6, Math.min(0.6, (midi - 62) / 30))
  for (let i = 0; i < at(length + 0.3) && start + i < N; i++) {
    const t = i / SR
    let v = 0
    for (const p of partials) v += p.amp * Math.exp(-t * p.decay) * (Math.sin(p.w * t) + Math.sin(p.w * 1.0007 * t)) * 0.5
    const hammer = t < 0.004 ? noise() * 0.08 * velocity : 0
    const damper = t < length ? 1 : Math.exp(-(t - length) * 22)
    add(start + i, (v * Math.min(1, t * 3000) + hammer) * damper * 0.16 * velocity, pan)
  }
}

/** Schroeder reverb (four combs and two allpasses per channel), mixed in place. */
function reverb(wet = 0.25) {
  for (const [channel, spread] of [[L, 0], [R, 23]]) {
    const dry = Float32Array.from(channel)
    const out = new Float32Array(N)
    for (const ms of [29.7, 37.1, 41.1, 43.7]) {
      const d = Math.round(((ms * SR) / 1000)) + spread
      const buf = new Float32Array(d)
      let low = 0
      for (let i = 0; i < N; i++) {
        const y = buf[i % d]
        low = y * 0.6 + low * 0.4 // damping
        buf[i % d] = dry[i] + low * 0.82
        out[i] += y * 0.25
      }
    }
    for (const ms of [5, 1.7]) {
      const d = Math.round((ms * SR) / 1000)
      const buf = new Float32Array(d)
      for (let i = 0; i < N; i++) {
        const b = buf[i % d]
        const y = -0.7 * out[i] + b
        buf[i % d] = out[i] + 0.7 * y
        out[i] = y
      }
    }
    for (let i = 0; i < N; i++) channel[i] = dry[i] + out[i] * wet
  }
}

/** Future-bass chord stab: detuned saws an octave up, with a pitch scoop at the start and an 8 Hz wobble. */
function stab(t0, chord, length, gain = 1) {
  const lpL = biquad('lp', 3800, 0.9)
  const lpR = biquad('lp', 3800, 0.9)
  const notes = chord.pad.map((midi) => hz(midi + 12))
  const phases = new Float64Array(notes.length * 3)
  const start = at(t0)
  for (let i = 0; i < at(length) && start + i < N; i++) {
    const t = i / SR
    const scoop = 2 ** ((-1.5 * Math.exp(-t * 30)) / 12)
    let l = 0
    let r = 0
    notes.forEach((f, n) => {
      ;[-0.15, 0, 0.15].forEach((detune, k) => {
        const j = n * 3 + k
        phases[j] += (2 * Math.PI * f * scoop * 2 ** (detune / 12)) / SR
        const v = saw(phases[j], 5)
        if (k === 0) l += v
        else if (k === 2) r += v
        else { l += v * 0.5; r += v * 0.5 }
      })
    })
    const env = Math.min(1, t * 300) * (t < length * 0.8 ? 1 : Math.max(0, 1 - (t - length * 0.8) / (length * 0.2)))
    const wobble = 0.7 + 0.3 * Math.sin(2 * Math.PI * 8 * t)
    const g = env * wobble * 0.06 * gain * duck(t0 + t)
    L[start + i] += lpL(l) * g
    R[start + i] += lpR(r) * g
  }
}

/** 808: a saturated sine with a short downward glide. */
function sub808(t0, midi, length) {
  const f = hz(midi)
  const start = at(t0)
  let phase = 0
  for (let i = 0; i < at(length + 0.05); i++) {
    const t = i / SR
    phase += (2 * Math.PI * f * 2 ** ((2 * Math.exp(-t * 25)) / 12)) / SR
    const env = Math.min(1, t * 300) * (t < length ? 1 : Math.max(0, 1 - (t - length) / 0.05)) * (0.6 + 0.4 * Math.exp(-t * 3))
    add(start + i, Math.tanh(Math.sin(phase) * 2.2) * 0.42 * env)
  }
}

/** Vocal chop: a saw through two "ah" formants, scooped up into the note. */
function chop(t0, midi, length, gain = 1, pan = 0) {
  const f1 = biquad('bp', 800, 3)
  const f2 = biquad('bp', 1250, 4)
  const f = hz(midi)
  const start = at(t0)
  let phase = 0
  for (let i = 0; i < at(length); i++) {
    const t = i / SR
    phase += (2 * Math.PI * f * 2 ** ((-2 * Math.exp(-t * 40)) / 12)) / SR
    const s = saw(phase, 12)
    const env = Math.min(1, t * 200) * Math.exp(-t * 3) * (t < length - 0.03 ? 1 : Math.max(0, (length - t) / 0.03))
    add(start + i, (f1(s) + f2(s) * 0.7) * env * 0.9 * gain, pan)
  }
}

function pads() {
  const lpL = biquad('lp', 600, 0.8)
  const lpR = biquad('lp', 600, 0.8)
  const phases = new Float64Array(16)
  for (let i = 0; i < N; i++) {
    const t = i / SR
    // the filter opens over the intro, stays open, and closes on the end card
    const open = t < DROP ? 500 + 1300 * (t / DROP) : t < END ? 2200 : 2200 * Math.exp(-(t - END) * 0.6)
    if (i % 64 === 0) {
      lpL.set(open)
      lpR.set(open * 1.05)
    }
    const chord = t < END ? chordAt(t) : CHORDS[0] // ends on the first chord
    let l = 0
    let r = 0
    chord.pad.forEach((midi, n) => {
      const f = hz(midi)
      for (const [d, side] of [[-0.12, 0], [0.12, 1]]) {
        const k = n * 2 + side
        phases[k] += (2 * Math.PI * f * 2 ** (d / 12)) / SR
        const v = saw(phases[k], 6)
        if (side === 0) l += v
        else r += v
      }
    })
    const fadeIn = Math.min(1, t / 1.2)
    const fadeOut = t > LENGTH - 3.5 ? Math.max(0, (LENGTH - t) / 3.5) : 1
    const g = PAD_GAIN * fadeIn * fadeOut * (t < END ? duck(t) : 1)
    L[i] += lpL(l) * g
    R[i] += lpR(r) * g
  }
}

// ---------- arrangement ----------
const ECHO = (BEAT * 3) / 4
const isPreArp = (t) => t >= ARP - BEAT && t < ARP // one beat of silence before the feature section

function synthwave() {
  // intro: soft ticking hats, then a riser into the drop
  for (let t = BAR; t < DROP; t += BEAT / 2) hat(t, false, 0.6, 0.2)
  riser(DROP - BAR, DROP)

  crash(DROP)
  for (const t of kickTimes) kick(t)
  for (let t = DROP; t < END - 1e-9; t += BEAT) {
    const beat = Math.round((t - DROP) / BEAT)
    const preArp = isPreArp(t)
    if (beat % 2 === 1 && !preArp) clap(t)
    hat(t, false, 0.5, 0.25)
    hat(t + BEAT / 2, false, 0.8, -0.2)
    if (!preArp) {
      const root = chordAt(t).bass
      bassNote(t, root, BEAT / 2)
      bassNote(t + BEAT / 2, root + 12, BEAT / 2)
    }
  }
  // tom-style snare build into the feature section
  for (let i = 0; i < 8; i++) clap(ARP - BEAT * 2 + (i * BEAT) / 4, 0.25 + i * 0.09)
  crash(ARP)

  // lead hook over the feature slides: an 8th-note motif on the chord tones, with a dotted-eighth echo
  const MOTIF = [3, null, 2, 3, null, 1, 2, null, 0, null, 1, 2, 1, null, 0, null] // two bars, indexes into the chord
  for (let t = ARP; t < END - 1e-9; t += BEAT / 2) {
    const step = Math.round((t - ARP) / (BEAT / 2)) % MOTIF.length
    const index = MOTIF[step]
    if (index === null) continue
    const held = MOTIF[(step + 1) % MOTIF.length] === null ? BEAT : BEAT / 2
    const midi = chordAt(t).pad[index] + 12
    lead(t, midi, held * 0.9)
    lead(t + ECHO, midi, held * 0.9, 0.35)
  }

  // end card: one big hit, then the pad rings out
  crash(END, 1.2)
  kick(END, 1.1)
  bassNote(END, 26, 2.5)
}

function futurebass() {
  // intro: filtered vocal chops and a riser
  const INTRO_CHOPS = [[0, 3], [0.75, 2], [1.5, 1], [3, 2], [4, 3], [4.75, 2], [5.5, 3], [6, 0]]
  for (const [beat, index] of INTRO_CHOPS) chop(beat * BEAT, chordAt(beat * BEAT).pad[index] + 12, BEAT * 0.7, 0.55, 0.2)
  riser(DROP - BAR, DROP)

  crash(DROP)
  for (const t of kickTimes) kick(t)
  // stabs on a syncopated 16th pattern
  const STABS = [1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0]
  for (let bar = DROP; bar < END - 1e-9; bar += BAR) {
    const chord = chordAt(bar)
    if (!isPreArp(bar + BEAT * 2)) clap(bar + BEAT * 2, 1.1) // half-time snare on beat 3
    sub808(bar, chord.bass - 12, BEAT * 1.4)
    sub808(bar + BEAT * 1.5, chord.bass - 12, BEAT * 2.3)
    STABS.forEach((hit, step) => {
      const t = bar + (step * BEAT) / 4
      if (hit && !isPreArp(t)) stab(t, chord, BEAT * 0.45)
    })
    // trap hats: 8ths, with a 16th-triplet roll at the end of every second bar
    const roll = Math.round((bar - DROP) / BAR) % 2 === 1
    for (let t = bar; t < bar + BAR - 1e-9; t += BEAT / 2) {
      if (roll && t >= bar + BEAT * 3) {
        for (let k = 0; k < 6; k++) hat(t + (k * BEAT) / 12, false, 0.35 + k * 0.06, 0.3)
        continue
      }
      hat(t, false, 0.7, 0.25)
    }
  }
  for (let i = 0; i < 8; i++) clap(ARP - BEAT * 2 + (i * BEAT) / 4, 0.25 + i * 0.09)
  crash(ARP)

  // vocal-chop hook over the feature slides, with a dotted-eighth echo
  const HOOK = [3, null, 2, null, 3, 2, null, 1, 2, null, null, 1, 0, null, 1, null]
  for (let t = ARP; t < END - 1e-9; t += BEAT / 2) {
    const step = Math.round((t - ARP) / (BEAT / 2)) % HOOK.length
    const index = HOOK[step]
    if (index === null) continue
    const midi = chordAt(t).pad[index] + 12
    chop(t, midi, BEAT * 0.45, 0.8, -0.1)
    chop(t + ECHO, midi, BEAT * 0.45, 0.3, 0.35)
  }

  // end card: one long stab on the first chord, then the pad rings out
  crash(END, 1.2)
  kick(END, 1.1)
  stab(END, CHORDS[0], 2.4, 1.2)
  sub808(END, CHORDS[0].bass - 12, 2)
}

// Prelude in C major BWV 846, bars 1-19: five notes per bar, played as 1 2 3 4 5 3 4 5, twice.
const PRELUDE = [
  [60, 64, 67, 72, 76], [60, 62, 69, 74, 77], [59, 62, 67, 74, 77], [60, 64, 67, 72, 76],
  [60, 64, 69, 76, 81], [60, 62, 66, 69, 74], [59, 62, 67, 74, 79], [59, 60, 64, 67, 72],
  [57, 60, 64, 67, 72], [50, 57, 62, 66, 72], [55, 59, 62, 67, 71], [55, 58, 64, 67, 73],
  [53, 57, 62, 69, 74], [53, 56, 62, 65, 71], [52, 55, 60, 67, 72], [52, 53, 57, 60, 65],
  [50, 53, 57, 60, 65], [43, 50, 55, 59, 65], [48, 52, 55, 60, 64],
]
const FIGURE = [0, 1, 2, 3, 4, 2, 3, 4]

function bach() {
  const step = BEAT / 4
  // bars 1-18 in strict time, one bar per 2 seconds
  PRELUDE.slice(0, 18).forEach((notes, bar) => {
    for (let half = 0; half < 2; half++) {
      const t0 = bar * BAR + half * (BAR / 2)
      FIGURE.forEach((index, k) => {
        const t = t0 + k * step
        // the two low notes are held for the half bar; the upper notes ring under the pedal
        const length = index < 2 ? BAR / 2 - k * step : Math.min(0.9, BAR / 2 - k * step)
        const velocity = (k === 0 ? 0.9 : index < 2 ? 0.75 : 0.6) + (half === 0 ? 0.05 : 0)
        piano(t, notes[index], length, velocity)
      })
    }
  })
  // bar 19 (C major) slows down into the end card
  const last = PRELUDE[18]
  const from = 18 * BAR
  for (let k = 0; k < 16; k++) {
    const t = from + (END - from) * (k / 16) ** 1.3
    piano(t, last[FIGURE[k % 8]], END - t, 0.6 - k * 0.012)
  }
  // end card: a rolled C major chord that rings out
  ;[36, 48, 55, 60, 64, 67, 72].forEach((midi, k) => piano(END + k * 0.03, midi, LENGTH - END - 0.5, 0.85))
  reverb()
}

if (PAD_GAIN > 0) pads()
if (STYLE === 'synthwave') synthwave()
else if (STYLE === 'futurebass') futurebass()
else bach()

// ---------- master ----------
let peak = 0
for (let i = 0; i < N; i++) peak = Math.max(peak, Math.abs(L[i]), Math.abs(R[i]))
const pcm = new Int16Array(N * 2)
for (let i = 0; i < N; i++) {
  pcm[i * 2] = Math.tanh((L[i] / peak) * DRIVE) * 0.89 * 32767
  pcm[i * 2 + 1] = Math.tanh((R[i] / peak) * DRIVE) * 0.89 * 32767
}

const header = Buffer.alloc(44)
header.write('RIFF', 0)
header.writeUInt32LE(36 + pcm.byteLength, 4)
header.write('WAVEfmt ', 8)
header.writeUInt32LE(16, 16)
header.writeUInt16LE(1, 20)
header.writeUInt16LE(2, 22)
header.writeUInt32LE(SR, 24)
header.writeUInt32LE(SR * 4, 28)
header.writeUInt16LE(4, 32)
header.writeUInt16LE(16, 34)
header.write('data', 36)
header.writeUInt32LE(pcm.byteLength, 40)

const out = join(dirname(fileURLToPath(import.meta.url)), '../public/audio/promo-beat.wav')
mkdirSync(dirname(out), { recursive: true })
writeFileSync(out, Buffer.concat([header, Buffer.from(pcm.buffer)]))
console.log('wrote', out)
