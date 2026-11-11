import { afterEach, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import type { ObsidianImport, ObsidianImportTree } from '@orbit/api/generated/types.gen'
import { CONTRACT_ID } from '@/api/client'
import { ApiProblem } from '@orbit/api/problem'
import { queryKeys } from '@/api/queryKeys'
import {
  obsidianImportErrorMessage,
  skippedSummary,
  uploadObsidianArchive,
  useCancelObsidianImport,
  useObsidianImport,
  useObsidianImportTree,
  useStartObsidianImport,
} from './obsidianImports'

const originalFetch = globalThis.fetch
const originalXhr = globalThis.XMLHttpRequest
afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.XMLHttpRequest = originalXhr
})

const obsidianImport = (patch: Partial<ObsidianImport> = {}): ObsidianImport => ({
  id: 'import-1', workspace_id: 'workspace-1', status: 'scanning', file_name: 'vault.zip', archive_bytes: 2048, error: null,
  progress: { total: 0, done: 0, failed: 0 }, destination: null, tree: null, report: null, root_page_ids: [],
  created_at: '2026-11-05T10:00:00Z', updated_at: '2026-11-05T10:00:00Z', ...patch,
})

const problem = (status: number, code: string) =>
  new ApiProblem({ type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports', request_id: 'request-1' })

/** A stand-in for `XMLHttpRequest`: the test drives progress and the response. */
class FakeXhr {
  static sent: FakeXhr[] = []
  method = ''
  url = ''
  headers: Record<string, string> = {}
  withCredentials = false
  responseType = ''
  status = 0
  response: unknown = null
  responseURL = ''
  body: FormData | null = null
  upload: { onprogress: ((event: { lengthComputable: boolean; loaded: number; total: number }) => void) | null } = { onprogress: null }
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  onabort: (() => void) | null = null
  open(method: string, url: string) {
    this.method = method
    this.url = url
  }
  setRequestHeader(name: string, value: string) {
    this.headers[name] = value
  }
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

function installXhr() {
  FakeXhr.sent = []
  globalThis.XMLHttpRequest = FakeXhr as unknown as typeof XMLHttpRequest
}

const problemBody = (status: number, code: string) => ({
  type: 'about:blank', title: 'Problem', status, code, detail: code, instance: '/imports/obsidian', request_id: 'request-1',
})

type Call = { method: string; path: string; search: string; body: unknown }

function mockFetch(handler: (call: Call) => Response | Promise<Response>) {
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const text = await request.text()
    const url = new URL(request.url)
    const call = { method: request.method, path: url.pathname, search: url.search, body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    return handler(call)
  }) as unknown as typeof fetch
  return calls
}

function setupClient() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return { client, wrapper }
}

const base = '/api/v1/workspaces/workspace-1/imports/obsidian'
const vault = () => new File(['PK'], 'vault.zip', { type: 'application/zip' })

test('the upload posts the zip as the `file` field with credentials and reports progress', async () => {
  installXhr()
  const progress: number[] = []
  const pending = uploadObsidianArchive('workspace-1', vault(), { onProgress: (fraction) => progress.push(fraction) })
  const request = FakeXhr.sent[0]
  expect([request.method, request.url]).toEqual(['POST', base])
  expect(request.headers).toEqual({ 'X-Orbit-Contract': CONTRACT_ID })
  expect(request.withCredentials).toBe(true)
  expect((request.body!.get('file') as File).name).toBe('vault.zip')
  request.progress(1, 4)
  request.progress(4, 4)
  request.finish(201, obsidianImport())
  expect(await pending).toEqual(obsidianImport())
  expect(progress).toEqual([0.25, 1])
})

test('upload failures become friendly messages', async () => {
  installXhr()
  const failure = async (finish: (request: FakeXhr) => void) => {
    const pending = uploadObsidianArchive('workspace-1', vault())
    finish(FakeXhr.sent[FakeXhr.sent.length - 1])
    return pending.then(
      () => 'resolved',
      (error: unknown) => obsidianImportErrorMessage(error),
    )
  }
  expect(await failure((request) => request.finish(409, problemBody(409, 'import_in_progress')))).toBe(
    'You already have an import running. Wait for it to finish or cancel it.',
  )
  expect(await failure((request) => request.finish(422, problemBody(422, 'obsidian_archive_invalid')))).toBe('Upload the vault as one .zip file.')
  // The body limit answers 413 without a problem document.
  expect(await failure((request) => request.finish(413, null))).toBe('The file is larger than the server’s upload limit.')
  expect(await failure((request) => request.onerror?.())).toBe('The server could not be reached. Check your connection and try again.')
})

test('aborting the upload rejects with an AbortError', async () => {
  installXhr()
  const controller = new AbortController()
  const pending = uploadObsidianArchive('workspace-1', vault(), { signal: controller.signal })
  controller.abort()
  const error = await pending.then(
    () => null,
    (caught: unknown) => caught,
  )
  expect(error instanceof DOMException && error.name).toBe('AbortError')
})

test('problems from start and cancel map to friendly messages', () => {
  expect(obsidianImportErrorMessage(problem(413, 'obsidian_archive_too_large'))).toBe(
    'The file is larger than the server’s upload limit. Remove large attachments from the vault and zip it again.',
  )
  expect(obsidianImportErrorMessage(problem(422, 'obsidian_import_too_large'))).toBe('An import can contain at most 5,000 pages. Select fewer notes.')
  expect(obsidianImportErrorMessage(problem(404, 'teamspace_not_found'))).toBe('The destination teamspace no longer exists. Choose another one.')
  expect(obsidianImportErrorMessage(new Error('boom'))).toBe('Something went wrong. Try again.')
})

test('skipped files read as counted parts without the zero kinds', () => {
  expect(skippedSummary({ system: 4, canvas: 1, csv: 2, unreferenced: 0, too_large: 0, duplicates: 1 })).toEqual({
    total: 8,
    parts: [
      '4 settings and system files (.obsidian/, .trash/)',
      '1 canvas',
      '2 CSV files',
      '1 file whose name differs only in letter case',
    ],
  })
  expect(skippedSummary({ system: 0, canvas: 0, csv: 0, unreferenced: 0, too_large: 0, duplicates: 0 })).toEqual({ total: 0, parts: [] })
})

test('an import is polled while scanning and stops at a final state, refreshing the page tree', async () => {
  let status: ObsidianImport['status'] = 'scanning'
  const calls = mockFetch(() => Response.json(obsidianImport({ status })))
  const polls = () => calls.filter((call) => call.path === `${base}/import-1`).length
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [])
  const view = renderHook(() => useObsidianImport('workspace-1', 'import-1', 20), { wrapper })
  await waitFor(() => expect(view.result.current.data?.status).toBe('scanning'))
  await waitFor(() => expect(polls()).toBeGreaterThanOrEqual(3))

  // A refused archive ends the scan as failed: polling stops and the pages are refreshed.
  status = 'failed'
  await waitFor(() => expect(view.result.current.data?.status).toBe('failed'))
  await waitFor(() => expect(client.getQueryState(queryKeys.pages.tree('workspace-1'))?.isInvalidated).toBe(true))
  const final = polls()
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(polls()).toBe(final)
  expect(calls.every((call) => call.search === '')).toBe(true)
})

