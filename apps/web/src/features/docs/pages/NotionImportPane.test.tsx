import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { NotionImport, NotionImportNode, PageSummary, Teamspace } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { waitForAbsence } from '@/test/waitForAbsence'
import { DocsPage } from './DocsPage'

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const teamspace = (id: string, name: string, position: number): Teamspace => ({
  id, workspace_id: 'workspace-1', name, icon: null, position, version: 1, is_default: position === 0,
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z',
})

const summary = (id: string, parent_id: string | null, title: string, teamspace_id: string | null = 'teamspace-1'): PageSummary => ({
  id, parent_id, teamspace_id, private: teamspace_id === null, position: 0, title, icon: null, version: 1,
  updated_at: '2026-09-25T10:00:00Z',
})

const node = (notion_id: string, parent_id: string | null, title: string, kind: NotionImportNode['kind'] = 'page'): NotionImportNode => ({
  notion_id, parent_id, title, kind, icon: null, child_count: 0,
})

const TREE: NotionImportNode[] = [
  { ...node('handbook', null, 'Handbook'), icon: '📘', child_count: 2 },
  { ...node('alpha', 'handbook', 'Alpha'), child_count: 1 },
  node('toggle', 'alpha', 'In toggle'),
  { ...node('beta', 'handbook', 'Beta'), child_count: 1 },
  { ...node('tasks', 'beta', 'Tasks', 'database'), child_count: 2 },
  node('row1', 'tasks', 'Row one'),
  node('row2', 'tasks', ''),
  node('orphan', null, 'Orphan'),
]

const item = (patch: Partial<NotionImport> = {}): NotionImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', notion_workspace_name: 'Acme Notion', error: null,
  progress: { total: 0, done: 0, failed: 0 }, destination: null, tree: null, report: null, root_page_ids: [],
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z', ...patch,
})

const report = (patch: Partial<NonNullable<NotionImport['report']>> = {}): NonNullable<NotionImport['report']> => ({
  skipped: {}, lossy: {}, unresolved_page_links: 0, missing_files: 0, truncated_blocks: 0, files_imported: 0,
  previously_imported: 0, unfilled_pages_trashed: 0, failures: [], ...patch,
})

const problem = (status: number, code: string) =>
  Response.json(
    { type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports', request_id: 'request-1' },
    { status, headers: { 'content-type': 'application/problem+json' } },
  )

type Call = { method: string; path: string; body: Record<string, unknown> | undefined }
type Handler = (call: Call) => Response | undefined

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}</output>
}

const PAGES = [summary('page-roadmap', null, 'Roadmap'), summary('page-child', 'page-roadmap', 'Q3'), summary('page-mine', null, 'Mine', null)]

function setup(path: string, handler: Handler) {
  window.localStorage.clear()
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = { method: request.method, path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''), body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    const response = handler(call)
    if (response) return response
    if (call.method === 'GET' && call.path === '/teamspaces') return Response.json({ items: [teamspace('teamspace-1', 'General', 0), teamspace('teamspace-2', 'Design', 1)] })
    if (call.method === 'GET' && call.path === '/pages') return Response.json({ items: PAGES })
    if (call.method === 'GET' && call.path === '/imports/notion') return Response.json({ items: [] })
    return problem(404, 'not_found')
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', version: 1 }])
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`${path}?workspace=workspace-1`]}>
        <ConfirmationModalHost />
        <WorkspaceProvider>
          <Routes>
            <Route path="docs/import/:importId?" element={<DocsPage view="import" />} />
            <Route path="docs/:pageId?" element={<DocsPage />} />
          </Routes>
        </WorkspaceProvider>
        <Location />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { view, calls, client }
}

const location = (view: ReturnType<typeof render>) => view.getByTestId('location').textContent

/** `userEvent.clear` does not reach Base UI's controlled Input under happy-dom; backspacing does. */
const clearInput = (input: HTMLElement) => userEvent.type(input, '{Backspace}'.repeat((input as HTMLInputElement).value.length))

