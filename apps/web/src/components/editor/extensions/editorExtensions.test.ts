import { expect, mock, test } from 'bun:test'
import { Editor, type JSONContent } from '@tiptap/core'
import { toServerDocument } from '../document'
import { buildEditorExtensions, type EditorKeyHandlers } from './editorExtensions'

/**
 * A mirror of the server allowlist (`crates/orbit/src/rich_text.rs`), used to
 * prove that what the real extension set emits is accepted once it has been
 * through `toServerDocument`.
 */
const NODE_ATTRS: Record<string, string[]> = {
  doc: [], paragraph: [], text: [], heading: ['level'], bulletList: [], orderedList: ['start'], listItem: [],
  taskList: [], taskItem: ['checked'], blockquote: [], codeBlock: ['language'], horizontalRule: [],
  hardBreak: [], mention: ['id', 'label'], taskMention: ['id', 'identifier'],
}
const MARK_ATTRS: Record<string, string[]> = {
  bold: [], italic: [], strike: [], code: [], underline: [], link: ['href', 'target', 'rel'],
}

function violations(node: JSONContent, found: string[] = []): string[] {
  const allowed = NODE_ATTRS[node.type ?? '']
  if (!allowed) found.push(`node ${node.type}`)
  for (const key of Object.keys(node.attrs ?? {})) if (!allowed?.includes(key)) found.push(`${node.type}.${key}`)
  for (const mark of node.marks ?? []) {
    const markAllowed = MARK_ATTRS[mark.type]
    if (!markAllowed) found.push(`mark ${mark.type}`)
    for (const key of Object.keys(mark.attrs ?? {})) if (!markAllowed?.includes(key)) found.push(`${mark.type}.${key}`)
  }
  for (const child of node.content ?? []) violations(child, found)
  return found
}

const everything: JSONContent = {
  type: 'doc',
  content: [
    { type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text: 'Scope' }] },
    {
      type: 'paragraph',
      content: [
        { type: 'text', text: 'bold ', marks: [{ type: 'bold' }] },
        { type: 'text', text: 'under ', marks: [{ type: 'underline' }] },
        { type: 'text', text: 'site', marks: [{ type: 'link', attrs: { href: 'https://example.com' } }] },
        { type: 'hardBreak' },
        { type: 'mention', attrs: { id: 'user-1', label: 'Ada Lovelace' } },
        { type: 'text', text: ' ' },
        { type: 'taskMention', attrs: { id: 'task-1', identifier: 'ORB-12' } },
      ],
    },
    { type: 'orderedList', attrs: { start: 3 }, content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'two' }] }] }] },
    { type: 'bulletList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'one' }] }] }] },
    { type: 'taskList', content: [{ type: 'taskItem', attrs: { checked: true }, content: [{ type: 'paragraph', content: [{ type: 'text', text: 'done' }] }] }] },
    { type: 'blockquote', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'quoted' }] }] },
    { type: 'codeBlock', attrs: { language: 'rust' }, content: [{ type: 'text', text: 'fn main() {}' }] },
    { type: 'horizontalRule' },
    { type: 'paragraph', content: [{ type: 'text', text: 'end' }] },
  ],
}

function editorWith(keys: Partial<EditorKeyHandlers> = {}) {
  return new Editor({
    extensions: buildEditorExtensions({
      placeholder: 'Write…',
      mentions: { workspaceId: 'w', members: () => [] },
      resolve: null,
      keys: { submit: () => false, cancel: () => false, ...keys },
    }),
    content: everything,
  })
}

test('raw editor JSON carries attributes the server rejects, which is why it is always sanitised', () => {
  const editor = editorWith()

  expect(violations(editor.getJSON())).toContain('orderedList.type')
  expect(violations(editor.getJSON())).toContain('link.class')
  editor.destroy()
})

test('every allowlisted node and mark survives the editor and leaves in the server shape', () => {
  const editor = editorWith()
  const sent = toServerDocument(editor.getJSON())

  expect(violations(sent)).toEqual([])
  expect(sent).toEqual(toServerDocument(everything))
  editor.destroy()
})

test('only allowlisted heading levels exist in the schema', () => {
  const editor = editorWith()

  expect(editor.can().setHeading({ level: 3 })).toBe(true)
  expect(editor.can().setHeading({ level: 4 as 3 })).toBe(false)
  editor.destroy()
})

test('Cmd/Ctrl+Enter submits the current document when a submit handler wants it', () => {
  const submit = mock((_document: JSONContent) => true)
  const editor = editorWith({ submit })

  editor.commands.keyboardShortcut('Mod-Enter')

  expect(submit).toHaveBeenCalledTimes(1)
  expect(toServerDocument(submit.mock.calls[0][0])).toEqual(toServerDocument(everything))
  editor.destroy()
})

test('without a submit handler Cmd/Ctrl+Enter falls through to a hard break', () => {
  const editor = editorWith()
  editor.commands.setContent({ type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'a' }] }] })
  editor.commands.focus('end')

  editor.commands.keyboardShortcut('Mod-Enter')

  expect(editor.getJSON().content?.[0]?.content?.map((node) => node.type)).toEqual(['text', 'hardBreak'])
  editor.destroy()
})

test('Escape reports a cancel', () => {
  const cancel = mock(() => true)
  const editor = editorWith({ cancel })

  editor.commands.keyboardShortcut('Escape')

  expect(cancel).toHaveBeenCalledTimes(1)
  editor.destroy()
})
