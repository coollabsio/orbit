import { expect, test } from 'bun:test'
import { localTimeLabel, noteToSave, phoneHref, phoneValid, profileChanges } from './profileLib'

const noon = Date.UTC(2026, 0, 15, 12, 5)

test('the local time is that of the zone, not of this device', () => {
  expect(localTimeLabel('Asia/Kolkata', noon, 'en-US')).toBe('5:35 PM local time')
  expect(localTimeLabel('America/New_York', noon, 'en-US')).toBe('7:05 AM local time')
})

test('no local time without a zone or with one the browser does not know', () => {
  expect(localTimeLabel(null, noon)).toBeNull()
  expect(localTimeLabel(undefined, noon)).toBeNull()
  expect(localTimeLabel('', noon)).toBeNull()
  expect(localTimeLabel('Mars/Olympus_Mons', noon)).toBeNull()
})

test('a note goes out only when it differs from the saved one; empty text removes it', () => {
  expect(noteToSave('met at the offsite', 'met at the offsite')).toBeNull()
  expect(noteToSave('met at the offsite', '  met at the offsite \n')).toBeNull()
  expect(noteToSave(null, '   ')).toBeNull()
  expect(noteToSave(undefined, 'new')).toBe('new')
  expect(noteToSave('old', ' new ')).toBe('new')
  expect(noteToSave('old', '  ')).toBe('')
})

const saved = { display_name: 'Ada', title: 'Engineer', pronouns: null, phone: null, timezone: 'Europe/London', bio: 'Hello' }

test('a profile save has the name and only the parts that changed; an emptied part goes as empty text', () => {
  const form = { name: 'Ada', title: 'Engineer', pronouns: '', phone: '', timezone: 'Europe/London', bio: 'Hello' }
  expect(profileChanges(saved, form)).toBeNull()
  expect(profileChanges(saved, { ...form, title: ' Engineer ' })).toBeNull()
  expect(profileChanges(saved, { ...form, pronouns: 'she/her' })).toEqual({ display_name: 'Ada', pronouns: 'she/her' })
  expect(profileChanges(saved, { ...form, name: 'Ada L', bio: '', timezone: '' })).toEqual({ display_name: 'Ada L', bio: '', timezone: '' })
  expect(profileChanges(saved, { ...form, name: '  ', title: 'CTO' })).toBeNull()
})

test('a phone number has a digit and only digits, spaces and + - ( ) .', () => {
  for (const phone of ['+36 (30) 123-4567', '555 0100', '020.7946.0958']) expect(phoneValid(phone)).toBe(true)
  for (const phone of ['call me', '+-()', '555 0100 ext 2', '']) expect(phoneValid(phone)).toBe(false)
  expect(phoneHref(' +36 (30) 123-4567')).toBe('tel:+36301234567')
  expect(phoneHref('020.7946.0958')).toBe('tel:02079460958')
})
