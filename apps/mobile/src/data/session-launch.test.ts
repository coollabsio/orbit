import { ApiProblem } from '@orbit/api/problem'
import { expect, test } from 'bun:test'

import { launchErrorEndsSession } from './session-launch'

const problem = (status: number) => new ApiProblem({ type: 'about:blank', title: 'Error', status, detail: 'x', instance: '/', request_id: 'r1' } as never)

test('only a 401 at launch ends the saved session', () => {
  expect(launchErrorEndsSession(problem(401))).toBe(true)
  for (const error of [problem(500), problem(503), problem(403), new TypeError('Network request failed'), new Error('timeout')]) {
    expect(launchErrorEndsSession(error)).toBe(false)
  }
})
