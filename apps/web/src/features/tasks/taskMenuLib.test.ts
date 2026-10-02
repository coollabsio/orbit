import { expect, test } from 'bun:test'
import { dueDatePresets, menuTargetIds } from './taskMenuLib'

test('a row in the selection targets the whole selection', () => {
  expect(menuTargetIds('b', ['a', 'b'])).toEqual(['a', 'b'])
})

test('a row outside the selection targets only itself', () => {
  expect(menuTargetIds('c', ['a', 'b'])).toEqual(['c'])
  expect(menuTargetIds('c', [])).toEqual(['c'])
})

const days = (now: Date) => dueDatePresets(now).map((preset) => [preset.label, preset.date.getDate()])

test('due presets end the week on Sunday', () => {
  // Wednesday 7 October 2026
  expect(days(new Date(2026, 9, 7, 15))).toEqual([['Today', 7], ['Tomorrow', 8], ['End of this week', 11], ['In one week', 14]])
})

test('due presets show a day once', () => {
  // Saturday: tomorrow is the end of the week. Sunday: today is.
  expect(days(new Date(2026, 9, 10, 15))).toEqual([['Today', 10], ['Tomorrow', 11], ['In one week', 17]])
  expect(days(new Date(2026, 9, 11, 15))).toEqual([['Today', 11], ['Tomorrow', 12], ['In one week', 18]])
})

test('a due preset is the end of the local day', () => {
  const [today] = dueDatePresets(new Date(2026, 9, 7, 15))
  expect([today.date.getHours(), today.date.getMinutes()]).toEqual([23, 59])
})
