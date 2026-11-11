import { expect, test } from 'bun:test'
import { appendChild, canNestGroup, duplicateNode, nodeAt, updateNode, wrapInGroup } from './filterTree'
import { simpleRules } from './meta'
import type { DatabaseFilterGroup } from './types'

const a = { property_id: 'a', operator: 'is_empty' } as const
const b = { property_id: 'b', operator: 'is_empty' } as const
const root: DatabaseFilterGroup = { op: 'and', children: [a, { op: 'or', children: [b] }] }

test('edits address nodes by child path and leave the input alone', () => {
  expect(nodeAt(root, [1, 0])).toEqual(b)
  expect(updateNode(root, [1], (group) => ({ ...group, op: 'and' }) as DatabaseFilterGroup)).toEqual({ op: 'and', children: [a, { op: 'and', children: [b] }] })
  expect(updateNode(root, [0], () => null)).toEqual({ op: 'and', children: [{ op: 'or', children: [b] }] })
  expect(appendChild(root, [1], a)).toEqual({ op: 'and', children: [a, { op: 'or', children: [b, a] }] })
  expect(duplicateNode(root, [0])).toEqual({ op: 'and', children: [a, a, { op: 'or', children: [b] }] })
  expect(wrapInGroup(root, [1, 0])).toEqual({ op: 'and', children: [a, { op: 'or', children: [{ op: 'and', children: [b] }] }] })
  expect(root).toEqual({ op: 'and', children: [a, { op: 'or', children: [b] }] })
})

test('groups nest three levels with the root as level one', () => {
  expect([canNestGroup([]), canNestGroup([1]), canNestGroup([1, 0])]).toEqual([true, true, false])
})

test('a filter shows as chips only when it is an and root of rules', () => {
  expect(simpleRules({ op: 'and', children: [a, b] })).toEqual([a, b])
  expect(simpleRules({ op: 'or', children: [a] })).toEqual([a])
  expect(simpleRules({ op: 'or', children: [a, b] })).toBeNull()
  expect(simpleRules(root)).toBeNull()
})
