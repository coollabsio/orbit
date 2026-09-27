import { afterEach, expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { insertPageBlock } from './pageBlockCommands'
import { pageEditorSchema, type PageEditorInstance } from './schema'

const mounted: { editor: PageEditorInstance; element: HTMLElement }[] = []

function mountEditor(initialContent: unknown[]): PageEditorInstance {
  const editor = BlockNoteEditor.create({ schema: pageEditorSchema, initialContent: initialContent as never })
  const element = document.createElement('div')
  document.body.append(element)
  editor.mount(element)
  mounted.push({ editor, element })
  return editor
}

afterEach(() => {
  for (const { editor, element } of mounted.splice(0)) {
    editor.unmount()
    element.remove()
  }
})

const shape = (editor: PageEditorInstance) => editor.document.map((block) => (block.type === 'page' ? `page:${block.props.pageId}` : block.type))

test('turns the empty block the slash menu was opened in into the page block, then adds a text block after it', () => {
  const editor = mountEditor([{ id: 'a', type: 'paragraph', content: [] }])
  insertPageBlock(editor, 'a', 'p1')
  expect(shape(editor)).toEqual(['page:p1', 'paragraph'])
  expect(editor.getTextCursorPosition().block.id).toBe(editor.document[1].id)
})

test('inserts after a non-empty block and moves the caret to the existing next text block', () => {
  const editor = mountEditor([
    { id: 'a', type: 'paragraph', content: 'intro' },
    { id: 'b', type: 'paragraph', content: 'next' },
  ])
  insertPageBlock(editor, 'a', 'p2')
  expect(shape(editor)).toEqual(['paragraph', 'page:p2', 'paragraph'])
  expect(editor.getTextCursorPosition().block.id).toBe('b')
})

test('appends at the end when the anchor block was deleted meanwhile', () => {
  const editor = mountEditor([{ id: 'a', type: 'heading', content: 'Title' }])
  insertPageBlock(editor, 'gone', 'p3')
  expect(shape(editor)).toEqual(['heading', 'page:p3', 'paragraph'])
})
