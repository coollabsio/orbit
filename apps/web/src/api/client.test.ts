import { expect, test } from 'bun:test'
import { CONTRACT_ID, createApiClient } from './client'
import { createTask, login, me, setupComplete, setupStatus } from '@orbit/api/generated/sdk.gen'

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

test('API client reports writes refused by Orbit Cloud billing with the server detail', async () => {
  const refusals = [
    { code: 'workspace_read_only', detail: 'This workspace is read-only because it has no active subscription.' },
    { code: 'trial_limit_reached', detail: 'The trial allows 50 tasks. Choose a plan to add more.' },
    { code: 'payment_required_elsewhere', detail: 'Not a billing refusal.' },
  ]
  const refused: string[] = []
  for (const { code, detail } of refusals) {
    const client = createApiClient({
      fetch: async () =>
        Response.json(
          {
            type: `https://docs.orbit.dev/problems/${code}`,
            title: 'Refused',
            status: 402,
            code,
            detail,
            instance: '/api/v1/workspaces/w1/tasks',
            request_id: 'request-two',
          },
          { status: 402, headers: { 'content-type': 'application/problem+json' } },
        ),
      onPaymentRequired: (detail) => refused.push(detail),
    })

    await expect(
      createTask({ client, path: { workspace_id: 'w1' }, body: { project_id: 'p1', status_id: 's1', title: 'New' }, throwOnError: true }),
    ).rejects.toMatchObject({ status: 402, code })
  }
  expect(refused).toEqual(['This workspace is read-only because it has no active subscription.', 'The trial allows 50 tasks. Choose a plan to add more.'])
})
