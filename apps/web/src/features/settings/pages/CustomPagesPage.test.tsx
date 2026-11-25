import { afterEach, expect, test } from 'bun:test'
import { fireEvent, screen } from '@testing-library/react'
import type { CustomPage } from '@orbit/api/generated/types.gen'
import { renderWithWorkspace } from '@/test/renderWithWorkspace'
import { CustomPagesPage } from './CustomPagesPage'

const originalFetch = globalThis.fetch
afterEach(() => { globalThis.fetch = originalFetch })

const page: CustomPage = {
  id: 'p1', workspace_id: 'w', slug: 'deploys', name: 'Deploys', icon: null, admins_only: false, version: 3,
  url: 'https://old.example.com/d/1', created_at: '', updated_at: '',
}

test('a page whose origin left the list can still be renamed: its unchanged URL is not sent again', async () => {
  const patches: unknown[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    if (request.method === 'PATCH') {
      const body = await request.json() as Record<string, unknown>
      patches.push(body)
      // the server refuses a sent URL whose origin is not allowed
      if ('url' in body) return Response.json({ code: 'custom_page_origin_not_allowed', status: 422, title: 'Origin not allowed', detail: '' }, { status: 422, headers: { 'content-type': 'application/problem+json' } })
      return Response.json({ ...page, ...body, version: 4 })
    }
    return Response.json({ items: [page], can_manage: true, frame_origins: ['https://grafana.example.com'] })
  }) as typeof fetch
  renderWithWorkspace(<CustomPagesPage />, { role: 'owner' })
  fireEvent.click(await screen.findByRole('button', { name: 'Edit' }))
  fireEvent.change(screen.getByLabelText(/^Name/), { target: { value: 'Deploy board' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  await screen.findByRole('button', { name: 'Edit' })
  await new Promise((resolve) => setTimeout(resolve, 0))
  expect(patches).toEqual([{ name: 'Deploy board', slug: 'deploys', icon: null, admins_only: false, expected_version: 3 }])
  expect(screen.queryByRole('alert')).toBeNull()
})
