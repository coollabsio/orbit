import { describe, expect, test } from 'bun:test'
import { ApiProblem } from '@/api/problem'
import { taskUnavailableDescription } from './taskAvailability'

describe('task unavailable message', () => {
  test('explains that a linked task may have been deleted or become inaccessible', () => {
    const error = new ApiProblem({
      type: 'about:blank',
      title: 'Task resource not found',
      status: 404,
      code: 'task_resource_not_found',
      detail: 'The requested task resource was not found.',
      instance: '/api/v1/workspaces/workspace-1/tasks/task-1',
      request_id: 'request-1',
    })

    expect(taskUnavailableDescription(error)).toBe(
      'This task may have been deleted, or you may no longer have access to it.',
    )
  })

  test('uses a safe task-specific message for other failures', () => {
    expect(taskUnavailableDescription(new Error('network failure'))).toBe(
      'The server could not load this task.',
    )
  })
})
