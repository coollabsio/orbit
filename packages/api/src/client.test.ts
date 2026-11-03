import { expect, test } from 'bun:test'
import { createOrbitClient } from './client'
import { me, setupStatus } from './generated/sdk.gen'

function recordingFetch(response: () => Response) {
  const requests: Request[] = []
  const fetch = async (request: Request) => {
    requests.push(request)
    return response()
  }
  return { requests, fetch }
}

test('sends the bearer token and contract header on every request', async () => {
  let token = 'token-one'
  const { requests, fetch } = recordingFetch(() => Response.json({ complete: true }))
  const client = createOrbitClient({ baseUrl: 'https://orbit.example.test', fetch, token: () => token })

  await setupStatus({ client })
  token = 'token-two'
  await setupStatus({ client })

  expect(requests.map((request) => request.url)).toEqual([
    'https://orbit.example.test/api/v1/setup/status',
    'https://orbit.example.test/api/v1/setup/status',
  ])
  expect(requests.map((request) => request.headers.get('authorization'))).toEqual([
    'Bearer token-one',
    'Bearer token-two',
  ])
  expect(requests.map((request) => request.headers.get('x-orbit-contract'))).toEqual(['orbit-api-v1', 'orbit-api-v1'])
})

test('omits the Authorization header when there is no token', async () => {
  const { requests, fetch } = recordingFetch(() => Response.json({ complete: true }))
  const client = createOrbitClient({ baseUrl: 'https://orbit.example.test', fetch, token: () => null })

  await setupStatus({ client })

  expect(requests[0]?.headers.get('authorization')).toBeNull()
})

test('reports an expired session once and rejects with the problem', async () => {
  let unauthorized = 0
  const { fetch } = recordingFetch(() =>
    Response.json(
      {
        type: 'https://docs.orbit.dev/problems/authentication-required',
        title: 'Authentication required',
        status: 401,
        code: 'authentication_required',
        detail: 'A valid session is required.',
        instance: '/api/v1/auth/me',
        request_id: 'request-one',
      },
      { status: 401, headers: { 'content-type': 'application/problem+json' } },
    ),
  )
  const client = createOrbitClient({
    baseUrl: 'https://orbit.example.test',
    fetch,
    token: () => 'stale-token',
    onUnauthorized: () => unauthorized++,
  })

  await expect(me({ client, throwOnError: true })).rejects.toMatchObject({
    status: 401,
    code: 'authentication_required',
    detail: 'A valid session is required.',
  })
  expect(unauthorized).toBe(1)
})
