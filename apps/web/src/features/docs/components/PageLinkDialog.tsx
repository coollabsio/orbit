// Port of the docs reference's PageLinkDialog: search + pick an existing page.
// Built on the shadcn Command (cmdk): it owns the highlight, arrow-key navigation and Enter.
// Filtering stays ours (`shouldFilter={false}`) so the substring match is unchanged.
import { useMemo, useState } from 'react'
import { DocumentText as FileText } from 'reicon-react'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Modal } from '@/components/common/Modal'
import type { Doc } from '@/mock/types'
import { ancestorsOf } from '@/features/docs/docsLib'

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

  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    return docs
      .filter((d) => d.id !== excludeId)
      .filter((d) => !q || d.title.toLowerCase().includes(q))
  }, [docs, excludeId, query])

  return (
    <Modal title="Link a page" onClose={onClose}>
      <Command shouldFilter={false} label="Link a page" className="gap-2 bg-transparent p-0">
        <CommandInput autoFocus placeholder="Search pages…" value={query} onValueChange={setQuery} />
        <CommandList className="max-h-[320px]">
          <CommandEmpty className="px-3 py-7 text-[13px] text-muted-foreground/70">No pages found</CommandEmpty>
          {results.map((doc) => {
            const parent = ancestorsOf(docs, doc.id).at(-1)
            return (
              <CommandItem
                key={doc.id}
                value={doc.id}
                className="gap-[9px] rounded-[7px] px-2.5 py-2 text-foreground"
                onSelect={() => onPick(doc)}
              >
                <FileText className="size-4" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate">{doc.title || 'Untitled'}</span>
                  {parent ? <span className="truncate text-[11px] text-muted-foreground/70">{parent.title}</span> : null}
                </span>
              </CommandItem>
            )
          })}
        </CommandList>
      </Command>
    </Modal>
  )
}
