import { useSyncExternalStore } from 'react'

export type SoundKind = 'message' | 'mention'
export type SoundSet = 'pop' | 'knock' | 'chime'

/** Belongs to the device (its speakers, its surroundings), so it lives in `localStorage`, not on the account. */
export interface SoundSettings {
  enabled: boolean
  set: SoundSet
  /** 0.05..1 */
  volume: number
}

/** `[frequency, startSeconds, durationSeconds, endFrequency?]`: an end frequency glides the pitch over the note. */
type Note = readonly [frequency: number, start: number, duration: number, endFrequency?: number]

interface Sound {
  wave: OscillatorType
  notes: readonly Note[]
}

export const SOUND_SETS: { value: SoundSet; label: string }[] = [
  { value: 'pop', label: 'Pop' },
  { value: 'knock', label: 'Knock' },
  { value: 'chime', label: 'Chime' },
]

const SOUNDS: Record<SoundSet, Record<SoundKind, Sound>> = {
  // Soft and low: a short upward blip into a held note. The mention adds a third, higher note.
  pop: {
    message: { wave: 'sine', notes: [[520, 0, 0.09, 660], [784, 0.09, 0.16]] },
    mention: { wave: 'sine', notes: [[587, 0, 0.08, 740], [880, 0.08, 0.1], [1175, 0.17, 0.22]] },
  },
  // Dry wood taps: short triangle notes that fall in pitch. The mention is four rising taps.
  knock: {
    message: { wave: 'triangle', notes: [[196, 0, 0.07, 140], [196, 0.11, 0.07, 140], [262, 0.22, 0.1, 180]] },
    mention: { wave: 'triangle', notes: [[262, 0, 0.06, 180], [262, 0.09, 0.06, 180], [330, 0.18, 0.06, 220], [392, 0.27, 0.12, 260]] },
  },
  // Two bell notes a fifth apart that ring out; the mention adds the octave.
  chime: {
    message: { wave: 'sine', notes: [[659, 0, 0.35], [988, 0.12, 0.6]] },
    mention: { wave: 'sine', notes: [[659, 0, 0.25], [988, 0.1, 0.3], [1319, 0.2, 0.7]] },
  },
}

/** Evens out how loud the wave shapes sound at the same gain. */
const WAVE_GAIN: Partial<Record<OscillatorType, number>> = { sine: 1, triangle: 0.9, square: 0.25, sawtooth: 0.3 }

export const DEFAULT_SOUND_SETTINGS: SoundSettings = { enabled: true, set: 'pop', volume: 0.6 }
export const MIN_VOLUME = 0.05
const STORAGE_KEY = 'orbit:sounds'

/** The stored settings; anything missing, of the wrong type or out of range becomes the default. */
export function parseSoundSettings(raw: string | null): SoundSettings {
  if (!raw) return DEFAULT_SOUND_SETTINGS
  let stored: unknown
  try {
    stored = JSON.parse(raw)
  } catch {
    return DEFAULT_SOUND_SETTINGS
  }
  if (typeof stored !== 'object' || stored === null) return DEFAULT_SOUND_SETTINGS
  const { enabled, set, volume } = stored as Record<string, unknown>
  return {
    enabled: typeof enabled === 'boolean' ? enabled : DEFAULT_SOUND_SETTINGS.enabled,
    set: SOUND_SETS.some((option) => option.value === set) ? (set as SoundSet) : DEFAULT_SOUND_SETTINGS.set,
    volume: typeof volume === 'number' && Number.isFinite(volume) ? Math.min(1, Math.max(MIN_VOLUME, volume)) : DEFAULT_SOUND_SETTINGS.volume,
  }
}

const listeners = new Set<() => void>()
/** `useSyncExternalStore` needs the same object while the stored text is the same. */
let cache: { raw: string | null; settings: SoundSettings } | null = null

function readRaw(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

export function getSoundSettings(): SoundSettings {
  const raw = readRaw()
  if (!cache || cache.raw !== raw) cache = { raw, settings: parseSoundSettings(raw) }
  return cache.settings
}

export function setSoundSettings(patch: Partial<SoundSettings>) {
  const next = parseSoundSettings(JSON.stringify({ ...getSoundSettings(), ...patch }))
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  } catch {
    // Storage is full or blocked: the setting lasts for this page only.
    cache = { raw: readRaw(), settings: next }
  }
  for (const listener of [...listeners]) listener()
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  // Another tab changed the settings.
  const onStorage = (event: StorageEvent) => {
    if (event.key === STORAGE_KEY || event.key === null) listener()
  }
  window.addEventListener('storage', onStorage)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

export function useSoundSettings(): [SoundSettings, (patch: Partial<SoundSettings>) => void] {
  return [useSyncExternalStore(subscribe, getSoundSettings, () => DEFAULT_SOUND_SETTINGS), setSoundSettings]
}

let context: AudioContext | null = null

/**
 * Plays a notification sound, synthesised with Web Audio (no audio files). Browsers allow audio only after the user
 * has clicked or typed on the page; before that the sound is dropped without an error.
 */
export function playSound(kind: SoundKind, settings: SoundSettings = getSoundSettings()) {
  if (!settings.enabled) return
  try {
    const AudioContextClass = window.AudioContext ?? (window as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
    if (!AudioContextClass) return
    const ac = (context ??= new AudioContextClass())
    const schedule = () => {
      const { wave, notes } = SOUNDS[settings.set][kind]
      const gain = 0.2 * settings.volume * (WAVE_GAIN[wave] ?? 1)
      const now = ac.currentTime + 0.02
      for (const [frequency, start, duration, endFrequency] of notes) {
        const oscillator = ac.createOscillator()
        const envelope = ac.createGain()
        oscillator.type = wave
        oscillator.frequency.setValueAtTime(frequency, now + start)
        if (endFrequency) oscillator.frequency.exponentialRampToValueAtTime(endFrequency, now + start + duration)
        envelope.gain.setValueAtTime(0, now + start)
        envelope.gain.linearRampToValueAtTime(gain, now + start + 0.01)
        envelope.gain.exponentialRampToValueAtTime(1e-4, now + start + duration)
        oscillator.connect(envelope).connect(ac.destination)
        oscillator.start(now + start)
        oscillator.stop(now + start + duration)
      }
    }
    if (ac.state === 'running') {
      schedule()
      return
    }
    // The browser holds the audio back until the user clicked or typed on the page. Notes put on a held clock would
    // all play at once later, so a sound plays only if the audio starts now.
    void ac
      .resume()
      .then(() => {
        if (ac.state === 'running') schedule()
      })
      .catch(() => {})
  } catch {
    // No audio device, or the browser refused: a missing sound is not an error.
  }
}
