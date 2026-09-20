import { expect, test } from 'bun:test'
import { createApiClient } from '@/api/client'
import { labelsQueryOptions } from './labels'

test('workspace labels include unused records with their real names and colors', async () => {
  let requested = ''
  const client = createApiClient({
    fetch: async (request) => {
      requested = request.url
      const cursor = new URL(request.url).searchParams.get('cursor')
      return Response.json(cursor
        ? { items: [{ id: 'unused', workspace_id: 'workspace-1', name: 'Needs review', color: '#123456', version: 0 }], next_cursor: null }
        : { items: [{ id: 'used', workspace_id: 'workspace-1', name: 'Bug', color: '#ff0000', version: 0 }], next_cursor: 'next' })
    },
  })

  const labels = await labelsQueryOptions('workspace-1', client).queryFn()

  expect(new URL(requested).pathname).toBe('/api/v1/workspaces/workspace-1/labels')
  expect(labels.map((label) => label.name)).toEqual(['Bug', 'Needs review'])
})
