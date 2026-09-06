import { expect, test } from 'bun:test'
import { dropdownPosition } from './dropdownPosition'

const viewport = { top: 0, bottom: 440, left: 0, width: 390 }
const trigger = { top: 300, bottom: 332, left: 115, right: 180 }

test('flips above a low trigger and clamps right-aligned menus to the screen', () => {
  expect(dropdownPosition(trigger, { width: 180, height: 141 }, viewport, 'right', 'down')).toEqual({ top: 155, left: 8, maxHeight: 288 })
})
test('respects upward preference but flips down when there is no room above', () => {
  expect(dropdownPosition(trigger, { width: 180, height: 80 }, viewport, 'left', 'up').top).toBe(216)
  expect(dropdownPosition({ top: 10, bottom: 42, left: 350, right: 380 }, { width: 180, height: 80 }, viewport, 'left', 'up')).toEqual({ top: 46, left: 202, maxHeight: 386 })
})
test('limits tall menus to the available space above the mobile dock', () => {
  const position = dropdownPosition({ top: 70, bottom: 102, left: 10, right: 90 }, { width: 180, height: 800 }, { ...viewport, bottom: 360 }, 'left', 'down')
  expect(position).toEqual({ top: 106, left: 10, maxHeight: 246 })
})
