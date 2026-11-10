import { expect, test } from 'bun:test'
import { mixSuggestions, taskSearchText, type MentionSuggestion } from './useMentionAutocomplete'

const person = (id: string): MentionSuggestion => ({ id, label: id, username: '', kind: 'channel' })
const task = (id: string): MentionSuggestion => ({ id, label: id, username: id, kind: 'task' })
const ids = (list: MentionSuggestion[]) => list.map((item) => item.id)

test('taskSearchText searches a short query, the recent tasks for none, nothing for a heading or plain text', () => {
  expect(taskSearchText('login')).toBe('login')
  expect(taskSearchText('fix login bug')).toBe('fix login bug')
  expect(taskSearchText('')).toBe('')
  expect(taskSearchText(' Heading')).toBeNull()
  expect(taskSearchText('can you check this one please')).toBeNull()
})

test('mixSuggestions lists channels first, tasks first for an ID, and shares ten rows', () => {
  const people = Array.from({ length: 10 }, (_, index) => person(`p${index}`))
  const tasks = Array.from({ length: 8 }, (_, index) => task(`t${index}`))
  expect(ids(mixSuggestions(people, [], 'a'))).toHaveLength(10)
  const mixed = mixSuggestions(people, tasks, 'a')
  expect(ids(mixed)).toEqual(['p0', 'p1', 'p2', 'p3', 't0', 't1', 't2', 't3', 't4', 't5'])
  expect(ids(mixSuggestions(people.slice(0, 1), tasks, 'eng-1'))).toEqual(['t0', 't1', 't2', 't3', 't4', 't5', 't6', 't7', 'p0'])
  expect(ids(mixSuggestions(people, tasks.slice(0, 2), 'a'))).toEqual(['p0', 'p1', 'p2', 'p3', 'p4', 'p5', 'p6', 'p7', 't0', 't1'])
})
