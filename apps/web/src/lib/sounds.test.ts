import { afterEach, expect, test } from 'bun:test'
import { DEFAULT_SOUND_SETTINGS, getSoundSettings, parseSoundSettings, setSoundSettings } from './sounds'

afterEach(() => window.localStorage.clear())

test('stored sound settings that are missing or broken become the defaults', () => {
  expect(parseSoundSettings(null)).toEqual(DEFAULT_SOUND_SETTINGS)
  expect(parseSoundSettings('{not json')).toEqual(DEFAULT_SOUND_SETTINGS)
  expect(parseSoundSettings('null')).toEqual(DEFAULT_SOUND_SETTINGS)
  expect(parseSoundSettings('"pop"')).toEqual(DEFAULT_SOUND_SETTINGS)
})

test('each stored field is checked on its own', () => {
  expect(parseSoundSettings(JSON.stringify({ enabled: false, set: 'laser', volume: 'loud' }))).toEqual({ enabled: false, set: 'pop', volume: 0.6 })
  expect(parseSoundSettings(JSON.stringify({ enabled: 'yes', set: 'knock', volume: 7 }))).toEqual({ enabled: true, set: 'knock', volume: 1 })
  expect(parseSoundSettings(JSON.stringify({ set: 'chime', volume: 0 }))).toEqual({ enabled: true, set: 'chime', volume: 0.05 })
})

test('a saved change is read back, and the snapshot is the same object until the next change', () => {
  expect(getSoundSettings()).toEqual(DEFAULT_SOUND_SETTINGS)
  setSoundSettings({ set: 'chime', volume: 0.3 })
  const saved = getSoundSettings()
  expect(saved).toEqual({ enabled: true, set: 'chime', volume: 0.3 })
  expect(getSoundSettings()).toBe(saved)
  window.localStorage.setItem('orbit:sounds', '{broken')
  expect(getSoundSettings()).toEqual(DEFAULT_SOUND_SETTINGS)
})
