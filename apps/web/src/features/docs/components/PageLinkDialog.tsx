// Port of the docs reference's PageLinkDialog: search + pick an existing page.
import { useMemo, useState } from 'react'
import { FileText, Search } from 'lucide-react'
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
      <div className="mb-2 flex items-center gap-2 rounded-[7px] border border-border bg-muted px-2.5 text-muted-foreground/70 focus-within:border-primary focus-within:ring-1 focus-within:ring-primary">
        <Search className="size-[15px]" />
        <input
          className="h-9 w-full border-0 bg-transparent text-foreground outline-none placeholder:text-muted-foreground/70"
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
      <div className="max-h-[320px] overflow-y-auto">
        {results.length === 0 ? (
          <div className="px-3 py-7 text-center text-[13px] text-muted-foreground/70">No pages found</div>
        ) : (
          results.map((doc, index) => {
            const parent = ancestorsOf(docs, doc.id).at(-1)
            return (
              <button
                key={doc.id}
                type="button"
                className="flex w-full items-center gap-[9px] rounded-[7px] px-2.5 py-2 text-left text-foreground hover:bg-muted data-[selected=true]:bg-muted"
                data-selected={index === activeIndex ? 'true' : undefined}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => onPick(doc)}
              >
                <FileText className="size-4" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate">{doc.title || 'Untitled'}</span>
                  {parent ? <span className="truncate text-[11px] text-muted-foreground/70">{parent.title}</span> : null}
                </span>
              </button>
            )
          })
        )}
      </div>
    </Modal>
  )
}
