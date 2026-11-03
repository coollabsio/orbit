import { describe, expect, test } from 'bun:test'
import { ApiProblem, fieldErrors, parseProblem } from './problem'

describe('Problem Details', () => {
  test('maps nested field paths without losing messages', () => {
    const problem = new ApiProblem({
      type: 'https://docs.orbit.dev/problems/validation-failed',
      title: 'Validation failed',
      status: 422,
      code: 'validation_failed',
      detail: 'One or more fields are invalid.',
      instance: '/api/v1/workspaces/one/tasks',
      request_id: 'request-one',
      errors: {
        'task.title': ['Title is required.'],
        'assignees[0].id': ['User does not belong to this workspace.'],
      },
    })

    expect(fieldErrors(problem)).toEqual([
      { path: ['assignees', '0', 'id'], message: 'User does not belong to this workspace.' },
      { path: ['task', 'title'], message: 'Title is required.' },
    ])
  })

  test('falls back safely when a response is not Problem Details', async () => {
    const response = new Response('<html>bad gateway</html>', {
      status: 502,
      headers: { 'content-type': 'text/html' },
    })

    const problem = await parseProblem(response)

    expect(problem.status).toBe(502)
    expect(problem.code).toBe('http_error')
    expect(problem.detail).not.toContain('bad gateway')
  })
})
