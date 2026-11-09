import { beforeAll, describe, expect, mock, test } from 'bun:test'
import { act, fireEvent, render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { createRef } from 'react'
import { NodeSelection } from '@tiptap/pm/state'
import type { Editor } from '@tiptap/core'
import { PageEditor, type PageEditorHandle } from './PageEditor'

beforeAll(() => {
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const DATABASE = { id: 'db-block', type: 'database', props: { databaseId: 'db-1', linkId: '' } }
const CONTENT = [
  { id: 'before', type: 'paragraph', content: [{ type: 'text', text: 'Before', styles: {} }] },
  DATABASE,
  { id: 'after', type: 'paragraph', content: [{ type: 'text', text: 'After', styles: {} }] },
]

async function renderBlock() {
  const ref = createRef<PageEditorHandle>()
  const onChange = mock((content: unknown[]) => void content)
  let view!: ReturnType<typeof render>
  await act(async () => {
    view = render(<PageEditor ref={ref} pageId="host" initialContent={CONTENT} editable onChange={onChange} />)
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
  const block = view.container.querySelector('[data-slot="database-block"]') as HTMLElement
  const tiptap = (view.container.querySelector('.bn-editor') as HTMLElement & { editor: Editor }).editor
  return { view, ref, onChange, block, tiptap }
}

const blockTypes = (ref: { current: PageEditorHandle | null }) => (ref.current!.getContent() as { type: string }[]).map((block) => block.type)

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

describe('database block isolation', () => {
  test('typing in an input inside the block does not change the editor document', async () => {
    const { ref, onChange, block } = await renderBlock()
    const input = block.querySelector('input[aria-label="Prototype input"]') as HTMLInputElement
    await userEvent.type(input, 'Hello{Enter}')
    expect(input.value).toBe('Hello')
    expect(onChange).not.toHaveBeenCalled()
    expect(blockTypes(ref)).toEqual(['paragraph', 'database', 'paragraph'])
  })

  test('Backspace inside the block does not delete it, even while the block is selected', async () => {
    const { ref, onChange, block, tiptap } = await renderBlock()
    selectBlock(tiptap)
    const input = block.querySelector('input[aria-label="Prototype input"]') as HTMLInputElement
    fireEvent.keyDown(input, { key: 'Backspace', code: 'Backspace' })
    fireEvent.keyDown(block.querySelector('li')!, { key: 'Backspace', code: 'Backspace' })
    expect(onChange).not.toHaveBeenCalled()
    expect(blockTypes(ref)).toEqual(['paragraph', 'database', 'paragraph'])

    // From the editor itself, Backspace deletes the selected block.
    fireEvent.keyDown(tiptap.view.dom, { key: 'Backspace', code: 'Backspace' })
    expect(blockTypes(ref)).toEqual(['paragraph', 'paragraph'])
  })

  test('a paste inside the block does not reach the editor', async () => {
    const { ref, onChange, block, tiptap } = await renderBlock()
    selectBlock(tiptap)
    const data: Record<string, string> = { 'text/plain': 'pasted text' }
    const clipboardData = { types: Object.keys(data), getData: (type: string) => data[type] ?? '' }
    fireEvent.paste(block.querySelector('li')!, { clipboardData })
    fireEvent.paste(block.querySelector('input')!, { clipboardData })
    expect(onChange).not.toHaveBeenCalled()
    expect(JSON.stringify(ref.current!.getContent())).not.toContain('pasted text')
  })
})
