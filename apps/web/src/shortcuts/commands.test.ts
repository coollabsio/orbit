import { expect, test } from 'bun:test'
import { COMMANDS, commandsFor } from './commands'
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

test('the chat commands exist only when chat is enabled', () => {
  const ids = (chat: boolean) => commandsFor(chat).map((command) => command.id)
  expect(ids(true)).toContain('nav.chat')
  expect(ids(false)).not.toContain('nav.chat')
  expect(commandsFor(false).filter((command) => command.group === 'Chat' || command.context === 'chat')).toHaveLength(0)
  expect(commandsFor(true).filter((command) => command.id !== 'nav.chat' && command.group !== 'Chat')).toEqual([...commandsFor(false)])
})

test('the chat commands run while the composer has the focus', () => {
  const chat = commandsFor(true).filter((command) => command.context === 'chat')
  expect(chat.map((command) => command.id)).toEqual(['chat.previous', 'chat.next', 'chat.previousUnread', 'chat.nextUnread', 'chat.unreads', 'chat.markAllRead'])
  expect(chat.every((command) => command.inInputs === true && command.group === 'Chat')).toBe(true)
})