test('the scan tree is fetched once with ?include=tree under its own key', async () => {
  const tree: ObsidianImportTree = {
    nodes: [{ path: 'Inbox.md', parent_path: null, note_path: 'Inbox.md', kind: 'note', title: 'Inbox', child_count: 0 }],
    attachments: 2,
    unresolved_links: 1,
    skipped: { system: 3, canvas: 0, csv: 0, unreferenced: 0, too_large: 0, duplicates: 0 },
  }
  const calls = mockFetch((call) => Response.json(obsidianImport({ status: 'ready', tree: call.search === '?include=tree' ? tree : null })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useObsidianImportTree('workspace-1', 'import-1'), { wrapper })
  await waitFor(() => expect(view.result.current.data).toEqual(tree))
  await act(() => new Promise((resolve) => setTimeout(resolve, 80)))
  expect(calls).toEqual([{ method: 'GET', path: `${base}/import-1`, search: '?include=tree', body: undefined }])
  expect(client.getQueryData(queryKeys.obsidianImports.detail('workspace-1', 'import-1'))).toBeUndefined()
})

test('start sends the selected paths and the destination and stores the queued import', async () => {
  const calls = mockFetch(() => Response.json(obsidianImport({ status: 'queued', progress: { total: 3, done: 0, failed: 0 } })))
  const { client, wrapper } = setupClient()
  const view = renderHook(() => useStartObsidianImport('workspace-1'), { wrapper })
  await act(() =>
    view.result.current.mutateAsync({ importId: 'import-1', selection: { paths: ['Projects'] }, destination: { teamspace_id: 'teamspace-1' } }),
  )
  expect(calls[0]).toEqual({
    method: 'POST',
    path: `${base}/import-1/start`,
    search: '',
    body: { selection: { paths: ['Projects'] }, destination: { teamspace_id: 'teamspace-1' } },
  })
  expect(client.getQueryData<ObsidianImport>(queryKeys.obsidianImports.detail('workspace-1', 'import-1'))?.status).toBe('queued')
})

test('cancel stores the cancelled import and refreshes pages', async () => {
  const calls = mockFetch(() => Response.json(obsidianImport({ status: 'cancelled' })))
  const { client, wrapper } = setupClient()
  client.setQueryData(queryKeys.pages.tree('workspace-1'), [])
  const view = renderHook(() => useCancelObsidianImport('workspace-1'), { wrapper })
  await act(() => view.result.current.mutateAsync({ importId: 'import-1' }))
  expect(calls[0]).toMatchObject({ method: 'POST', path: `${base}/import-1/cancel` })
  expect(client.getQueryData<ObsidianImport>(queryKeys.obsidianImports.detail('workspace-1', 'import-1'))?.status).toBe('cancelled')
  expect(client.getQueryState(queryKeys.pages.tree('workspace-1'))?.isInvalidated).toBe(true)
})
