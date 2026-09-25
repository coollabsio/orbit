import { expect, test } from 'bun:test'
import { parseEvent } from './events'

test('accepts versioned invalidations with exact integer cursors', () => {
  expect(parseEvent('{"version":1,"kind":"workspace.changed","sequence":"9007199254740993"}')).toEqual({version:1,kind:'workspace.changed',sequence:'9007199254740993'})
  for (const value of ['{}', 'broken', '{"version":2,"kind":"workspace.changed","sequence":"1"}', '{"version":1,"kind":"workspace.changed","sequence":"-1"}']) expect(parseEvent(value)).toBeNull()
})

test('views_only is an optional boolean', () => {
  expect(parseEvent('{"version":1,"kind":"workspace.changed","sequence":"1","views_only":true}')?.views_only).toBeTrue()
  expect(parseEvent('{"version":1,"kind":"workspace.changed","sequence":"1","views_only":"yes"}')).toBeNull()
})
