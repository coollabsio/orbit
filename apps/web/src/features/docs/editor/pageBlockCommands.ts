import type { PageEditorInstance } from './schema'

function isEmptyInlineBlock(block: { content?: unknown }): boolean {
  return Array.isArray(block.content) && block.content.length === 0
}

/**
 * Inserts a `page` block for `pageId` where the slash menu was opened (`anchorId`): an empty anchor block is
 * turned into the page block, otherwise the page block goes right after it. The caret then moves to the next
 * text block (a paragraph is appended when the page block ends the document). If the anchor disappeared while
 * an async action was pending, the block is appended at the end.
 */
export function insertPageBlock(editor: PageEditorInstance, anchorId: string, pageId: string): void {
  const pageBlock = { type: 'page' as const, props: { pageId } }
  editor.transact(() => {
    const anchor = editor.getBlock(anchorId)
    let inserted
    if (anchor && isEmptyInlineBlock(anchor) && !anchor.children.length) {
      inserted = editor.updateBlock(anchor, pageBlock)
    } else {
      const reference = anchor ?? editor.document[editor.document.length - 1]
      inserted = editor.insertBlocks([pageBlock], reference, 'after')[0]
    }

    const next = editor.getNextBlock(inserted)
    if (next && Array.isArray(next.content)) {
      editor.setTextCursorPosition(next, 'start')
    } else {
      const paragraph = editor.insertBlocks([{ type: 'paragraph' }], inserted, 'after')[0]
      editor.setTextCursorPosition(paragraph, 'start')
    }
  })
}
