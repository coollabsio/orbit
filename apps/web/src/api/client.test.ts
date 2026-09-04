import { expect, test } from 'bun:test'
import { CONTRACT_ID, createApiClient } from './client'
import { setupStatus, me } from './generated/sdk.gen'

test('API client sends cookies and the contract identifier', async () => {
  let captured: Request | undefined
  const client = createApiClient({
    fetch: async (request) => {
      captured = request
      return Response.json({ complete: true })
    },
  })

  await setupStatus({ client })

  expect(captured?.credentials).toBe('include')
  expect(captured?.headers.get('x-orbit-contract')).toBe(CONTRACT_ID)
})

test('API client reports unauthorized responses at one boundary', async () => {
  let unauthorized = 0
  const client = createApiClient({
    fetch: async () =>
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
    onUnauthorized: () => unauthorized++,
  })

  await expect(me({ client, throwOnError: true })).rejects.toMatchObject({ status: 401 })
  expect(unauthorized).toBe(1)
})
