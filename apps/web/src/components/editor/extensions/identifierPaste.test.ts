import { expect, test } from 'bun:test'
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import {
  IdentifierPaste,
  createIdentifierInputRule,
  identifierFromTaskUrl,
  taskIdFromTaskUrl,
  taskReferenceInPaste,
} from './identifierPaste'
import { TaskMention } from './taskMention'
import type { TaskResolver } from './taskResolver'

const origin = 'https://orbit.example'

test('a same-origin task URL yields the task id it points at', () => {
  expect(taskIdFromTaskUrl(`${origin}/tasks/0193c0de-0000-7000-8000-000000000002`, origin)).toBe(
    '0193c0de-0000-7000-8000-000000000002',
  )
  expect(taskIdFromTaskUrl(`${origin}/tasks/0193c0de-0000-7000-8000-000000000002?focus=1`, origin)).toBe(
    '0193c0de-0000-7000-8000-000000000002',
  )
})

test('a foreign origin or a non-task path is left alone', () => {
  expect(taskIdFromTaskUrl('https://evil.example/tasks/0193c0de-0000-7000-8000-000000000002', origin)).toBeNull()
  expect(taskIdFromTaskUrl(`${origin}/projects/abc`, origin)).toBeNull()
  expect(taskIdFromTaskUrl('not a url', origin)).toBeNull()
  expect(taskIdFromTaskUrl('/tasks/0193c0de-0000-7000-8000-000000000002', origin)).toBeNull()
})

test('a URL carrying an identifier segment yields the identifier', () => {
  expect(identifierFromTaskUrl(`${origin}/tasks/ORB-12`, origin)).toBe('ORB-12')
  expect(identifierFromTaskUrl(`${origin}/tasks/orb-12`, origin)).toBeNull()
  expect(identifierFromTaskUrl(`${origin}/tasks/0193c0de-0000-7000-8000-000000000002`, origin)).toBeNull()
})

test('the input rule fires on a bare identifier followed by a boundary', () => {
  const { find } = createIdentifierInputRule()

  expect('ORB-12 '.match(find)?.[1]).toBe('ORB-12')
  expect('see ACME_2-134 '.match(find)?.[1]).toBe('ACME_2-134')
  expect('ORB-12'.match(find)).toBeNull()
  expect('orb-12 '.match(find)).toBeNull()
  expect('WORD-WORD '.match(find)).toBeNull()
  expect('xORB-12 '.match(find)).toBeNull()
})

test('a paste is a task reference only when it is exactly an identifier or a task link', () => {
  expect(taskReferenceInPaste(' ORB-12 ', origin)).toEqual({ identifier: 'ORB-12' })
  expect(taskReferenceInPaste(`${origin}/tasks/ORB-12`, origin)).toEqual({ identifier: 'ORB-12' })
  expect(taskReferenceInPaste(`${origin}/tasks/0193c0de-0000-7000-8000-000000000002`, origin)).toEqual({
    taskId: '0193c0de-0000-7000-8000-000000000002',
  })
  expect(taskReferenceInPaste('see ORB-12', origin)).toBeNull()
  expect(taskReferenceInPaste('https://example.com/tasks/ORB-12', origin)).toBeNull()
})

function editorWith(resolve: TaskResolver) {
  return new Editor({
    extensions: [StarterKit, TaskMention, IdentifierPaste.configure({ resolve })],
    content: { type: 'doc', content: [{ type: 'paragraph' }] },
  })
}

/** Loosely typed so plain JSON literals compare without TipTap's node generics. */
const firstBlock = (editor: Editor): unknown => editor.getJSON().content?.[0]?.content

const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

test('pasting a known identifier becomes a chip once it resolves', async () => {
  const editor = editorWith(async ({ identifier }) => (identifier === 'ORB-12' ? { id: 'task-12', identifier } : null))

  editor.view.pasteText('ORB-12')
  expect(editor.getText()).toBe('ORB-12')
  await settle()

  expect(firstBlock(editor)).toEqual([
    { type: 'taskMention', attrs: { id: 'task-12', identifier: 'ORB-12' } },
  ])
  editor.destroy()
})

test('an unknown identifier stays plain text', async () => {
  const editor = editorWith(async () => null)

  editor.view.pasteText('NOPE-1')
  await settle()

  expect(firstBlock(editor)).toEqual([{ type: 'text', text: 'NOPE-1' }])
  editor.destroy()
})

test('typing a space after an identifier chips it, keeping the space', async () => {
  const editor = editorWith(async ({ identifier }) => (identifier ? { id: 'task-12', identifier } : null))
  editor.commands.insertContent('see ORB-12')
  const at = editor.state.selection.from

  const handled = editor.view.someProp('handleTextInput', (handler) =>
    handler(editor.view, at, at, ' ', () => editor.state.tr.insertText(' ', at, at)),
  )
  expect(handled).toBe(true)
  await settle()

  expect(firstBlock(editor)).toEqual([
    { type: 'text', text: 'see ' },
    { type: 'taskMention', attrs: { id: 'task-12', identifier: 'ORB-12' } },
    { type: 'text', text: ' ' },
  ])
  editor.destroy()
})

test('an identifier edited away before the answer arrives is left alone', async () => {
  let answer: (value: { id: string; identifier: string }) => void = () => {}
  const editor = editorWith(() => new Promise((resolve) => (answer = resolve)))

  editor.view.pasteText('ORB-12')
  editor.commands.setContent({ type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'rewritten' }] }] })
  answer({ id: 'task-12', identifier: 'ORB-12' })
  await settle()

  expect(editor.getText()).toBe('rewritten')
  editor.destroy()
})
