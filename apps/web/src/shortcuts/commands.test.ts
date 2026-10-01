import { expect, test } from 'bun:test'
import { COMMANDS } from './commands'
import { findConflict, parseKeys, resolveBindings } from './bindings'

test('command ids are unique', () => {
  const ids = COMMANDS.map((command) => command.id)
  expect(new Set(ids).size).toBe(ids.length)
})

test('every default binding parses', () => {
  for (const command of COMMANDS) if (command.keys) expect(parseKeys(command.keys)).not.toBeNull()
})

test('no two default bindings conflict in overlapping contexts', () => {
  const bindings = resolveBindings({})
  for (const command of COMMANDS) {
    if (command.keys) expect([command.id, findConflict(bindings, command.id, command.keys)?.id ?? null]).toEqual([command.id, null])
  }
})