test('the Documents "…" menu opens the import pane', async () => {
  const { view } = setup('/docs/page-roadmap', (call) => {
    if (call.path === '/pages/page-roadmap') return problem(404, 'page_not_found')
  })
  fireEvent.click(await view.findByRole('button', { name: 'Documents options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Import from Notion' }))
  await waitFor(() => expect(location(view)).toBe('/docs/import'))
  expect(await view.findByText('Connect Notion')).toBeTruthy()
})

test('connect: the token field is a password field and problems show friendly messages', async () => {
  let code = 'notion_token_invalid'
  const { view, calls } = setup('/docs/import', (call) => {
    if (call.method === 'POST' && call.path === '/imports/notion') return problem(code === 'app_key_missing' ? 409 : 422, code)
  })
  const input = (await view.findByLabelText('Notion token')) as HTMLInputElement
  expect(input.type).toBe('password')
  expect(input.autocomplete).toBe('off')
  expect(view.getByText('https://www.notion.so/developers/tokens').getAttribute('href')).toBe('https://www.notion.so/developers/tokens')
  const scan = view.getByRole('button', { name: 'Scan workspace' }) as HTMLButtonElement
  expect(scan.disabled).toBe(true)

  await userEvent.type(input, 'ntn_invalid')
  fireEvent.click(scan)
  expect((await view.findByRole('alert')).textContent).toContain('Notion rejected this token')
  expect(calls.find((call) => call.method === 'POST')?.body).toEqual({ token: 'ntn_invalid' })

  code = 'app_key_missing'
  fireEvent.click(scan)
  await waitFor(() =>
    expect(view.getByRole('alert').textContent).toContain('The server administrator must set an app key'),
  )
})

test('connect: an import in progress links to the running import', async () => {
  let listed: NotionImport[] = []
  const { view } = setup('/docs/import', (call) => {
    if (call.method === 'GET' && call.path === '/imports/notion') return Response.json({ items: listed })
    if (call.method === 'POST' && call.path === '/imports/notion') {
      listed = [item({ id: 'running', status: 'importing', progress: { total: 10, done: 4, failed: 0 } })]
      return problem(409, 'import_in_progress')
    }
  })
  await userEvent.type(await view.findByLabelText('Notion token'), 'ntn_x')
  fireEvent.click(view.getByRole('button', { name: 'Scan workspace' }))
  const link = await view.findByRole('link', { name: 'Open the running import' })
  expect(link.getAttribute('href')).toBe('/docs/import/running')
  // The recent imports list shows it too.
  const recent = view.getByRole('region', { name: 'Recent imports' })
  expect(within(recent).getByText('4 of 10 pages')).toBeTruthy()
  expect(within(recent).getByText('Importing')).toBeTruthy()
})

test('connect → scanning: a valid token opens the import, which shows the Notion workspace while scanning', async () => {
  const { view } = setup('/docs/import', (call) => {
    if (call.method === 'POST' && call.path === '/imports/notion') return Response.json(item(), { status: 201 })
    if (call.path === '/imports/notion/import-1') return Response.json(item())
  })
  await userEvent.type(await view.findByLabelText('Notion token'), 'ntn_good')
  fireEvent.click(view.getByRole('button', { name: 'Scan workspace' }))
  await waitFor(() => expect(location(view)).toBe('/docs/import/import-1'))
  expect(await view.findByText('Scanning “Acme Notion”…')).toBeTruthy()
  expect(view.queryByLabelText('Notion token')).toBeNull()
})

