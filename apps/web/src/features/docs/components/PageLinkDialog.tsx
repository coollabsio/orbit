// Port of the docs reference's PageLinkDialog: search + pick an existing page.
import { useMemo, useState } from 'react'
import { DocumentText, SearchNormal } from 'reicon-react'
import { Modal } from '../../../components/ui/Modal'
import type { Doc } from '../../../mock/types'
import { ancestorsOf } from '../lib'

export function PageLinkDialog({
  docs,
  excludeId,
  onPick,
  onClose,
}: {
  docs: Doc[]
  /** The current page: linking to itself makes no sense (the docs reference excludeId). */
  excludeId: string
  onPick: (doc: Doc) => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)

  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    return docs
      .filter((d) => d.id !== excludeId)
      .filter((d) => !q || d.title.toLowerCase().includes(q))
  }, [docs, excludeId, query])

  return (
    <Modal title="Link a page" onClose={onClose}>
      <div className="page-link-search">
        <SearchNormal size={15} />
        <input
          autoFocus
          placeholder="Search pages…"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setActiveIndex(0)
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setActiveIndex((i) => Math.min(i + 1, results.length - 1))
            }
            if (e.key === 'ArrowUp') {
              e.preventDefault()
              setActiveIndex((i) => Math.max(i - 1, 0))
            }
            if (e.key === 'Enter' && results[activeIndex]) {
              e.preventDefault()
              onPick(results[activeIndex])
            }
          }}
        />
      </div>
      <div className="page-link-list">
        {results.length === 0 ? (
          <div className="page-link-empty">No pages found</div>
        ) : (
          results.map((doc, index) => {
            const parent = ancestorsOf(docs, doc.id).at(-1)
            return (
              <button
                key={doc.id}
                type="button"
                className="page-link-row"
                data-selected={index === activeIndex ? 'true' : undefined}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => onPick(doc)}
              >
                <DocumentText size={16} />
                <span className="page-link-row-text">
                  <span className="truncate">{doc.title || 'Untitled'}</span>
                  {parent ? <span className="page-link-row-parent truncate">{parent.title}</span> : null}
                </span>
              </button>
            )
          })
        )}
      </div>
    </Modal>
  )
}
