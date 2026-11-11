import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import type { ObsidianImport, ObsidianImportNode, ObsidianImportTree, PageSummary, Teamspace } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { waitForAbsence } from '@/test/waitForAbsence'
import { DocsPage } from './DocsPage'

const originalFetch = globalThis.fetch
const originalXhr = globalThis.XMLHttpRequest
afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.XMLHttpRequest = originalXhr
})

const teamspace = (id: string, name: string, position: number): Teamspace => ({
  id, workspace_id: 'workspace-1', name, icon: null, position, version: 1, is_default: position === 0,
  created_at: '2026-11-05T10:00:00Z', updated_at: '2026-11-05T10:00:00Z',
})

const summary = (id: string, parent_id: string | null, title: string, teamspace_id: string | null = 'teamspace-1'): PageSummary => ({
  id, parent_id, teamspace_id, private: teamspace_id === null, kind: 'page', position: 0, title, icon: null, version: 1,
  updated_at: '2026-11-05T10:00:00Z',
})

const node = (path: string, parent_path: string | null, title: string, kind: ObsidianImportNode['kind'] = 'note', child_count = 0): ObsidianImportNode => ({
  path, parent_path, title, kind, child_count, note_path: kind === 'note' ? path : null,
})

// Projects ─┬─ Alpha
//           └─ Archive ── Old
// Inbox
const TREE: ObsidianImportTree = {
  nodes: [
    { ...node('Projects', null, 'Projects', 'folder', 2), note_path: 'Projects/Projects.md' },
    node('Projects/Alpha.md', 'Projects', 'Alpha'),
    node('Projects/Archive', 'Projects', 'Archive', 'folder', 1),
    node('Projects/Archive/Old.md', 'Projects/Archive', 'Old'),
    node('Inbox.md', null, 'Inbox'),
  ],
  attachments: 3,
  unresolved_links: 1,
  skipped: { system: 4, canvas: 2, csv: 1, unreferenced: 0, too_large: 0, duplicates: 0 },
}

const item = (patch: Partial<ObsidianImport> = {}): ObsidianImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', file_name: 'vault.zip', archive_bytes: 2048, error: null,
  progress: { total: 0, done: 0, failed: 0 }, destination: null, tree: null, report: null, root_page_ids: [],
  created_at: '2026-11-05T10:00:00Z', updated_at: '2026-11-05T10:00:00Z', ...patch,
})

const report = (patch: Partial<NonNullable<ObsidianImport['report']>> = {}): NonNullable<ObsidianImport['report']> => ({
  files_imported: 0, missing_files: 0, unresolved_links: 0, truncated_blocks: 0, unfilled_pages_trashed: 0, failures: [],
  skipped_files: { system: 0, canvas: 0, csv: 0, unreferenced: 0, too_large: 0, duplicates: 0 }, ...patch,
})

const problemBody = (status: number, code: string) => ({
  type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports/obsidian', request_id: 'request-1',
})

const problem = (status: number, code: string) =>
  Response.json(problemBody(status, code), { status, headers: { 'content-type': 'application/problem+json' } })

/** A stand-in for `XMLHttpRequest`: the test drives upload progress and the response. */
class FakeXhr {
  static sent: FakeXhr[] = []
  url = ''
  status = 0
  response: unknown = null
  responseURL = ''
  withCredentials = false
  responseType = ''
  body: FormData | null = null
  upload: { onprogress: ((event: { lengthComputable: boolean; loaded: number; total: number }) => void) | null } = { onprogress: null }
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  onabort: (() => void) | null = null
  open(_method: string, url: string) {
    this.url = url
  }
  setRequestHeader() {}
  getResponseHeader() {
    return null
  }
  send(body: FormData) {
    this.body = body
    FakeXhr.sent.push(this)
  }
  abort() {
    this.onabort?.()
  }
  progress(loaded: number, total: number) {
    this.upload.onprogress?.({ lengthComputable: true, loaded, total })
  }
  finish(status: number, response: unknown) {
    this.status = status
    this.response = response
    this.onload?.()
  }
}

