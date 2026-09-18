import { renderJSONContentToReactElement } from '@tiptap/static-renderer/json/react'
import { asDocument } from './document'
import { markMapping, nodeMapping, passThrough } from './nodeMapping'
import { TaskChipContext, type TaskChipLookup } from './taskChipContext'
import './editor.css'

/**
 * Walks the stored JSON directly. The JSON renderer only depends on React — no
 * schema, no ProseMirror — so comment lists and task descriptions cost a plain
 * React tree and never pull the editor chunk.
 */
const render = (content: ReturnType<typeof asDocument>) =>
  renderJSONContentToReactElement({
    nodeMapping,
    markMapping,
    unhandledNode: passThrough,
    unhandledMark: passThrough,
  })({ content })

export function RichTextView({
  document,
  chips,
  className = 'editor-view',
}: {
  document: unknown
  chips?: TaskChipLookup
  className?: string
}) {
  return (
    <TaskChipContext value={chips}>
      <div className={className}>{render(asDocument(document))}</div>
    </TaskChipContext>
  )
}

export type { TaskChipLookup }
