import { expect, test } from 'bun:test'
import { parentFilterIds } from './useFilterTaskRefs'

test('parent ids come from every parent condition in the tree, without "none" or repeats', () => {
  expect(parentFilterIds({ op: 'and', children: [
    { field: 'parent', operator: 'is', value: ['a', 'none'] },
    { op: 'or', children: [{ field: 'parent', operator: 'is_not', value: ['b', 'a'] }, { field: 'project', operator: 'is', value: ['p'] }] },
  ] })).toEqual(['a', 'b'])
})
