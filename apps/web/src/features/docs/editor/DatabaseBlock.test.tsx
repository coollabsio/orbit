import { afterEach, beforeAll, describe, expect, mock, test } from 'bun:test'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createRef, type ReactNode } from 'react'
import { MemoryRouter } from 'react-router'
import { NodeSelection } from '@tiptap/pm/state'
import type { Editor } from '@tiptap/core'
import type { DatabaseDetail, DatabaseRow } from '@orbit/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { testWorkspace } from '@/test/workspace'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { PageEditor, type PageEditorHandle } from './PageEditor'

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

const row = (id: string, position: number, title: string, values: DatabaseRow['values'] = {}): DatabaseRow => ({
  id, title, icon: null, position, version: 1, has_content: false,
  created_at: '2026-11-09T10:00:00Z', created_by: 'u1', updated_at: '2026-11-09T10:00:00Z', updated_by: 'u1', values,
})

const detail = (rows: number): DatabaseDetail => ({
  database: { id: 'db-1', title: 'Reading list', icon: null, locked: false, row_count: rows },
  properties: [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-notes', name: 'Notes', type: 'text', config: {}, position: 1 },
  ],
  views: [{ id: 'v1', name: 'Table', type: 'table', config: {}, position: 0 }],
  rows: Array.from({ length: rows }, (_, index) => row(`r${index}`, index, `Book ${index}`, { 'p-notes': `note ${index}` })),
})

const host = { workspaceId: 'ws', pageId: 'host', locked: false, peekContainer: null }
const workspace = testWorkspace('owner', { id: 'ws' })
const block = (props: { databaseId: string; linkId?: string }, id = 'db-block') => ({ id, type: 'database', props: { linkId: '', ...props } })
const content = (databaseBlock: unknown) => [
  { id: 'before', type: 'paragraph', content: [{ type: 'text', text: 'Before', styles: {} }] },
  ...(Array.isArray(databaseBlock) ? databaseBlock : [databaseBlock]),
  { id: 'after', type: 'paragraph', content: [{ type: 'text', text: 'After', styles: {} }] },
]

/** Answers every request with a 404 problem (members, the trash…); the database comes from the cache. */
function notFoundServer(code = 'page_not_found') {
  globalThis.fetch = (async () =>
    Response.json({ type: 'about:blank', title: 'Not found', status: 404, detail: 'Not found.', code, instance: '/', request_id: 'r' }, { status: 404, headers: { 'content-type': 'application/problem+json' } })) as unknown as typeof fetch
}

