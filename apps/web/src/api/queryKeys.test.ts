import { describe, expect, test } from 'bun:test'
import { queryKeys } from './queryKeys'

describe('workspace query keys', () => {
  test('never collide across workspaces', () => {
    expect(queryKeys.tasks.list('workspace-a', { status: 'open' })).not.toEqual(
      queryKeys.tasks.list('workspace-b', { status: 'open' }),
    )
    expect(queryKeys.members('workspace-a')).not.toEqual(queryKeys.members('workspace-b'))
  })

  test('use one workspace prefix for targeted invalidation', () => {
    expect(queryKeys.tasks.detail('workspace-a', 'task-one').slice(0, 2)).toEqual(
      [...queryKeys.workspace('workspace-a')],
    )
  })

  test('cover every persisted workspace task collection under that prefix', () => {
    expect(queryKeys.statuses('workspace-a', 'project-one').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.comments('workspace-a', 'task-one').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.attachments('workspace-a', 'task-one').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.labels('workspace-a').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.taskTrash('workspace-a').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.projectTrash('workspace-a').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.workspaceTrash('workspace-a').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.audit('workspace-a').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
    expect(queryKeys.teamspaces('workspace-a').slice(0, 2)).toEqual(['workspace', 'workspace-a'])
  })

  test('page keys live under the workspace prefix (realtime invalidation) and under pages.all', () => {
    const all = queryKeys.pages.all('workspace-a')
    for (const key of [
      queryKeys.pages.tree('workspace-a'),
      queryKeys.pages.detail('workspace-a', 'page-one'),
      queryKeys.pages.trash('workspace-a'),
      queryKeys.pages.search('workspace-a', 'roadmap'),
      queryKeys.pages.favorites('workspace-a'),
    ]) {
      expect(key.slice(0, 2)).toEqual(['workspace', 'workspace-a'])
      expect(key.slice(0, all.length)).toEqual([...all])
    }
  })
  test('task relations live under the task detail key, so task invalidation refreshes them', () => {
    expect(queryKeys.taskRelations('workspace-a', 'task-one')).toEqual([
      ...queryKeys.tasks.detail('workspace-a', 'task-one'), 'relations',
    ])
    expect(queryKeys.taskRelations('workspace-a', 'task-one').slice(0, 3)).toEqual([...queryKeys.tasks.all('workspace-a')])
  })

  test('Notion import keys stay outside the workspace prefix (polled, not refetched on every realtime event)', () => {
    for (const key of [queryKeys.notionImports.list('workspace-a'), queryKeys.notionImports.detail('workspace-a', 'import-1')]) {
      expect(key.slice(0, 2)).not.toEqual([...queryKeys.workspace('workspace-a')])
      expect(key.slice(0, 2)).toEqual([...queryKeys.notionImports.all('workspace-a')])
    }
    expect(queryKeys.notionImports.all('workspace-a')).not.toEqual(queryKeys.notionImports.all('workspace-b'))
  })
})
