import { beforeAll, describe, expect, mock, test } from 'bun:test'
import { act, fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { createRef } from 'react'
import { PageEditor, type PageEditorHandle, type PageEditorProps, type PageRef } from './PageEditor'

beforeAll(() => {
  // BlockNote warns about mobile keyboards without this viewport flag; keep test output clean.
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const FILE_URL =
  '/api/v1/workspaces/01a0d778-64cb-7422-a4df-c7eae799677f/pages/01a0d779-0000-7000-8000-000000000001/files/01a0d779-0000-7000-8000-000000000002'

const pages: Record<string, PageRef> = {
  live: { title: 'Roadmap', icon: null, trashed: false },
  binned: { title: 'Old notes', icon: null, trashed: true },
}

async function renderEditor(overrides: Partial<PageEditorProps> = {}) {
  const ref = createRef<PageEditorHandle>()
  const props: PageEditorProps = {
    pageId: 'page-1',
    initialContent: [],
    editable: true,
    onChange: mock(() => {}),
    resolvePage: (id) => pages[id] ?? null,
    onOpenPage: mock(() => {}),
    onCreateSubpage: async () => 'new-page',
    onPickPage: async () => null,
    ...overrides,
  }
  let view!: ReturnType<typeof render>
  await act(async () => {
    view = render(<PageEditor ref={ref} {...props} />)
    // Let BlockNote's deferred UI state (toolbars, suggestion menu registration) settle inside act.
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
  return { view, ref, props }
}

describe('PageEditor', () => {
  test('starts from empty content with a single empty paragraph and no change event', async () => {
    const { ref, props } = await renderEditor()
    const content = ref.current!.getContent() as { type: string; content: unknown[] }[]
    expect(content).toHaveLength(1)
    expect(content[0].type).toBe('paragraph')
    expect(content[0].content).toEqual([])
    expect(props.onChange).not.toHaveBeenCalled()
  })

  test('renders page blocks from resolvePage: live pages open, trashed and missing ones are disabled', async () => {
    const { view, props } = await renderEditor({
      initialContent: [
        { id: 'a', type: 'page', props: { pageId: 'live' } },
        { id: 'b', type: 'page', props: { pageId: 'binned' } },
        { id: 'c', type: 'page', props: { pageId: 'nope' } },
      ],
    })

    fireEvent.click(await view.findByText('Roadmap'))
    expect(props.onOpenPage).toHaveBeenCalledWith('live')

    expect(view.getByText('Page in trash')).toBeTruthy()
    fireEvent.click(view.getByText('Old notes'))
    fireEvent.click(view.getByText('Missing page'))
    expect(props.onOpenPage).toHaveBeenCalledTimes(1)

    const states = [...view.container.querySelectorAll('button[data-page-id]')].map((el) => el.getAttribute('data-state'))
    expect(states).toEqual(['ok', 'trashed', 'missing'])
  })

  test('strips disallowed links and unknown block types from loaded content', async () => {
    const { ref, view } = await renderEditor({
      initialContent: [
        { id: 'a', type: 'video', props: { url: 'https://x/y.mp4' } },
        {
          id: 'b',
          type: 'paragraph',
          content: [
            { type: 'link', href: 'javascript:alert(1)', content: [{ type: 'text', text: 'bad', styles: {} }] },
            { type: 'link', href: 'https://ok.example', content: [{ type: 'text', text: 'good', styles: {} }] },
          ],
        },
      ],
    })
    expect(JSON.stringify(ref.current!.getContent())).not.toContain('javascript:')
    const hrefs = [...view.container.querySelectorAll('a')].map((a) => a.getAttribute('href'))
    expect(hrefs).toEqual(['https://ok.example'])
    expect(view.container.querySelector('[data-content-type="video"]')).toBeNull()
  })

  test('keeps /docs/<uuid> page links and opens them in the app on click', async () => {
    const pageId = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b'
    const { ref, view, props } = await renderEditor({
      initialContent: [
        {
          id: 'b',
          type: 'paragraph',
          content: [
            { type: 'link', href: `/docs/${pageId}`, content: [{ type: 'text', text: 'Other page', styles: {} }] },
            { type: 'link', href: '/docs/not-a-uuid', content: [{ type: 'text', text: 'broken', styles: {} }] },
            { type: 'link', href: 'data:text/html,x', content: [{ type: 'text', text: 'data', styles: {} }] },
          ],
        },
      ],
    })
    const hrefs = [...view.container.querySelectorAll('a')].map((a) => a.getAttribute('href'))
    expect(hrefs).toEqual([`/docs/${pageId}`])
    expect(JSON.stringify(ref.current!.getContent())).toContain(`"href":"/docs/${pageId}"`)
    const link = view.getByText('Other page').closest('a')!
    const click = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 })
    act(() => {
      link.dispatchEvent(click)
    })
    expect(click.defaultPrevented).toBe(true)
    expect(props.onOpenPage).toHaveBeenCalledWith(pageId)
    // Cmd/Ctrl-click keeps the browser's new-tab behaviour (the listener below only stops happy-dom from navigating).
    const stop = (event: Event) => event.preventDefault()
    document.addEventListener('click', stop)
    const modified = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, metaKey: true })
    act(() => {
      link.dispatchEvent(modified)
    })
    document.removeEventListener('click', stop)
    expect(props.onOpenPage).toHaveBeenCalledTimes(1)
  })

  test('renders uploaded images from their page file URL and blanks unsafe media URLs', async () => {
    const { view, ref } = await renderEditor({
      initialContent: [
        { id: 'a', type: 'image', props: { url: FILE_URL, name: 'photo.png' } },
        { id: 'b', type: 'file', props: { url: 'javascript:alert(1)', name: 'evil.pdf' } },
      ],
    })
    // The media views settle their size/loading state after mount.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20))
    })
    const img = view.container.querySelector('[data-content-type="image"] img')
    expect(img?.getAttribute('src')).toBe(FILE_URL)
    const stored = ref.current!.getContent() as { type: string; props: { url: string } }[]
    expect(stored.map((block) => [block.type, block.props.url])).toEqual([
      ['image', FILE_URL],
      ['file', ''],
    ])
  })

  test('pasted image files go through uploadFile and land as an image block with the returned URL', async () => {
    const uploadFile = mock(async (file: File) => {
      void file
      return FILE_URL
    })
    const onChange = mock((content: unknown[]) => void content)
    const { ref } = await renderEditor({ uploadFile, onChange })
    await act(async () => ref.current!.focus())
    const editorEl = document.querySelector('.bn-editor') as HTMLElement
    const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], 'shot.png', { type: 'image/png' })
    await act(async () => {
      fireEvent.paste(editorEl, {
        clipboardData: {
          types: ['Files'],
          files: [file],
          items: [{ kind: 'file', type: 'image/png', getAsFile: () => file }],
          getData: () => '',
        },
      })
      await new Promise((resolve) => setTimeout(resolve, 20))
    })
    expect(uploadFile).toHaveBeenCalledTimes(1)
    expect(uploadFile.mock.calls[0][0].name).toBe('shot.png')
    const images = (ref.current!.getContent() as { type: string; props: { url: string } }[]).filter((block) => block.type === 'image')
    expect(images.map((block) => block.props.url)).toEqual([FILE_URL])
  })

  test('reports user edits through onChange with the full, sanitized document', async () => {
    const onChange = mock((content: unknown[]) => void content)
    const { ref } = await renderEditor({ onChange })
    await act(async () => ref.current!.focus())
    const editorEl = document.querySelector('.bn-editor') as HTMLElement
    const data: Record<string, string> = { 'text/plain': 'pasted text' }
    await act(async () => {
      fireEvent.paste(editorEl, { clipboardData: { types: Object.keys(data), getData: (type: string) => data[type] ?? '' } })
    })
    expect(onChange).toHaveBeenCalled()
    const last = onChange.mock.calls.at(-1)![0]
    expect(JSON.stringify(last)).toContain('pasted text')
    expect(last).toEqual(ref.current!.getContent())
  })

  test('recreates the editor when pageId changes', async () => {
    const { view, ref, props } = await renderEditor({
      initialContent: [{ id: 'a', type: 'paragraph', content: 'first page' }],
    })
    expect(await view.findByText('first page')).toBeTruthy()
    await act(async () => {
      view.rerender(<PageEditor ref={ref} {...props} pageId="page-2" initialContent={[{ id: 'b', type: 'paragraph', content: 'second page' }]} />)
    })
    expect(await view.findByText('second page')).toBeTruthy()
    expect(view.queryByText('first page')).toBeNull()
  })

  test('callout: renders the emoji and text; clicking the emoji opens the picker and a pick updates the block', async () => {
    const onChange = mock((content: unknown[]) => void content)
    const { view, ref } = await renderEditor({
      onChange,
      initialContent: [{ id: 'c', type: 'callout', props: { emoji: '⭐', backgroundColor: 'blue' }, content: 'Heads up' }],
    })
    expect(await view.findByText('Heads up')).toBeTruthy()
    const trigger = view.getByRole('button', { name: 'Change callout icon' })
    expect(trigger.textContent).toBe('⭐')
    expect(trigger.closest('[data-content-type="callout"]')?.getAttribute('data-background-color')).toBe('blue')

    fireEvent.click(trigger)
    expect(trigger.getAttribute('aria-expanded')).toBe('true')
    await userEvent.click(await view.findByTitle('grinning face'))

    const [callout] = ref.current!.getContent() as { type: string; props: Record<string, string> }[]
    expect(callout.type).toBe('callout')
    expect(callout.props).toEqual({ emoji: '😀', backgroundColor: 'blue', textColor: 'default' })
    expect(onChange).toHaveBeenCalled()
    expect(view.getByRole('button', { name: 'Change callout icon' }).textContent).toBe('😀')
  })

  test('read-only mode renders a non-editable document', async () => {
    const { view } = await renderEditor({ editable: false, initialContent: [{ id: 'a', type: 'paragraph', content: 'locked' }] })
    expect(await view.findByText('locked')).toBeTruthy()
    expect(view.container.querySelector('.bn-editor')?.getAttribute('contenteditable')).toBe('false')
  })
})
