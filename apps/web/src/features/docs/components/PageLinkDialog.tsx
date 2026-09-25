// Port of the docs reference's PageLinkDialog: search + pick an existing page.
// Built on the shadcn Command (cmdk): it owns the highlight, arrow-key navigation and Enter.
// Filtering stays ours (`shouldFilter={false}`) so the substring match is unchanged.
import { useMemo, useState } from 'react'
import { DocumentText as FileText } from 'reicon-react'
import type { PageSummary } from '@/api/generated/types.gen'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Emoji } from '@/components/common/Emoji'
import { Modal } from '@/components/common/Modal'
import { ancestorsOf, pageTitle } from '@/features/docs/pageTree'

export function PageLinkDialog({
  pages,
  excludeId,
  onPick,
  onClose,
}: {
  pages: PageSummary[]
  /** The current page: linking to itself makes no sense (the docs reference excludeId). */
  excludeId: string
  onPick: (page: PageSummary) => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')

  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    return pages
      .filter((page) => page.id !== excludeId)
      .filter((page) => !q || pageTitle(page).toLowerCase().includes(q))
  }, [pages, excludeId, query])

  return (
    <Modal title="Link a page" onClose={onClose}>
      <Command shouldFilter={false} label="Link a page" className="gap-2 bg-transparent p-0">
        <CommandInput autoFocus placeholder="Search pages…" value={query} onValueChange={setQuery} />
        <CommandList className="max-h-[320px]">
          <CommandEmpty className="px-3 py-7 text-[13px] text-muted-foreground/70">No pages found</CommandEmpty>
          {results.map((page) => {
            const parent = ancestorsOf(pages, page.id).at(-1)
            return (
              <CommandItem
                key={page.id}
                value={page.id}
                className="gap-[9px] rounded-[7px] px-2.5 py-2 text-foreground"
                onSelect={() => onPick(page)}
              >
                {page.icon ? <Emoji value={page.icon} size={16} /> : <FileText className="size-4" />}
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate">{pageTitle(page)}</span>
                  {parent ? <span className="truncate text-[11px] text-muted-foreground/70">{pageTitle(parent)}</span> : null}
                </span>
              </CommandItem>
            )
          })}
        </CommandList>
      </Command>
    </Modal>
  )
}
