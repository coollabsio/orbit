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
  })
})