type Call = { method: string; path: string; search: string; body: Record<string, unknown> | undefined }
type Handler = (call: Call) => Response | undefined

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}</output>
}

const PAGES = [summary('page-roadmap', null, 'Roadmap'), summary('page-child', 'page-roadmap', 'Q3')]

function setup(path: string, handler: Handler = () => undefined) {
  window.localStorage.clear()
  FakeXhr.sent = []
  globalThis.XMLHttpRequest = FakeXhr as unknown as typeof XMLHttpRequest
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = {
      method: request.method,
      path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''),
      search: url.search,
      body: text ? JSON.parse(text) : undefined,
    }
    calls.push(call)
    const response = handler(call)
    if (response) return response
    if (call.method === 'GET' && call.path === '/teamspaces') return Response.json({ items: [teamspace('teamspace-1', 'General', 0), teamspace('teamspace-2', 'Design', 1)] })
    if (call.method === 'GET' && call.path === '/pages') return Response.json({ items: PAGES })
    if (call.method === 'GET' && call.path === '/imports/obsidian') return Response.json({ items: [] })
    return problem(404, 'not_found')
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'owner', permissions: [], version: 1 }])
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`${path}?workspace=workspace-1`]}>
        <ConfirmationModalHost />
        <WorkspaceProvider>
          <Routes>
            <Route path="docs/import/obsidian/:importId?" element={<DocsPage view="obsidian-import" />} />
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
const choose = (view: ReturnType<typeof render>, file: File) => fireEvent.change(view.getByLabelText('Vault .zip file'), { target: { files: [file] } })
const zip = () => new File(['PK'], 'vault.zip', { type: 'application/zip' })

test('the Documents "…" menu opens the Obsidian import', async () => {
  const { view } = setup('/docs/page-roadmap', (call) => {
    if (call.path === '/pages/page-roadmap') return problem(404, 'page_not_found')
  })
  fireEvent.click(await view.findByRole('button', { name: 'Documents options' }))
  await userEvent.click(await view.findByRole('menuitem', { name: 'Import from Obsidian' }))
  await waitFor(() => expect(location(view)).toBe('/docs/import/obsidian'))
  expect(await view.findByText('Upload your vault')).toBeTruthy()
})

test('upload: progress, then the new import opens while it is scanned', async () => {
  const { view } = setup('/docs/import/obsidian', (call) => {
    if (call.path === '/imports/obsidian/import-1') return Response.json(item())
  })
  await view.findByText('Upload your vault')
  choose(view, zip())
  const request = FakeXhr.sent[0]
  expect(request.url).toBe('/api/v1/workspaces/workspace-1/imports/obsidian')
  expect((request.body!.get('file') as File).name).toBe('vault.zip')
  request.progress(2, 5)
  expect(await view.findByText('40%')).toBeTruthy()
  expect(view.getByRole('progressbar', { name: 'Upload progress' }).getAttribute('aria-valuenow')).toBe('40')
  request.finish(201, item())
  await waitFor(() => expect(location(view)).toBe('/docs/import/obsidian/import-1'))
  expect(await view.findByText('Reading “vault.zip”…')).toBeTruthy()
})

test('upload: a file that is not a .zip is refused before sending', async () => {
  const { view } = setup('/docs/import/obsidian')
  await view.findByText('Upload your vault')
  choose(view, new File(['# Note'], 'note.md', { type: 'text/markdown' }))
  expect((await view.findByRole('alert')).textContent).toBe('Upload the vault as one .zip file. ')
  expect(FakeXhr.sent.length).toBe(0)
})

test('upload: an import in progress links to the running import', async () => {
  let listed: ObsidianImport[] = []
  const { view } = setup('/docs/import/obsidian', (call) => {
    if (call.method === 'GET' && call.path === '/imports/obsidian') return Response.json({ items: listed })
  })
  await view.findByText('Upload your vault')
  listed = [item({ id: 'running', status: 'importing', file_name: 'notes.zip', progress: { total: 10, done: 4, failed: 0 } })]
  choose(view, zip())
  FakeXhr.sent[0].finish(409, problemBody(409, 'import_in_progress'))
  const link = await view.findByRole('link', { name: 'Open the running import' })
  expect(link.getAttribute('href')).toBe('/docs/import/obsidian/running')
  const recent = view.getByRole('region', { name: 'Recent imports' })
  expect(within(recent).getByText('notes.zip')).toBeTruthy()
  expect(within(recent).getByText('4 of 10 pages')).toBeTruthy()
})

