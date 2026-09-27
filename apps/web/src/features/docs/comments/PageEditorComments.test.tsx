// Thread store wiring: collaborative editors load the comment mark (anchors survive), marks of known threads are
// live highlights, unknown/resolved ones turn orphan, and the Comments panel lists the page's threads.
import { beforeAll, describe, expect, test } from 'bun:test'
import { act, fireEvent, render, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { BlockNoteEditor } from '@blocknote/core'
import { CommentsExtension } from '@blocknote/core/comments'
import { _blocksToProsemirrorNode } from '@blocknote/core/yjs'
import { Transform } from '@tiptap/pm/transform'
import { prosemirrorToYXmlFragment } from 'y-prosemirror'
import * as Y from 'yjs'
import type { PageThread } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { COLLAB_FRAGMENT } from '@/features/docs/collab/connection'
import { PageEditor, type PageEditorComments } from '@/features/docs/editor/PageEditor'
import { pageEditorSchema } from '@/features/docs/editor/schema'
import { FakeProvider } from '@/test/fakeCollab'
import { NoCommentsThreadStore } from './threadStore'

beforeAll(() => {
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const LIVE = '0199a0b0-0000-7000-8000-0000000000a1'
const GONE = '0199a0b0-0000-7000-8000-0000000000a2'
/** A thread whose anchor is not in the document (its text was removed). */
const LOST = '0199a0b0-0000-7000-8000-0000000000a3'

/** A synced document whose paragraph carries two comment marks (as another editor would have written them). */
function markedDoc() {
  const headless = BlockNoteEditor.create({
    schema: pageEditorSchema,
    extensions: [CommentsExtension({ threadStore: new NoCommentsThreadStore(), resolveUsers: async () => [] })],
    _tiptapOptions: { injectCSS: false },
  })
  const node = _blocksToProsemirrorNode(headless, [{ id: 'p', type: 'paragraph', content: 'Anchored words and more' }] as never)
  const mark = (threadId: string) => node.type.schema.marks.comment.create({ threadId, orphan: false })
  const marked = new Transform(node).addMark(3, 11, mark(LIVE)).addMark(16, 19, mark(GONE)).doc
  const doc = new Y.Doc()
  prosemirrorToYXmlFragment(marked, doc.getXmlFragment(COLLAB_FRAGMENT))
  return doc
}

function thread(overrides: Partial<PageThread> = {}): PageThread {
  return {
    ...threadBase(),
    ...overrides,
  }
}

function threadBase(): PageThread {
  return {
    id: LIVE,
    page_id: 'page-1',
    created_by: 'ann',
    quote: 'Anchored',
    resolved: false,
    resolved_at: null,
    resolved_by: null,
    version: 0,
    created_at: '2026-09-25T10:00:00Z',
    updated_at: '2026-09-25T10:00:00Z',
    comments: [
      {
        id: 'c1',
        thread_id: LIVE,
        author_id: 'ann',
        body: [{ type: 'paragraph', content: [{ type: 'text', text: 'Please check', styles: {} }, { type: 'mention', props: { userId: 'bob', name: 'Bobby' } }] }],
        body_text: 'Please check @Bobby',
        mentioned_user_ids: ['bob'],
        created_at: '2026-09-25T10:00:00Z',
        updated_at: '2026-09-25T10:00:00Z',
        edited_at: null,
        deleted_at: null,
      },
    ],
  }
}

async function renderCollab(comments?: Partial<PageEditorComments>, withClient = true, threads: PageThread[] = [thread()]) {
  const doc = markedDoc()
  const provider = new FakeProvider(doc)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } })
  client.setQueryData(queryKeys.pages.threads('w1', 'page-1'), threads)
  const config: PageEditorComments | undefined = comments
    ? {
        workspaceId: 'w1',
        pageId: 'page-1',
        userId: 'ann',
        members: [
          { id: 'ann', name: 'Ann', color: '#e11d48' },
          { id: 'bob', name: 'Bob Stone' },
        ],
        mentionable: [{ id: 'bob', name: 'Bob Stone' }],
        panel: null,
        ...comments,
      }
    : undefined
  const editor = (
    <PageEditor
      pageId="page-1"
      collab={{ provider, fragment: doc.getXmlFragment(COLLAB_FRAGMENT), user: { name: 'Ann', color: '#f00' } }}
      editable
      resolvePage={() => null}
      onOpenPage={() => {}}
      onCreateSubpage={async () => 'x'}
      onPickPage={async () => null}
      comments={config}
    />
  )
  let view!: ReturnType<typeof render>
  await act(async () => {
    view = render(withClient ? <QueryClientProvider client={client}>{editor}</QueryClientProvider> : editor)
    await new Promise((resolve) => setTimeout(resolve, 20))
  })
  return { view, doc }
}

