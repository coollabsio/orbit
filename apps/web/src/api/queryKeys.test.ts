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
})
