import { describe, expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { withCollaboration } from '@blocknote/core/yjs'
import * as Y from 'yjs'
import { collabUndoExtension } from '@orbit/editor/collab/undo'

function collabEditor(doc: Y.Doc, extensions: ReturnType<typeof collabUndoExtension>[]) {
  return BlockNoteEditor.create(
    withCollaboration({
      _tiptapOptions: { injectCSS: false },
      extensions,
      collaboration: { fragment: doc.getXmlFragment('prosemirror'), user: { name: 'A', color: '#000' } },
    }),
  )
}

/** What React StrictMode does to the page editor: mount, unmount, mount again on the same editor. */
function remount(editor: BlockNoteEditor) {
  const element = document.createElement('div')
  document.body.append(element)
  editor.mount(element)
  editor.unmount()
  editor.mount(element)
}

function typeAndUndo(editor: BlockNoteEditor) {
  editor.insertBlocks([{ type: 'paragraph', content: 'typed' }], editor.document[0]!, 'after')
  const typed = JSON.stringify(editor.document).includes('typed')
  editor.undo()
  return { typed, undone: !JSON.stringify(editor.document).includes('typed') }
}

describe('collab undo', () => {
  test('undo still works after a remount', () => {
    const editor = collabEditor(new Y.Doc(), [collabUndoExtension()])
    remount(editor)
    expect(typeAndUndo(editor)).toEqual({ typed: true, undone: true })
  })
})