describe('comments in the collaborative editor', () => {
  test('waits for the threads before mounting (an empty store would orphan every anchor)', async () => {
    const doc = markedDoc()
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    let release: (threads: PageThread[]) => void = () => {}
    const pending = new Promise<PageThread[]>((resolve) => {
      release = resolve
    })
    client.setQueryDefaults(queryKeys.pages.threads('w1', 'page-1'), { queryFn: () => pending })
    const originalFetch = globalThis.fetch
    globalThis.fetch = (async () => {
      const threads = await pending
      return Response.json({ items: threads })
    }) as unknown as typeof fetch
    let view!: ReturnType<typeof render>
    await act(async () => {
      view = render(
        <QueryClientProvider client={client}>
          <PageEditor
            pageId="page-1"
            collab={{ provider: new FakeProvider(doc), fragment: doc.getXmlFragment(COLLAB_FRAGMENT), user: { name: 'Ann', color: '#f00' } }}
            editable
            resolvePage={() => null}
            onOpenPage={() => {}}
            onCreateSubpage={async () => 'x'}
            onPickPage={async () => null}
            comments={{ workspaceId: 'w1', pageId: 'page-1', userId: 'ann', members: [], mentionable: [], panel: null }}
          />
        </QueryClientProvider>,
      )
      await new Promise((resolve) => setTimeout(resolve, 20))
    })
    expect(view.getByTestId('editor-loading')).toBeTruthy()
    expect(doc.getXmlFragment(COLLAB_FRAGMENT).toJSON()).not.toContain('orphan="true"')
    await act(async () => {
      release([thread()])
      await new Promise((resolve) => setTimeout(resolve, 50))
    })
    await waitFor(() => expect(view.container.querySelector(`.bn-thread-mark[data-bn-thread-id="${LIVE}"]`)).toBeTruthy())
    // Only the mark without a thread changed.
    expect(doc.getXmlFragment(COLLAB_FRAGMENT).toJSON().match(/orphan="true"/g)).toHaveLength(1)
    globalThis.fetch = originalFetch
    view.unmount()
  })

  test('without a comments backend the marked text still loads (the mark is always in the schema)', async () => {
    const { view, doc } = await renderCollab(undefined, false)
    await waitFor(() => expect(view.container.textContent).toContain('Anchored words and more'))
    expect(doc.getXmlFragment(COLLAB_FRAGMENT).toJSON()).toContain('Anchored</comment--')
  })

  test('marks of known threads highlight; marks without a thread turn orphan', async () => {
    const { view } = await renderCollab({})
    await waitFor(() => {
      const live = view.container.querySelector(`.bn-thread-mark[data-bn-thread-id="${LIVE}"]`)
      expect(live).toBeTruthy()
      expect(live!.getAttribute('data-orphan')).toBeNull()
    })
    await waitFor(() => {
      const gone = view.container.querySelector(`.bn-thread-mark[data-bn-thread-id="${GONE}"]`)
      expect(gone?.getAttribute('data-orphan')).toBe('true')
    })
  })

  test('the Comments panel lists open threads with authors and mentions', async () => {
    const container = document.createElement('div')
    document.body.append(container)
    const { view } = await renderCollab({ panel: { container, filter: 'open' } })
    await waitFor(() => expect(container.textContent).toContain('Please check'))
    expect(container.querySelector('.orbit-mention')?.textContent).toBe('@Bob Stone')
    await waitFor(() => expect(container.textContent).toContain('Ann'))
    view.unmount()
    container.remove()
  })

  test('the resolved filter hides open threads', async () => {
    const container = document.createElement('div')
    document.body.append(container)
    const { view } = await renderCollab({ panel: { container, filter: 'resolved' } })
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(container.textContent).not.toContain('Please check')
    view.unmount()
    container.remove()
  })
  test('panel cards: quote on top, author row, own-comment actions, resolve button and a collapsed reply field', async () => {
    const container = document.createElement('div')
    document.body.append(container)
    const reply = { ...threadBase().comments[0], id: 'c2', author_id: 'bob', body: [{ type: 'paragraph', content: [{ type: 'text', text: 'Done', styles: {} }] }], body_text: 'Done', mentioned_user_ids: [] }
    const { view } = await renderCollab({ panel: { container, filter: 'open' } }, true, [thread({ comments: [threadBase().comments[0], reply] })])
    await waitFor(() => expect(container.textContent).toContain('Done'))
    const card = container.querySelector('[data-thread-card]')!
    expect(card.querySelector('[data-thread-quote]')?.textContent).toBe('Anchored')
    expect([...card.querySelectorAll('[data-comment-author]')].map((node) => node.textContent)).toEqual(['Ann', 'Bob Stone'])
    // Only the signed-in author's comment offers Edit/Delete.
    expect(card.querySelectorAll('button[aria-label="Comment actions"]')).toHaveLength(1)
    expect(card.querySelector('button[aria-label="Resolve"]')).toBeTruthy()
    expect(card.querySelector('[data-reply-trigger]')?.textContent).toBe('Reply…')
    expect(card.hasAttribute('data-orphaned')).toBe(false)
    view.unmount()
    container.remove()
  })

  test('clicking a panel card selects its thread and highlights the anchor', async () => {
    const container = document.createElement('div')
    document.body.append(container)
    const { view } = await renderCollab({ panel: { container, filter: 'open' } })
    await waitFor(() => expect(container.querySelector('[data-thread-card]')).toBeTruthy())
    await act(async () => {
      fireEvent.click(container.querySelector('[data-comment-author]')!)
    })
    await waitFor(() => expect(view.container.querySelector('.bn-thread-mark-selected')?.textContent).toBe('Anchored'))
    expect(container.querySelector('[data-thread-card]')?.hasAttribute('data-selected')).toBe(true)
    view.unmount()
    container.remove()
  })

  test('a thread whose text is gone is labelled "Text removed"', async () => {
    const container = document.createElement('div')
    document.body.append(container)
    const lost = thread({ id: LOST, quote: 'Old words', comments: [{ ...threadBase().comments[0], id: 'c9', thread_id: LOST }] })
    const { view } = await renderCollab({ panel: { container, filter: 'open' } }, true, [thread(), lost])
    await waitFor(() => expect(container.querySelectorAll('[data-thread-card]')).toHaveLength(2))
    const cards = [...container.querySelectorAll('[data-thread-card]')]
    // Document order first, anchorless threads last.
    expect(cards.map((card) => card.getAttribute('data-thread-id'))).toEqual([LIVE, LOST])
    expect(cards[1].hasAttribute('data-orphaned')).toBe(true)
    expect(cards[1].querySelector('[data-thread-orphaned]')?.textContent).toBe('Text removed')
    expect(cards[1].querySelector('[data-thread-quote]')?.textContent).toBe('Old words')
    expect(cards[0].querySelector('[data-thread-orphaned]')).toBeNull()
    view.unmount()
    container.remove()
  })

  test('resolved threads say who resolved them, offer Reopen and no reply field', async () => {
    const container = document.createElement('div')
    document.body.append(container)
    const resolved = thread({ resolved: true, resolved_by: 'bob', resolved_at: '2026-09-25T11:00:00Z' })
    const { view } = await renderCollab({ panel: { container, filter: 'resolved' } }, true, [resolved])
    await waitFor(() => expect(container.querySelector('[data-thread-resolved]')).toBeTruthy())
    const card = container.querySelector('[data-thread-card]')!
    expect(card.hasAttribute('data-resolved')).toBe(true)
    expect(card.querySelector('[data-thread-resolved]')?.textContent).toContain('Resolved by Bob Stone')
    expect(card.querySelector('button[aria-label="Reopen"]')).toBeTruthy()
    expect(card.querySelector('[data-reply-trigger]')).toBeNull()
    view.unmount()
    container.remove()
  })
})
