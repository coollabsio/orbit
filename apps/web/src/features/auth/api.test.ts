import { expect, test } from 'bun:test'
import { createApiClient } from '../../api/client'
import { currentUserQueryOptions } from './api'

test('current user reads the generated endpoint and never falls back to mock identity', async () => {
  let path = ''
  const client = createApiClient({
    fetch: async (request) => {
      path = new URL(request.url).pathname
      return Response.json({ id: 'user-1', email: 'owner@orbit.test', display_name: 'Owner' })
    },
  })

  const user = await currentUserQueryOptions(client).queryFn()

  expect(path).toBe('/api/v1/auth/me')
  expect(user).toEqual({ id: 'user-1', email: 'owner@orbit.test', display_name: 'Owner' })
})

test('current user exposes server failure instead of mock fallback', async () => {
  const client = createApiClient({
    fetch: async () => Response.json({ detail: 'unavailable' }, { status: 503 }),
  })

  await expect(currentUserQueryOptions(client).queryFn()).rejects.toMatchObject({ status: 503 })
})
