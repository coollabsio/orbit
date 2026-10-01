import { expect, test } from 'bun:test'
import { applyRebind, contextsOverlap, findConflict, findConflicts, fromRecorded, isReserved, parseKeys, resolveBindings, toEngineStep } from './bindings'

test('parseKeys splits a sequence and rejects bad input', () => {
  expect(parseKeys('G I')).toEqual(['G', 'I'])
  expect(parseKeys('Mod+Shift+K')).toEqual(['Mod+Shift+K'])
  expect(parseKeys('G I X')).toBeNull()
  expect(parseKeys('not a key ++')).toBeNull()
  expect(parseKeys('Shift')).toBeNull()
  expect(parseKeys('')).toBeNull()
})

test('toEngineStep uses physical codes for letters, digits and punctuation', () => {
  expect(toEngineStep('Mod+Shift+K')).toBe('Mod+Shift+[KeyK]')
  expect(toEngineStep('S')).toBe('[KeyS]')
  expect(toEngineStep('3')).toBe('[Digit3]')
  expect(toEngineStep('Mod+,')).toBe('Mod+[Comma]')
  expect(toEngineStep('?')).toBe('Shift+[Slash]')
  expect(toEngineStep('Escape')).toBe('Escape')
})

test('fromRecorded turns recorder output into registry notation', () => {
  expect(fromRecorded(['Mod+[KeyS]'])).toBe('Mod+S')
  expect(fromRecorded(['[KeyG]', '[KeyI]'])).toBe('G I')
  expect(fromRecorded(['Mod+Shift+[Comma]'])).toBe('Mod+Shift+,')
  expect(fromRecorded(['[KeyG]', '[KeyI]', '[KeyX]'])).toBe('G I')
  expect(fromRecorded(['Shift+[Slash]'])).toBe('?')
})

test('resolveBindings applies valid overrides only', () => {
  const bindings = resolveBindings({ 'task.setStatus': 'Q', 'nav.inbox': null, nope: 'X', 'task.setPriority': 'not a key ++' })
  expect(bindings['task.setStatus']).toBe('Q')
  expect(bindings['nav.inbox']).toBeNull()
  expect('nope' in bindings).toBe(false)
  expect(bindings['task.setPriority']).toBe('P')
})

test('a fixed command ignores its override', () => {
  expect(resolveBindings({ 'detail.close': 'Q' })['detail.close']).toBe('Escape')
})

test('findConflict sees equal keys and sequence prefixes in overlapping contexts', () => {
  const bindings = resolveBindings({})
  expect(findConflict(bindings, 'task.setStatus', 'P')?.id).toBe('task.setPriority')
  expect(findConflict(bindings, 'task.setStatus', 'G')?.id).toMatch(/^nav\./)
  expect(findConflict(bindings, 'task.setStatus', 'G I')?.id).toBe('nav.inbox')
  expect(findConflict(bindings, 'task.setStatus', 'S')).toBeNull()
  expect(findConflict(bindings, 'docs.history', 'T')).toBeNull()
  expect(findConflict(bindings, 'task.setStatus', 'Shift+/')?.id).toBe('help.open')
})

test('contextsOverlap', () => {
  expect(contextsOverlap('global', 'docs')).toBe(true)
  expect(contextsOverlap('task-target', 'task-detail')).toBe(true)
  expect(contextsOverlap('task-list', 'timeline')).toBe(true)
  expect(contextsOverlap('task-detail', 'task-list')).toBe(false)
  expect(contextsOverlap('docs', 'timeline')).toBe(false)
  expect(contextsOverlap('new-task', 'task-detail')).toBe(false)
})

test('applyRebind swaps, clears and drops overrides equal to the default', () => {
  expect(applyRebind({}, 'task.setStatus', 'P', true)).toEqual({ 'task.setStatus': 'P', 'task.setPriority': 'S' })
  expect(applyRebind({ 'task.setStatus': 'Q' }, 'task.setStatus', 'S', false)).toEqual({})
  expect(applyRebind({}, 'task.setStatus', null, false)).toEqual({ 'task.setStatus': null })
  expect(applyRebind({}, 'task.setStatus', 'Q', false)).toEqual({ 'task.setStatus': 'Q' })
})

test('isReserved refuses browser shortcuts and bare modifiers', () => {
  expect(isReserved('Mod+W')).toBe(true)
  expect(isReserved('Shift')).toBe(true)
  expect(isReserved('Mod+Shift')).toBe(true)
  expect(isReserved('Mod+K')).toBe(false)
})

test('findConflicts lists every command a key collides with', () => {
  const bindings = resolveBindings({})
  expect(findConflicts(bindings, 'task.setStatus', 'G').length).toBe(10)
  expect(findConflicts(bindings, 'task.setStatus', 'P').map((command) => command.id)).toEqual(['task.setPriority'])
  expect(findConflicts(bindings, 'task.setStatus', 'Q')).toEqual([])
})