/** Like the server: the detail carries the scan tree only with `?include=tree`. */
const withTree = (call: Call) => (call.search === '?include=tree' ? TREE : null)

test('choose: whole vault selected, counts, a partial selection by path and a destination', async () => {
  let started: Call | undefined
  const { view, calls } = setup('/docs/import/obsidian/import-1', (call) => {
    if (call.path === '/imports/obsidian/import-1') return Response.json(item({ status: 'ready', tree: withTree(call) }))
    if (call.path === '/imports/obsidian/import-1/start') {
      started = call
      return Response.json(item({ status: 'queued', progress: { total: 2, done: 0, failed: 0 } }))
    }
  })
  expect(await view.findByText('5 pages selected')).toBeTruthy()
  expect(calls.filter((call) => call.path === '/imports/obsidian/import-1').map((call) => call.search)).toEqual(['', '?include=tree'])
  expect(view.getByRole('status', { name: 'Vault contents' }).textContent).toBe(
    '3 attachments · 1 unresolved link (kept as text) · 7 skipped filesSkipped: 4 settings and system files (.obsidian/, .trash/), 2 canvases, 1 CSV file',
  )
  const tree = view.getByRole('tree', { name: 'Obsidian notes' })
  expect(within(tree).getAllByRole('treeitem').map((row) => row.textContent)).toEqual(['Projects2', 'Inbox'])

  fireEvent.click(within(tree).getByRole('button', { name: 'Expand Projects' }))
  // Unchecking Archive removes Old with it and un-checks Projects, which becomes indeterminate.
  fireEvent.click(within(tree).getByRole('checkbox', { name: 'Select Archive' }))
  expect(view.getByText('2 pages selected')).toBeTruthy()
  expect(within(tree).getByRole('checkbox', { name: 'Select Projects' }).getAttribute('aria-checked')).toBe('mixed')

  fireEvent.click(view.getByRole('combobox', { name: 'Inside page' }))
  await userEvent.click(await view.findByRole('option', { name: 'Roadmap' }))
  await waitFor(() => expect(view.getByRole('combobox', { name: 'Inside page' }).textContent).toContain('Roadmap'))

  fireEvent.click(view.getByRole('button', { name: 'Import 2 pages' }))
  await waitFor(() => expect(started).toBeDefined())
  expect(started!.body).toEqual({ selection: { paths: ['Projects/Alpha.md', 'Inbox.md'] }, destination: { parent_page_id: 'page-roadmap' } })
  expect(await view.findByRole('progressbar', { name: 'Import progress' })).toBeTruthy()
})

test('choose: the whole vault into the default teamspace sends { all: true }', async () => {
  let started: Call | undefined
  const { view } = setup('/docs/import/obsidian/import-1', (call) => {
    if (call.path === '/imports/obsidian/import-1') return Response.json(item({ status: 'ready', tree: withTree(call) }))
    if (call.path === '/imports/obsidian/import-1/start') {
      started = call
      return problem(409, 'obsidian_import_state_conflict')
    }
  })
  await waitFor(() => expect(view.getByRole('combobox', { name: 'Import into' }).textContent).toContain('General'))
  fireEvent.click(view.getByRole('button', { name: 'Import 5 pages' }))
  expect((await view.findByRole('alert')).textContent).toBe('This import changed in the meantime (it may have expired or been cancelled).')
  expect(started!.body).toEqual({ selection: { all: true }, destination: { teamspace_id: 'teamspace-1' } })
})

