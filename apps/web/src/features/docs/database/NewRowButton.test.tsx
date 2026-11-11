import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { DatabaseDetail, DatabaseRow, DatabaseTemplate } from '@orbit/api/generated/types.gen'
import { parseDatabase } from '@orbit/database/values'
import { render } from '@/test/render'
import type { DatabaseEnv } from './context'
import { DatabaseSurface } from './DatabaseSurface'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const page = (id: string, position: number, title: string): DatabaseRow => ({
  id, title, icon: null, position, version: 0, has_content: false,
  created_at: '2026-11-09T10:00:00Z', created_by: 'u1', updated_at: '2026-11-09T10:00:00Z', updated_by: 'u1', values: {},
})
const template = (id: string, position: number, name: string): DatabaseTemplate => ({ ...page(id, position, name), name })

function detail(defaultTemplate?: string): DatabaseDetail {
  return {
    database: { id: 'db', title: 'Bugs', icon: null, locked: false, row_count: 1 },
    properties: [{ id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 }],
    views: [{ id: 'v1', name: 'Table', type: 'table', config: defaultTemplate ? { default_template_id: defaultTemplate } : {}, position: 0 }],
    rows: [page('r1', 0, 'Crash')],
    templates: [template('t-feature', 1, 'Feature'), template('t-bug', 0, 'Bug report')],
  }
}

const env: DatabaseEnv = {
  workspaceId: 'ws',
  databaseId: 'db',
  ctx: { userId: 'u1', now: Date.UTC(2026, 10, 10, 12), timeZone: 'UTC', locale: 'en-US', members: new Map() },
  users: new Map(),
  canEditValues: true,
  canEditSchema: true,
  canEditViews: true,
  linkId: null,
}

function setup(data: DatabaseDetail) {
  const calls: { method: string; path: string; body: Record<string, unknown> }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const body = JSON.parse((await request.text()) || 'null') as Record<string, unknown>
    const path = new URL(request.url).pathname
    calls.push({ method: request.method, path, body })
    if (path.endsWith('/rows')) return Response.json(page('r-new', 0, 'New'), { status: 201 })
    if (path.endsWith('/views/v1')) return Response.json({ ...data.views[0], config: body.config })
    return Response.json({}, { status: 404 })
  }) as unknown as typeof fetch
  function wrapper({ children }: { children: ReactNode }) {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
    client.setQueryData(['workspaces', 'ws', 'databases', 'db'], data)
    return (
      <MemoryRouter>
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      </MemoryRouter>
    )
  }
  render(<DatabaseSurface database={parseDatabase(data)} views={data.views} env={env} locked={false} viewKey="test-templates" layout={({ controls }) => controls} />, {
    wrapper,
  })
  return calls
}

const rowCalls = <T extends { path: string }>(calls: T[]) => calls.filter((call) => call.path.endsWith('/rows'))

test('"New" fills the row from the view\'s default template with the local day', async () => {
  const calls = setup(detail('t-bug'))
  await userEvent.click(screen.getByRole('button', { name: 'New' }))
  await waitFor(() => expect(rowCalls(calls).length).toBe(1))
  expect(rowCalls(calls)[0]).toEqual({
    method: 'POST',
    path: '/api/v1/workspaces/ws/databases/db/rows',
    body: { values: {}, after_row_id: null, template_id: 't-bug', today: '2026-11-10' },
  })
})

test('the arrow lists the templates in order; "Empty" makes a blank row', async () => {
  const calls = setup(detail('t-bug'))
  await userEvent.click(screen.getByRole('button', { name: 'New from a template' }))
  const list = await screen.findByRole('list', { name: 'Templates' })
  expect([...list.querySelectorAll('li')].map((item) => item.textContent)).toEqual(['Bug reportDefault', 'Feature', 'EmptySet as default'])
  await userEvent.click(screen.getByRole('button', { name: 'Feature' }))
  await waitFor(() => expect(rowCalls(calls).length).toBe(1))
  expect(rowCalls(calls)[0]?.body.template_id).toBe('t-feature')
})

test('"Empty" makes a blank row even when the view has a default template', async () => {
  const calls = setup(detail('t-bug'))
  await userEvent.click(screen.getByRole('button', { name: 'New from a template' }))
  await userEvent.click(await screen.findByRole('button', { name: 'Empty' }))
  await waitFor(() => expect(rowCalls(calls).length).toBe(1))
  expect(rowCalls(calls)[0]?.body).toEqual({ values: {}, after_row_id: null })
})

test('"Set as default for this view" saves the view config', async () => {
  const calls = setup(detail())
  await userEvent.click(screen.getByRole('button', { name: 'New from a template' }))
  await userEvent.click(await screen.findByRole('button', { name: 'Feature options' }))
  await userEvent.click(await screen.findByRole('menuitem', { name: 'Set as default for this view' }))
  await waitFor(() => expect(calls.length).toBe(1), { timeout: 2000 })
  expect(calls[0].method).toBe('PATCH')
  expect((calls[0].body.config as { default_template_id?: string }).default_template_id).toBe('t-feature')
})
