// Page options in the docs editor: full width, lock (read-only rendering, live lock over the collab socket), the
// "Edited … by …" header label, and the recent-page visit.
import { afterEach, beforeAll, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { Awareness, applyAwarenessUpdate, encodeAwarenessUpdate } from 'y-protocols/awareness'
import * as Y from 'yjs'
import type { Page, PageSummary, Teamspace } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { CollabConnectContext } from '@/features/docs/collab/connection'
import { lastEditedLabel, relativeEditTime } from '@/features/docs/lastEdited'
import { fakeCollab, type FakeCollab } from '@/test/fakeCollab'
import { waitForAbsence } from '@/test/waitForAbsence'
import { DocsPage } from './DocsPage'

beforeAll(() => {
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const originalFetch = globalThis.fetch
afterEach(() => {
  globalThis.fetch = originalFetch
})

const MINUTE = 60_000
const summary = (id: string, title = id): PageSummary => ({
  id, parent_id: null, teamspace_id: 'teamspace-1', private: false, position: 0, title, icon: null, version: 1,
  updated_at: '2026-09-25T10:00:00Z',
})
const teamspaces: Teamspace[] = [{
  id: 'teamspace-1', workspace_id: 'workspace-1', name: 'General', icon: null, position: 0, version: 1, is_default: true,
  created_at: '2026-09-25T10:00:00Z', updated_at: '2026-09-25T10:00:00Z',
}]
const mia = { id: 'user-2', display_name: 'Mia Member' }
const page = (patch: Partial<Page> = {}): Page => ({
  ...summary('first', 'Plan'), workspace_id: 'workspace-1', cover_url: null, cover_position: null,
  content: [{ id: 'b1', type: 'paragraph', props: {}, content: [{ type: 'text', text: 'Body text', styles: {} }], children: [] }],
  creator_id: 'user-1', updated_by: 'user-2', created_at: '2026-09-25T10:00:00Z', deleted_at: null, collab_epoch: 'epoch-1',
  updated_at: new Date(Date.now() - 5 * MINUTE).toISOString(),
  full_width: false, locked_at: null, locked_by: null, updated_by_user: mia,
  ...patch,
})

type Call = { method: string; path: string; body: Record<string, unknown> | undefined }

function setup(pageRef: { current: Page }, extra: (call: Call) => Response | undefined = () => undefined, collab: FakeCollab = fakeCollab({ content: () => pageRef.current.content })) {
  window.localStorage.clear()
  const calls: Call[] = []
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    const request = input as Request
    const url = new URL(request.url)
    const text = await request.text()
    const call: Call = { method: request.method, path: url.pathname.replace('/api/v1/workspaces/workspace-1', ''), body: text ? JSON.parse(text) : undefined }
    calls.push(call)
    const handled = extra(call)
    if (handled) return handled
    if (call.path === '/api/v1/auth/me') return Response.json({ id: 'user-1', display_name: 'Ada Owner', email: 'ada@example.com' })
    if (call.path === '/teamspaces') return Response.json({ items: teamspaces })
    if (call.path === '/pages') return Response.json({ items: [summary('first', 'Plan')] })
    if (call.path === '/pages/first' && call.method === 'GET') return Response.json(pageRef.current)
    if (call.path === '/pages/first' && call.method === 'PATCH') {
      pageRef.current = { ...pageRef.current, ...(call.body as Partial<Page>), version: pageRef.current.version + 1 }
      return Response.json(pageRef.current)
    }
    if (call.path === '/pages/first/lock') {
      const locked = call.body?.locked === true
      pageRef.current = {
        ...pageRef.current,
        locked_at: locked ? new Date().toISOString() : null,
        locked_by: locked ? { id: 'user-1', display_name: 'Ada Owner' } : null,
        version: pageRef.current.version + 1,
      }
      return Response.json(pageRef.current)
    }
    if (call.path === '/pages/first/visit') return new Response(null, { status: 204 })
    return Response.json({ code: 'not_found' }, { status: 404 })
  }) as unknown as typeof fetch
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  client.setQueryData(queryKeys.workspaces, [{ id: 'workspace-1', name: 'Alpha', role: 'member', version: 1 }])
  const view = render(
    <QueryClientProvider client={client}>
      <CollabConnectContext value={collab.connect}>
        <MemoryRouter initialEntries={['/docs/first?workspace=workspace-1']}>
          <WorkspaceProvider>
            <Routes>
              <Route path="docs/:pageId?" element={<DocsPage />} />
            </Routes>
          </WorkspaceProvider>
        </MemoryRouter>
      </CollabConnectContext>
    </QueryClientProvider>,
  )
  return { view, calls, collab }
}

async function editorEditable(view: ReturnType<typeof render>, editable: boolean) {
  await waitFor(() => {
    const value = view.container.querySelector('.bn-editor')?.getAttribute('contenteditable')
    if (value !== String(editable)) throw new Error(`contenteditable is ${value}`)
  }, { timeout: 4000 })
}

async function openMenu(view: ReturnType<typeof render>) {
  const section = view.container.querySelector('[data-realtime-safe]') as HTMLElement
  fireEvent.click(within(section).getByRole('button', { name: 'Page options' }))
}

test('relative edit times and the "by you" wording', () => {
  const now = Date.parse('2026-09-25T12:00:00Z')
  expect(relativeEditTime(now - 20_000, now)).toBe('just now')
  expect(relativeEditTime(now - MINUTE, now)).toBe('1 minute ago')
  expect(relativeEditTime(now - 5 * MINUTE, now)).toBe('5 minutes ago')
  expect(relativeEditTime(now - 3 * 60 * MINUTE, now)).toBe('3 hours ago')
  expect(relativeEditTime(now - 2 * 24 * 60 * MINUTE, now)).toBe('2 days ago')
  expect(relativeEditTime(now - 30 * 24 * 60 * MINUTE, now)).toMatch(/^on /)
  expect(lastEditedLabel({ userId: 'user-1', name: 'Ada', at: now }, 'user-1', now)).toBe('Edited just now by you')
  expect(lastEditedLabel({ userId: 'user-2', name: 'Mia', at: now - 5 * MINUTE }, 'user-1', now)).toBe('Edited 5 minutes ago by Mia')
})

test('the Full width toggle saves with expected_version and widens the column', async () => {
  const ref = { current: page() }
  const { view, calls } = setup(ref)
  await editorEditable(view, true)
  expect(view.container.querySelector('[data-full-width]')).toBeNull()
  await openMenu(view)
  const toggle = await view.findByRole('menuitemcheckbox', { name: 'Full width' })
  expect(toggle.getAttribute('aria-checked')).toBe('false')
  await userEvent.click(toggle)
  await waitFor(() => expect(calls.some((call) => call.method === 'PATCH')).toBeTrue())
  expect(calls.find((call) => call.method === 'PATCH')?.body).toEqual({ full_width: true, expected_version: 1 })
  await waitFor(() => expect(view.container.querySelector('[data-full-width]')).toBeTruthy())
  // The label still names the last editor: layout is not an edit.
  expect(view.container.querySelector('[data-last-edited]')?.textContent).toBe('Edited 5 minutes ago by Mia Member')
})

test('Lock page locks it: read-only editor, title and icon, a badge with Unlock', async () => {
  const ref = { current: page({ icon: '🔥' }) }
  const { view, calls, collab } = setup(ref)
  await editorEditable(view, true)
  expect(view.getByRole('button', { name: /Add cover/ })).toBeTruthy()
  await openMenu(view)
  await userEvent.click(await view.findByRole('menuitem', { name: 'Lock page' }))
  await waitFor(() => expect(calls.find((call) => call.path === '/pages/first/lock')?.body).toEqual({ locked: true }))
  // The server closes the socket with 4423 after a lock change.
  act(() => collab.last().provider.drop(4423))
  await editorEditable(view, false)
  const badge = await waitFor(() => view.container.querySelector('[data-lock-badge]') as HTMLElement)
  expect(badge.textContent).toContain('Locked by you')
  expect((view.getByLabelText('Page title') as HTMLInputElement).readOnly).toBeTrue()
  expect(view.getByRole('button', { name: 'Change icon' }).hasAttribute('disabled')).toBeTrue()
  expect(view.queryByRole('button', { name: /Add cover/ })).toBeNull()
  // A fresh document after the reconnect.
  expect(collab.connections.length).toBe(2)

  fireEvent.click(within(badge).getByRole('button', { name: 'Unlock' }))
  await waitFor(() => expect(calls.filter((call) => call.path === '/pages/first/lock').at(-1)?.body).toEqual({ locked: false }))
  act(() => collab.last().provider.drop(4423))
  await editorEditable(view, true)
  await waitForAbsence(() => view.container.querySelector('[data-lock-badge]'))
  expect((view.getByLabelText('Page title') as HTMLInputElement).readOnly).toBeFalse()
})

test('a page locked by someone else opens read-only; the lock arriving live turns an open editor read-only', async () => {
  const ref = { current: page() }
  const { view, collab } = setup(ref)
  await editorEditable(view, true)
  // Mia locks the page elsewhere: the socket closes with 4423 and the refetched page carries the lock.
  ref.current = { ...ref.current, locked_at: new Date().toISOString(), locked_by: mia, version: 2 }
  act(() => collab.last().provider.drop(4423))
  await editorEditable(view, false)
  await waitFor(() => expect(view.container.querySelector('[data-lock-badge]')?.textContent).toContain('Locked by Mia Member'))
  await openMenu(view)
  expect(await view.findByRole('menuitem', { name: 'Unlock page' })).toBeTruthy()
})

test('"Edited … by …" names the last editor and follows live edits of collaborators', async () => {
  const ref = { current: page() }
  const { view, collab } = setup(ref)
  await editorEditable(view, true)
  const label = () => view.container.querySelector('[data-last-edited]')?.textContent
  expect(label()).toBe('Edited 5 minutes ago by Mia Member')

  // Someone else types: attributed through their awareness user.
  const { doc, provider } = collab.last()
  const peerDoc = new Y.Doc()
  Y.applyUpdate(peerDoc, Y.encodeStateAsUpdate(doc))
  const peer = new Awareness(peerDoc)
  peer.setLocalState({ user: { id: 'user-3', name: 'Sam Peer', color: '#2563eb' } })
  applyAwarenessUpdate(provider.awareness, encodeAwarenessUpdate(peer, [peerDoc.clientID]), 'remote')
  const before = Y.encodeStateVector(doc)
  const firstText = (node: Y.XmlFragment | Y.XmlElement): Y.XmlText | null => {
    for (const child of node.toArray()) {
      if (child instanceof Y.XmlText) return child
      const found = child instanceof Y.XmlElement ? firstText(child) : null
      if (found) return found
    }
    return null
  }
  firstText(peerDoc.getXmlFragment('prosemirror'))!.insert(0, 'x')
  act(() => Y.applyUpdate(doc, Y.encodeStateAsUpdate(peerDoc, before), provider))
  await waitFor(() => expect(label()).toBe('Edited just now by Sam Peer'))
  peer.destroy()
  peerDoc.destroy()

  // A page refetch with our own newer edit reads "by you".
  ref.current = { ...ref.current, updated_by_user: { id: 'user-1', display_name: 'Ada Owner' }, updated_by: 'user-1', updated_at: new Date(Date.now() + 1000).toISOString() }
  act(() => collab.last().provider.drop(4409))
  await waitFor(() => expect(label()).toBe('Edited just now by you'))
})

test('opening a page records one visit after a short dwell', async () => {
  const ref = { current: page() }
  const { view, calls } = setup(ref)
  await editorEditable(view, true)
  await waitFor(() => expect(calls.filter((call) => call.path === '/pages/first/visit')).toHaveLength(1), { timeout: 3000 })
  expect(calls.find((call) => call.path === '/pages/first/visit')?.method).toBe('POST')
})