test('scanning: cancel asks first, then shows the cancelled import', async () => {
  let cancelled = false
  const { view } = setup('/docs/import/obsidian/import-1', (call) => {
    if (call.path === '/imports/obsidian/import-1/cancel') {
      cancelled = true
      return Response.json(item({ status: 'cancelled' }))
    }
    if (call.path === '/imports/obsidian/import-1') return Response.json(item())
  })
  fireEvent.click(await view.findByRole('button', { name: 'Cancel' }))
  const dialog = await view.findByRole('dialog')
  expect(dialog.textContent).toContain('Orbit deletes the uploaded file.')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel import' }))
  expect(await view.findByText('Import cancelled')).toBeTruthy()
  expect(cancelled).toBe(true)
})

test('failed: a refused archive shows its reason and offers another upload', async () => {
  const { view } = setup('/docs/import/obsidian/import-1', (call) => {
    if (call.path === '/imports/obsidian/import-1')
      return Response.json(item({ status: 'failed', error: 'The archive contains an unsafe path: ../evil.md' }))
  })
  expect(await view.findByText('Import failed')).toBeTruthy()
  expect(view.getByRole('alert').textContent).toBe('The archive contains an unsafe path: ../evil.md')
  expect(view.getByRole('link', { name: 'Upload another file' }).getAttribute('href')).toBe('/docs/import/obsidian')
  await waitForAbsence(() => view.queryByRole('button', { name: 'Open imported pages' }))
})

test('done: counts, conversion notes, skipped files, failures and the imported pages', async () => {
  const { view } = setup('/docs/import/obsidian/import-1', (call) => {
    if (call.path === '/imports/obsidian/import-1')
      return Response.json(
        item({
          status: 'completed',
          progress: { total: 5, done: 4, failed: 1 },
          destination: { teamspace_id: 'teamspace-2', private: false, parent_page_id: null },
          root_page_ids: ['page-roadmap', 'page-gone'],
          report: report({
            files_imported: 3,
            missing_files: 1,
            unresolved_links: 2,
            skipped_files: { system: 4, canvas: 0, csv: 1, unreferenced: 2, too_large: 0, duplicates: 0 },
            failures: [{ path: 'Projects/Archive/Old.md', title: 'Old', error: 'The note is not valid UTF-8.' }],
          }),
        }),
      )
  })
  expect(await view.findByText('Import complete')).toBeTruthy()
  expect(view.getByText('Destination: Design')).toBeTruthy()
  expect(view.getByText('Pages imported').previousSibling?.textContent).toBe('4')
  expect(view.getByText('Files imported').previousSibling?.textContent).toBe('3')
  expect(view.getByText('Pages failed').previousSibling?.textContent).toBe('1')
  expect(view.getByText('1 linked file could not be attached.')).toBeTruthy()
  expect(view.getByText('2 links point to notes or files that were not imported; they stayed as text.')).toBeTruthy()
  expect(view.getByText('4 settings and system files (.obsidian/, .trash/), 1 CSV file, 2 files no note links to')).toBeTruthy()
  const failures = view.getByRole('region', { name: 'Failed pages' })
  expect(failures.textContent).toBe('Failed pagesOldThe note is not valid UTF-8.')
  const imported = view.getByRole('region', { name: 'Imported pages' })
  await waitFor(() => expect(within(imported).getByRole('link', { name: 'Roadmap' }).getAttribute('href')).toBe('/docs/page-roadmap'))
  expect(view.getByRole('link', { name: 'Import more' }).getAttribute('href')).toBe('/docs/import/obsidian')
  fireEvent.click(view.getByRole('button', { name: 'Open imported pages' }))
  await waitFor(() => expect(location(view)).toBe('/docs/page-roadmap'))
})

test('an unknown import shows "Import not found" with a new upload', async () => {
  const { view } = setup('/docs/import/obsidian/nope', (call) => {
    if (call.path === '/imports/obsidian/nope') return problem(404, 'obsidian_import_not_found')
  })
  expect(await view.findByText('Import not found')).toBeTruthy()
  expect(view.getByRole('link', { name: 'New import' }).getAttribute('href')).toBe('/docs/import/obsidian')
})
