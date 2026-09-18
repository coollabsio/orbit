import { expect, test } from 'bun:test'
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import type { TaskRecord } from '../../../api/generated/types.gen'
import type { User } from '../../../features/tasks/api/models'
import {
  MAX_TASK_SUGGESTIONS,
  MAX_USER_SUGGESTIONS,
  SUGGESTION_DEBOUNCE_MS,
  buildMentionItems,
  mentionNodeFor,
} from './mentionSuggestion'
import { PersonMention } from './personMention'

function user(id: string, name: string, handle: string): User {
  return {
    id, membershipId: `m-${id}`, name, handle, email: `${handle}@orbit.test`,
    role: 'Member', color: '#000', online: false, title: '', roleIds: [], version: 0,
  }
}

function task(id: string, identifier: string, title: string): TaskRecord {
  return {
    id, identifier, title, workspace_id: 'w', project_id: 'p', status_id: 's',
    description_json: { type: 'doc', content: [] }, description_text: '', priority: 'none',
    position: 0, creator_id: 'u', assignee_ids: [], label_ids: [], due_at: null,
    version: 0, deleted_at: null, created_at: '', updated_at: '',
  } as unknown as TaskRecord
}

const members = [user('u1', 'Ada Lovelace', 'ada'), user('u2', 'Adam Smith', 'adam'), user('u3', 'Grace Hopper', 'grace')]

test('people come first, then issues, both matching the query', () => {
  const items = buildMentionItems({
    query: 'ad',
    members,
    tasks: [task('t1', 'ORB-12', 'Adapt the gateway')],
  })

  expect(items.map((item) => item.kind)).toEqual(['user', 'user', 'task'])
  expect(items[0]).toEqual({ kind: 'user', id: 'u1', label: 'Ada Lovelace', handle: 'ada' })
  expect(items[2]).toEqual({ kind: 'task', id: 't1', identifier: 'ORB-12', title: 'Adapt the gateway' })
})

test('people match on name and on handle', () => {
  expect(buildMentionItems({ query: 'grace', members, tasks: [] })).toHaveLength(1)
  expect(buildMentionItems({ query: 'hopper', members, tasks: [] })).toHaveLength(1)
  expect(buildMentionItems({ query: 'nobody', members, tasks: [] })).toHaveLength(0)
})

test('an empty query offers people but not a full issue dump', () => {
  const items = buildMentionItems({ query: '', members, tasks: [] })

  expect(items.every((item) => item.kind === 'user')).toBe(true)
  expect(items).toHaveLength(3)
})

test('caps are 5 people and 8 issues', () => {
  expect(MAX_USER_SUGGESTIONS).toBe(5)
  expect(MAX_TASK_SUGGESTIONS).toBe(8)
  expect(SUGGESTION_DEBOUNCE_MS).toBe(150)

  const manyUsers = Array.from({ length: 12 }, (_, index) => user(`u${index}`, `User ${index}`, `user${index}`))
  const manyTasks = Array.from({ length: 12 }, (_, index) => task(`t${index}`, `ORB-${index}`, `User task ${index}`))
  const items = buildMentionItems({ query: 'user', members: manyUsers, tasks: manyTasks })

  expect(items.filter((item) => item.kind === 'user')).toHaveLength(5)
  expect(items.filter((item) => item.kind === 'task')).toHaveLength(8)
})

test('the server relevance order of issues is preserved, not re-sorted client side', () => {
  const items = buildMentionItems({
    query: 'gate',
    members: [],
    tasks: [task('t2', 'ORB-99', 'zzz gateway'), task('t1', 'ORB-1', 'aaa gateway')],
  })

  expect(items.map((item) => (item.kind === 'task' ? item.identifier : ''))).toEqual(['ORB-99', 'ORB-1'])
})

test('a picked item becomes a node carrying exactly the allowlisted attributes', () => {
  expect(mentionNodeFor({ kind: 'user', id: 'u1', label: 'Ada Lovelace', handle: 'ada' })).toEqual({
    type: 'mention',
    attrs: { id: 'u1', label: 'Ada Lovelace' },
  })
  expect(mentionNodeFor({ kind: 'task', id: 't1', identifier: 'ORB-12', title: 'Ship' })).toEqual({
    type: 'taskMention',
    attrs: { id: 't1', identifier: 'ORB-12' },
  })
})

test('the person mention node never serializes a mentionSuggestionChar attribute', () => {
  const editor = new Editor({
    extensions: [StarterKit, PersonMention],
    content: { type: 'doc', content: [{ type: 'paragraph' }] },
  })
  editor.commands.insertContent(mentionNodeFor({ kind: 'user', id: 'u1', label: 'Ada', handle: 'ada' }))

  const mention = editor.getJSON().content?.[0]?.content?.[0]
  expect(mention).toEqual({ type: 'mention', attrs: { id: 'u1', label: 'Ada' } })
  expect(editor.getText()).toContain('@Ada')
  editor.destroy()
})
