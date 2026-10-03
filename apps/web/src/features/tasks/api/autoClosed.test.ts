import { expect, spyOn, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { TaskRecord } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { announceAutoClosed, autoClosedMessage, autoClosedOf } from './autoClosed'
import { keepIdentifiersTogether } from '@/lib/toast'

const done = (id: string) => ({ id, status_id: 'done' })
const identifierOf = (id: string) => `ORB-${id}`
const record = (id: string, parentId: string | null = null) => ({
  id, workspace_id: 'ws', project_id: 'project-1', number: Number(id.slice(-4)), status_id: 'todo', title: id, description: '', position: 0, priority: 'none',
  assignee_ids: [], creator_id: 'user-1', label_ids: [], created_at: '', updated_at: '', version: 1, duplicate_of: null, blocked: false,
  parent_task_id: parentId, parent: null, sub_issue_count: 0, sub_issue_closed_count: 0,
}) as TaskRecord

test('auto-closed items come from the response; anything else is none', () => {
  expect(autoClosedOf({ auto_closed: [done('a'), { id: 1 }] })).toEqual([done('a')])
  expect(autoClosedOf({ items: [] })).toEqual([])
  expect(autoClosedOf(null)).toEqual([])
})

test('messages name one closed parent, count closed sub-issues, or both', () => {
  expect(autoClosedMessage({ items: [done('12')], parentIds: new Set(['12']), identifierOf })).toBe('Closed parent ORB-12')
  expect(autoClosedMessage({ items: [done('1'), done('2'), done('3')], parentIds: new Set(), identifierOf })).toBe('Closed 3 sub-issues')
  expect(autoClosedMessage({ items: [done('1')], parentIds: new Set(), identifierOf })).toBe('Closed 1 sub-issue')
  expect(autoClosedMessage({ items: [done('1'), done('2'), done('12')], parentIds: new Set(['12']), identifierOf })).toBe('Closed 2 sub-issues and parent ORB-12')
  expect(autoClosedMessage({ items: [done('12'), done('9')], parentIds: new Set(['12', '9']), identifierOf })).toBe('Closed 2 parent tasks')
  expect(autoClosedMessage({ items: [], parentIds: new Set(), identifierOf })).toBeNull()
})

test('the toast finds the parent chain and project key in the cache', () => {
  const client = new QueryClient()
  client.setQueryData(queryKeys.projects('ws'), [{ id: 'project-1', key: 'ORB' }])
  client.setQueryData(queryKeys.tasks.list('ws', {}), { pages: [{ items: [record('task-0012', 'task-0009'), record('task-0009')], next_cursor: null }], pageParams: [undefined] })
  const success = spyOn(toast, 'success').mockImplementation(() => 0)
  announceAutoClosed(client, 'ws', [record('task-0031', 'task-0012')], { auto_closed: [done('task-0012')] })
  expect(success.mock.calls[0]![0]).toBe(keepIdentifiersTogether('Closed parent ORB-12'))
  announceAutoClosed(client, 'ws', [record('task-0031', 'task-0012')], { auto_closed: [done('task-0012'), done('task-0009')] })
  expect(success.mock.calls[1]![0]).toBe('Closed 2 parent tasks')
  announceAutoClosed(client, 'ws', [record('task-0031', 'task-0012')], { auto_closed: [] })
  expect(success).toHaveBeenCalledTimes(2)
  success.mockRestore()
})
