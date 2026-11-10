import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import type { DatabaseDetail } from '@orbit/api/generated/types.gen'
import type { DatabaseFormConfig } from '@orbit/database/form'
import { parseDatabase } from '@orbit/database/values'
import { render } from '@/test/render'
import { DatabaseEnvContext, type DatabaseEnv } from './context'
import { FormFill } from './DatabaseForm'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const detail: DatabaseDetail = {
  database: { id: 'db', title: 'Bugs', icon: null, locked: false, row_count: 0 },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-num', name: 'Severity', type: 'number', config: {}, position: 1 },
  ],
  views: [{ id: 'v-form', name: 'Form', type: 'form', config: {}, position: 0 }],
  rows: [],
}

const form: DatabaseFormConfig = { questions: [{ property_id: 'p-title', required: true }, { property_id: 'p-num' }], access: 'members' }

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

test('a required question left empty sends nothing; a filled form sends its answers once', async () => {
  const calls: { method: string; path: string; body: unknown }[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    calls.push({ method: request.method, path: new URL(request.url).pathname, body: JSON.parse((await request.text()) || 'null') })
    return Response.json(
      { id: 'r-new', title: 'Crash on save', icon: null, position: 0, version: 0, has_content: false, created_at: '2026-11-10T12:00:00Z', created_by: 'u1', updated_at: '2026-11-10T12:00:00Z', updated_by: 'u1', values: { 'p-num': 3 } },
      { status: 201 },
    )
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <DatabaseEnvContext.Provider value={env}>{children}</DatabaseEnvContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  )
  render(<FormFill database={parseDatabase(detail)} viewId="v-form" form={form} />, { wrapper })
  const user = userEvent.setup()

  await user.type(screen.getByLabelText('Severity'), '3')
  await user.click(screen.getByRole('button', { name: 'Submit' }))
  expect(await screen.findByText('Answer this question.')).toBeTruthy()
  expect(calls).toEqual([])

  await user.type(screen.getByLabelText(/^Name/), '  Crash on save ')
  await user.click(screen.getByRole('button', { name: 'Submit' }))
  expect(await screen.findByText('Thanks for your answer')).toBeTruthy()
  expect(calls).toEqual([{ method: 'POST', path: '/api/v1/workspaces/ws/databases/db/forms/v-form/submit', body: { answers: { 'p-title': 'Crash on save', 'p-num': 3 } } }])
})
