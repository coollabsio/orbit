import { expect, test } from 'bun:test'
import { dueDateInputValue, dueDatePatchValue } from './dueDate'

test('due date values round-trip between API timestamps and local inputs', () => {
  const input = dueDateInputValue('2030-01-02T12:30:00.000Z')
  expect(input).toMatch(/^2030-01-02T\d{2}:30$/)
  expect(dueDatePatchValue(input)).toMatch(/^2030-01-02T\d{2}:30:00\.000Z$/)
  expect(dueDatePatchValue('')).toBeNull()
})
