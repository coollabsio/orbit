import { expect, test } from 'bun:test'
import { ApiProblem } from '../../../api/problem'
import { isTaskVersionConflict } from './conflicts'

test('only stale task versions request an explicit refresh', () => {
  const stale = new ApiProblem({
    type: 'https://docs.orbit.test/task-conflict', title: 'Conflict', status: 409, code: 'task_conflict',
    detail: 'The task changed.', instance: '/tasks/task-1', request_id: 'request-1',
  })
  const unavailable = new ApiProblem({
    type: 'about:blank', title: 'Unavailable', status: 503, code: 'http_error',
    detail: 'Unavailable.', instance: '/tasks/task-1', request_id: 'request-2',
  })
  expect(isTaskVersionConflict(stale)).toBeTrue()
  expect(isTaskVersionConflict(unavailable)).toBeFalse()
})
