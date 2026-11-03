import { expect, test } from 'bun:test'

import { chatActions, LiveCursor, parseLiveFrame, parseWorkspaceEvent, socketUrl, workspaceEventKeys } from './live'
import type { MessageRecord } from './orbit'

test('socket urls swap the scheme and encode the query', () => {
  expect(socketUrl('https://orbit.example.com/', '/api/v1/workspaces/w/live', { epoch: 'a b', after: '7', skip: undefined })).toBe(
    'wss://orbit.example.com/api/v1/workspaces/w/live?epoch=a%20b&after=7',
  )
  expect(socketUrl('http://127.0.0.1:18080', '/api/v1/workspaces/w/events')).toBe('ws://127.0.0.1:18080/api/v1/workspaces/w/events')
})

test('a workspace event refreshes the workspace data; a resync also the workspace list', () => {
  const changed = parseWorkspaceEvent('{"version":1,"kind":"workspace.changed","sequence":"12"}')
  expect(changed).toEqual({ version: 1, kind: 'workspace.changed', sequence: '12' })
  expect(workspaceEventKeys(changed!, 'w')).toEqual([
    ['tasks', 'w'],
    ['task', 'w'],
    ['comments', 'w'],
    ['task-activity', 'w'],
    ['projects', 'w'],
    ['statuses', 'w'],
    ['labels', 'w'],
    ['members', 'w'],
    ['notifications', 'w'],
    ['pages', 'w'],
    ['page', 'w'],
    ['teamspaces', 'w'],
  ])
  const resync = parseWorkspaceEvent('{"version":1,"kind":"resync_required","sequence":"0"}')
  expect(workspaceEventKeys(resync!, 'w').at(-1)).toEqual(['workspaces'])
  expect(parseWorkspaceEvent('{"version":2,"kind":"workspace.changed","sequence":"1"}')).toBe(null)
  expect(parseWorkspaceEvent('{"version":1,"kind":"workspace.changed","sequence":"01"}')).toBe(null)
})

test('chat events patch messages and states, and refetch conversations', () => {
  const message = { id: 'm1', conversation_id: 'c1' } as MessageRecord
  expect(chatActions({ type: 'message.created', message }, 'w')).toEqual([
    { kind: 'upsert-message', conversationId: 'c1', message },
    { kind: 'invalidate', keys: [['conversations', 'w']] },
  ])
  expect(chatActions({ type: 'message.deleted', conversation_id: 'c1', message_id: 'm1', thread_root_id: null }, 'w')).toEqual([
    { kind: 'remove-message', conversationId: 'c1', messageId: 'm1' },
  ])
  expect(chatActions({ type: 'categories.changed', categories: [] }, 'w')).toEqual([{ kind: 'invalidate', keys: [['chat-categories', 'w']] }])
})

test('the live cursor resumes after the last event of the same epoch', () => {
  const cursor = new LiveCursor()
  expect(cursor.query()).toEqual({})
  for (const raw of ['{"type":"hello","epoch":"e1","seq":40}', '{"type":"resync"}', '{"seq":41,"topic":"chat","event":{}}', '{"type":"ping"}']) {
    const frame = parseLiveFrame(raw)
    if (frame) cursor.read(frame)
  }
  expect(cursor.query()).toEqual({ epoch: 'e1', after: '41' })
  // a restored database starts a new epoch: the old number means nothing there
  cursor.read({ type: 'hello', epoch: 'e2', seq: 3 })
  expect(cursor.query()).toEqual({})
})