test('choose: tri-state tree, filter, counts, destination and a minimal selection', async () => {
  let started: Call | undefined
  const { view } = setup('/docs/import/import-1', (call) => {
    if (call.path === '/imports/notion/import-1') return Response.json(item({ status: 'ready', tree: { nodes: TREE, truncated: true, incomplete: false } }))
    if (call.path === '/imports/notion/import-1/start') {
      started = call
      return Response.json(item({ status: 'queued', progress: { total: 4, done: 0, failed: 0 } }))
    }
  })
  expect(await view.findByText('8 pages selected')).toBeTruthy()
  expect(view.getByText(/more than 5,000 pages/)).toBeTruthy()
  const tree = view.getByRole('tree', { name: 'Notion pages' })
  expect(within(tree).getAllByRole('treeitem').map((row) => row.textContent)).toEqual(['📘Handbook2', 'Orphan'])

  fireEvent.click(within(tree).getByRole('button', { name: 'Expand Handbook' }))
  fireEvent.click(within(tree).getByRole('button', { name: 'Expand Beta' }))
  // Unchecking Beta removes its subtree and un-checks Handbook, which becomes indeterminate.
  fireEvent.click(within(tree).getByRole('checkbox', { name: 'Select Beta' }))
  expect(view.getByText('3 pages selected')).toBeTruthy()
  const handbook = within(tree).getByRole('checkbox', { name: 'Select Handbook' })
  expect(handbook.getAttribute('aria-checked')).toBe('mixed')
  expect(within(tree).getByRole('checkbox', { name: 'Select Tasks' }).getAttribute('aria-checked')).toBe('false')

  // Filter: matches and their ancestors.
  const filter = view.getByLabelText('Filter pages')
  await userEvent.type(filter, 'row one')
  expect(within(tree).getAllByRole('treeitem').map((row) => row.textContent)).toEqual(['📘Handbook2', 'Beta1', 'Tasks2', 'Row one'])
  fireEvent.click(within(tree).getByRole('checkbox', { name: 'Select Row one' }))
  expect(view.getByText('4 pages selected')).toBeTruthy()
  await clearInput(filter)
  await userEvent.type(filter, 'zzz')
  expect(view.getByText('No pages match “zzz”.')).toBeTruthy()

  fireEvent.click(view.getByRole('button', { name: 'Clear' }))
  expect(view.getByText('0 pages selected')).toBeTruthy()
  expect((view.getByRole('button', { name: 'Import 0 pages' }) as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(view.getByRole('button', { name: 'Select all' }))
  expect(view.getByText('8 pages selected')).toBeTruthy()
  await clearInput(filter)
  fireEvent.click(within(tree).getByRole('checkbox', { name: 'Select Alpha' }))
  expect(view.getByText('5 pages selected')).toBeTruthy()

  // Destination: the default teamspace, then "Inside page" lists that space's pages only.
  const space = view.getByRole('combobox', { name: 'Import into' })
  expect(space.textContent).toContain('General')
  fireEvent.click(view.getByRole('combobox', { name: 'Inside page' }))
  const options = await view.findAllByRole('option')
  expect(options.map((option) => option.textContent)).toEqual(['Top level', 'Roadmap', 'Q3'])
  await userEvent.click(options[2])
  await waitFor(() => expect(view.getByRole('combobox', { name: 'Inside page' }).textContent).toContain('Q3'))

  fireEvent.click(view.getByRole('button', { name: 'Import 5 pages' }))
  await waitFor(() => expect(started).toBeDefined())
  expect(started!.body).toEqual({ selection: { notion_ids: ['beta', 'orphan'] }, destination: { parent_page_id: 'page-child' } })
  expect(await view.findByRole('progressbar', { name: 'Import progress' })).toBeTruthy()
})

test('choose: everything selected into Private sends { all: true } and { private: true }', async () => {
  let started: Call | undefined
  const { view } = setup('/docs/import/import-1', (call) => {
    if (call.path === '/imports/notion/import-1') return Response.json(item({ status: 'ready', tree: { nodes: TREE, truncated: false, incomplete: false } }))
    if (call.path === '/imports/notion/import-1/start') {
      started = call
      return problem(422, 'notion_import_too_large')
    }
  })
  fireEvent.click(await view.findByRole('combobox', { name: 'Import into' }))
  await userEvent.click(await view.findByRole('option', { name: 'Private' }))
  await waitFor(() => expect(view.getByRole('combobox', { name: 'Import into' }).textContent).toContain('Private'))
  fireEvent.click(view.getByRole('button', { name: 'Import 8 pages' }))
  expect((await view.findByRole('alert')).textContent).toContain('at most 5,000 pages')
  expect(started!.body).toEqual({ selection: { all: true }, destination: { private: true } })
})

test('importing: progress, failed count and a confirmed cancel', async () => {
  let cancelled = false
  const { view } = setup('/docs/import/import-1', (call) => {
    if (call.path === '/imports/notion/import-1/cancel') {
      cancelled = true
      return Response.json(item({ status: 'cancelled', progress: { total: 10, done: 3, failed: 1 }, report: report({ unfilled_pages_trashed: 6 }) }))
    }
    if (call.path === '/imports/notion/import-1') return Response.json(item({ status: 'importing', progress: { total: 10, done: 3, failed: 1 } }))
  })
  const bar = await view.findByRole('progressbar', { name: 'Import progress' })
  expect(bar.getAttribute('aria-valuenow')).toBe('4')
  expect(bar.getAttribute('aria-valuemax')).toBe('10')
  expect(view.getByText('3 of 10 pages imported')).toBeTruthy()
  expect(view.getByText('1 failed')).toBeTruthy()
  fireEvent.click(view.getByRole('button', { name: 'Cancel import' }))
  const dialog = await view.findByRole('dialog')
  expect(dialog.textContent).toContain('Empty pages the import created but did not fill yet are moved to the trash.')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel import' }))
  expect(await view.findByText('Import cancelled')).toBeTruthy()
  expect(cancelled).toBe(true)
  expect(view.getByText('6 empty pages the import had created were moved to the trash.')).toBeTruthy()
})

test('done: summary with files, skipped and lossy blocks, failures, and links to the imported pages', async () => {
  const { view } = setup('/docs/import/import-1', (call) => {
    if (call.path === '/imports/notion/import-1')
      return Response.json(
        item({
          status: 'completed',
          progress: { total: 8, done: 7, failed: 1 },
          destination: { teamspace_id: 'teamspace-2', private: false, parent_page_id: null },
          root_page_ids: ['page-roadmap', 'page-gone'],
          report: report({
            files_imported: 3,
            skipped: { embed: 2, synced_block: 1 },
            lossy: { column_list: 1 },
            missing_files: 1,
            failures: [{ notion_id: 'orphan', title: 'Orphan', error: 'Could not find object.' }],
          }),
        }),
      )
  })
  expect(await view.findByText('Import complete')).toBeTruthy()
  expect(view.getByText('Destination: Design')).toBeTruthy()
  expect(view.getByText('Files imported').previousSibling?.textContent).toBe('3')
  expect(view.getByText('Pages imported').previousSibling?.textContent).toBe('7')
  expect(view.getByText('embed (2), synced block (1)')).toBeTruthy()
  expect(view.getByText('column list (1)')).toBeTruthy()
  expect(view.getByText('1 Notion files could not be downloaded.')).toBeTruthy()
  const failures = view.getByRole('region', { name: 'Failed pages' })
  expect(failures.textContent).toContain('Orphan')
  expect(failures.textContent).toContain('Could not find object.')
  const imported = view.getByRole('region', { name: 'Imported pages' })
  await waitFor(() => expect(within(imported).getByRole('link', { name: 'Roadmap' }).getAttribute('href')).toBe('/docs/page-roadmap'))
  expect(within(imported).getByRole('link', { name: 'Imported page' })).toBeTruthy()
  expect(view.getByRole('link', { name: 'Import more' }).getAttribute('href')).toBe('/docs/import')
  fireEvent.click(view.getByRole('button', { name: 'Open imported pages' }))
  await waitFor(() => expect(location(view)).toBe('/docs/page-roadmap'))
})

test('failed: shows the error; an unknown import shows "Import not found"', async () => {
  const failed = setup('/docs/import/import-1', (call) => {
    if (call.path === '/imports/notion/import-1')
      return Response.json(item({ status: 'failed', error: 'The Notion token was revoked or is no longer valid.', progress: { total: 8, done: 0, failed: 0 }, report: report({ unfilled_pages_trashed: 8 }) }))
  })
  expect(await failed.view.findByText('Import failed')).toBeTruthy()
  expect(failed.view.getByText('The Notion token was revoked or is no longer valid.')).toBeTruthy()
  expect(failed.view.getByText('8 empty pages the import had created were moved to the trash.')).toBeTruthy()
  await waitForAbsence(() => failed.view.queryByRole('button', { name: 'Open imported pages' }))
  failed.view.unmount()

  const missing = setup('/docs/import/nope', (call) => {
    if (call.path === '/imports/notion/nope') return problem(404, 'notion_import_not_found')
  })
  expect(await missing.view.findByText('Import not found')).toBeTruthy()
  expect(missing.view.getByRole('link', { name: 'New import' }).getAttribute('href')).toBe('/docs/import')
})
