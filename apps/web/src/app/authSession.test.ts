import { expect, test } from 'bun:test'
import { QueryClient } from '@tanstack/react-query'
import { createApiClient } from '@/api/client'
import { listWorkspaces } from '@/api/generated/sdk.gen'
import { clearExpiredSession } from './authSession'

test('a 401 from an unrelated request clears the authenticated user and navigates to login', async () => {
  const queryClient = new QueryClient()
  queryClient.setQueryData(['current-user'], { id: 'user-1' })
  let destination: string | undefined

  const client = createApiClient({
    fetch: async () =>
      Response.json(
        {
          type: 'https://docs.orbit.dev/problems/authentication-required',
          title: 'Authentication required',
          status: 401,
          code: 'authentication_required',
          detail: 'A valid session is required.',
          instance: '/api/v1/workspaces',
          request_id: 'request-one',
        },
        { status: 401, headers: { 'content-type': 'application/problem+json' } },
      ),
    onUnauthorized: () => void clearExpiredSession(queryClient, (path) => {
      destination = path
    }),
  })

  await expect(listWorkspaces({ client, throwOnError: true })).rejects.toMatchObject({ status: 401 })
  await Promise.resolve()

  expect(queryClient.getQueryData(['current-user'])).toBeUndefined()
  expect(destination).toBe('/login')
})
