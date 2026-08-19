import { ArrowRight, DocumentText, Trash } from 'reicon-react'
import type { Doc, DocBlock } from '../../../mock/types'

export function PageBlock({
  block,
  docs,
  onOpen,
  onRemove,
}: {
  block: DocBlock
  docs: Doc[]
  onOpen: (id: string) => void
  onRemove: () => void
}) {
  const page = docs.find((doc) => doc.id === block.refId)

  return (
    <div className="doc-page-block">
      <button
        type="button"
        className="doc-page-link"
        disabled={!page}
        onClick={() => page && onOpen(page.id)}
      >
        <DocumentText size={18} />
        <span className="truncate">{page?.title || block.text || 'Untitled'}</span>
        <ArrowRight size={15} />
      </button>
      <button type="button" className="icon-button doc-page-remove" aria-label="Remove page link" onClick={onRemove}>
        <Trash size={14} />
      </button>
    </div>
  )
}
