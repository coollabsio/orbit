import { expect, test } from 'bun:test'
import { parseAwayMinutes, watchAway, watchIdle } from './idle'

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

test('idle after the time without input, active on the next input, and only changes are reported', async () => {
  const reports: boolean[] = []
  const stop = watchIdle((idle) => reports.push(idle), 60)
  await wait(30)
  // input keeps the tab active and reports nothing
  window.dispatchEvent(new Event('pointermove'))
  await wait(45)
  expect(reports).toEqual([])
  await wait(50)
  expect(reports).toEqual([true])
  window.dispatchEvent(new Event('keydown'))
  window.dispatchEvent(new Event('keydown'))
  expect(reports).toEqual([true, false])
  await wait(90)
  expect(reports).toEqual([true, false, true])
  // an idle tab that stops watching is not left as idle
  stop()
  expect(reports).toEqual([true, false, true, false])
  window.dispatchEvent(new Event('keydown'))
  expect(reports.length).toBe(4)
})

test('away after the window is out of focus for the time, back on focus, and a short look elsewhere reports nothing', async () => {
  const reports: boolean[] = []
  const hasFocus = document.hasFocus
  document.hasFocus = () => true
  const stop = watchAway((away) => reports.push(away), 60)
  // a short look at another window
  window.dispatchEvent(new Event('blur'))
  await wait(30)
  window.dispatchEvent(new Event('focus'))
  await wait(60)
  expect(reports).toEqual([])
  window.dispatchEvent(new Event('blur'))
  window.dispatchEvent(new Event('blur'))
  await wait(90)
  expect(reports).toEqual([true])
  window.dispatchEvent(new Event('focus'))
  expect(reports).toEqual([true, false])
  // an away tab that stops watching is not left as away
  window.dispatchEvent(new Event('blur'))
  await wait(90)
  stop()
  expect(reports).toEqual([true, false, true, false])
  document.hasFocus = hasFocus
})

test('the stored time is one of the choices, or the default', () => {
  expect(parseAwayMinutes('5')).toBe(5)
  for (const raw of [null, '', '0', '4', '-1', 'soon', '600']) expect(parseAwayMinutes(raw)).toBe(1)
})