/** `peek`: the page opens at `url` with its row-peek pane, and requests stay pending (the peek shows its spinner). */
async function renderBlock(databaseBlock: unknown, cached?: DatabaseDetail, notFoundCode?: string, onDatabaseBlockRemoved?: (databaseId: string) => void, peek?: { url: string; container: HTMLElement }) {
  if (peek) globalThis.fetch = (() => new Promise(() => {})) as unknown as typeof fetch
  else notFoundServer(notFoundCode)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } } })
  if (cached) client.setQueryData(queryKeys.database('ws', 'db-1'), cached)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <MemoryRouter initialEntries={[peek?.url ?? '/']}>
      <QueryClientProvider client={client}>
        <WorkspaceContext.Provider value={{ workspace, workspaces: [workspace], selectWorkspace: () => {} }}>{children}</WorkspaceContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  )
  const ref = createRef<PageEditorHandle>()
  const onChange = mock((value: unknown[]) => void value)
  let view!: ReturnType<typeof render>
  await act(async () => {
    view = render(<PageEditor ref={ref} pageId="host" initialContent={content(databaseBlock)} editable onChange={onChange} databaseHost={{ ...host, peekContainer: peek?.container ?? null }} onDatabaseBlockRemoved={onDatabaseBlockRemoved} />, { wrapper })
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
  const root = () => view.container.querySelector('[data-slot="database-block"]') as HTMLElement
  const tiptap = (view.container.querySelector('.bn-editor') as HTMLElement & { editor: Editor }).editor
  return { view, ref, onChange, root, tiptap }
}

const blockTypes = (ref: { current: PageEditorHandle | null }) => (ref.current!.getContent() as { type: string }[]).map((item) => item.type)

/** Selects the database block as a whole, as a click on its margin or an arrow key from the block above does. */
function selectBlock(tiptap: Editor) {
  let at = -1
  tiptap.state.doc.descendants((node, pos) => {
    if (node.type.name === 'database') at = pos
    return at < 0
  })
  act(() => {
    tiptap.view.dispatch(tiptap.state.tr.setSelection(NodeSelection.create(tiptap.state.doc, at)))
  })
}

describe('database block', () => {
  test('shows the title, the views and the first 25 rows, then 25 more with "Load more"', async () => {
    const { view, root } = await renderBlock(block({ databaseId: 'db-1' }), detail(30))
    expect(root().textContent).toContain('Reading list')
    expect(view.getByRole('tab', { name: 'Table' }).getAttribute('aria-selected')).toBe('true')
    expect(root().querySelectorAll('[role="row"][aria-rowindex]:not([aria-rowindex="1"])').length).toBe(25)
    await userEvent.click(view.getByRole('button', { name: 'Load more' }))
    expect(root().querySelectorAll('[role="row"][aria-rowindex]:not([aria-rowindex="1"])').length).toBe(30)
    expect(view.queryAllByRole('button', { name: 'Load more' }).length).toBe(0)
  })

  test('typing in a cell does not change the editor document', async () => {
    const { ref, onChange, root } = await renderBlock(block({ databaseId: 'db-1' }), detail(2))
    const cell = root().querySelector('[data-cell="r0:p-notes"]') as HTMLElement
    fireEvent.focus(cell)
    fireEvent.keyDown(cell, { key: 'Enter' })
    const input = await waitFor(() => {
      const found = document.querySelector<HTMLInputElement | HTMLTextAreaElement>('[data-slot="popover-content"] input, [data-slot="popover-content"] textarea, [data-cell="r0:p-notes"] input, [data-cell="r0:p-notes"] textarea')
      if (!found) throw new Error('no cell editor')
      return found
    })
    await userEvent.type(input, ' more')
    expect(input.value).toContain('more')
    expect(onChange).not.toHaveBeenCalled()
    expect(blockTypes(ref)).toEqual(['paragraph', 'database', 'paragraph'])
  })

  test('Backspace in a cell does not delete the block, even while the block is selected', async () => {
    const { ref, onChange, root, tiptap } = await renderBlock(block({ databaseId: 'db-1' }), detail(2))
    selectBlock(tiptap)
    fireEvent.keyDown(root().querySelector('[data-cell="r0:p-notes"]')!, { key: 'Backspace', code: 'Backspace' })
    expect(onChange).not.toHaveBeenCalled()
    expect(blockTypes(ref)).toEqual(['paragraph', 'database', 'paragraph'])

    // From the editor itself, Backspace deletes the selected block.
    fireEvent.keyDown(tiptap.view.dom, { key: 'Backspace', code: 'Backspace' })
    expect(blockTypes(ref)).toEqual(['paragraph', 'paragraph'])
  })

  test('deleting an inline database block reports its database (the toast offers to trash it)', async () => {
    const removed = mock((databaseId: string) => void databaseId)
    const { tiptap } = await renderBlock(block({ databaseId: 'db-1' }), detail(1), undefined, removed)
    selectBlock(tiptap)
    fireEvent.keyDown(tiptap.view.dom, { key: 'Backspace', code: 'Backspace' })
    expect(removed.mock.calls).toEqual([['db-1']])
  })

  test('deleting a linked view block reports nothing: its database lives elsewhere', async () => {
    const removed = mock((databaseId: string) => void databaseId)
    const { tiptap } = await renderBlock(block({ databaseId: 'db-1', linkId: 'l1' }), detail(1), undefined, removed)
    selectBlock(tiptap)
    fireEvent.keyDown(tiptap.view.dom, { key: 'Backspace', code: 'Backspace' })
    expect(removed).not.toHaveBeenCalled()
  })

  test('a paste inside the block does not reach the editor', async () => {
    const { ref, onChange, root, tiptap } = await renderBlock(block({ databaseId: 'db-1' }), detail(2))
    selectBlock(tiptap)
    const data: Record<string, string> = { 'text/plain': 'pasted text' }
    fireEvent.paste(root().querySelector('[data-cell="r0:p-notes"]')!, { clipboardData: { types: Object.keys(data), getData: (type: string) => data[type] ?? '' } })
    expect(onChange).not.toHaveBeenCalled()
    expect(JSON.stringify(ref.current!.getContent())).not.toContain('pasted text')
  })

  test('a database the viewer cannot read shows "No access to this database" and no title', async () => {
    const { root } = await renderBlock(block({ databaseId: 'db-1' }))
    await waitFor(() => {
      if (!root().textContent?.includes('No access to this database')) throw new Error(root().textContent ?? '')
    })
    expect(root().textContent).toBe('No access to this database')
  })

  test('a block whose link is gone offers the database\'s own views', async () => {
    const { view, root, ref } = await renderBlock(block({ databaseId: 'db-1', linkId: 'gone' }), detail(1), 'database_link_not_found')
    await waitFor(() => {
      if (!root().textContent?.includes('View not found')) throw new Error(root().textContent ?? '')
    })
    await userEvent.click(view.getByRole('button', { name: "Show the database's own views" }))
    const stored = ref.current!.getContent() as { type: string; props?: { linkId?: string } }[]
    expect(stored[1].props?.linkId).toBe('')
  })

  test('a row opened from a block shows in the page pane, outside the editor, and only for that block', async () => {
    const pane = document.createElement('div')
    document.body.append(pane)
    const { view } = await renderBlock([block({ databaseId: 'db-1' }), block({ databaseId: 'db-1' }, 'db-block-2')], detail(2), undefined, undefined, {
      url: '/docs/host?row=r1&row_block=db-block-2',
      container: pane,
    })
    expect(view.container.querySelectorAll('[data-slot="database-block"]').length).toBe(2)
    expect(view.container.querySelectorAll('[data-row-peek]').length).toBe(0)
    expect([...pane.querySelectorAll('[data-row-peek]')].map((peek) => peek.getAttribute('data-row-peek'))).toEqual(['r1'])
    pane.remove()
  })
})
