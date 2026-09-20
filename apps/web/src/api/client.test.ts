import { expect, test } from 'bun:test'
import { CONTRACT_ID, createApiClient } from './client'
import { login, me, setupComplete, setupStatus } from '@/api/generated/sdk.gen'

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

test('public credential and setup-token failures do not expire a session', async () => {
  for (const [code, request] of [
    [
      'invalid_credentials',
      (client: ReturnType<typeof createApiClient>) =>
        login({ client, body: { email: 'person@example.test', password: 'wrong password' }, throwOnError: true }),
    ],
    [
      'invalid_setup_token',
      (client: ReturnType<typeof createApiClient>) =>
        setupComplete({
          client,
          body: {
            token: 'expired',
            email: 'owner@example.test',
            display_name: 'Owner',
            password: 'a long enough password',
            workspace_name: 'Orbit',
            project_name: 'Work',
          },
          throwOnError: true,
        }),
    ],
  ] as const) {
    let unauthorized = 0
    const client = createApiClient({
      fetch: async () =>
        Response.json(
          {
            type: `https://docs.orbit.dev/problems/${code.replaceAll('_', '-')}`,
            title: 'Request rejected',
            status: 401,
            code,
            detail: 'The request was rejected.',
            instance: '/public-operation',
            request_id: 'request-one',
          },
          { status: 401, headers: { 'content-type': 'application/problem+json' } },
        ),
      onUnauthorized: () => unauthorized++,
    })

    await expect(request(client)).rejects.toMatchObject({ code })
    expect(unauthorized).toBe(0)
  }
})
