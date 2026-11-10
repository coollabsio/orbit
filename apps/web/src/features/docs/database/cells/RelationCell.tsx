import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Check, DocumentText } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Emoji } from '@/components/common/Emoji'
import { Tip } from '@/components/common/Tip'
import { hiddenLinks, MAX_RELATION_LINKS, relatedTitle, rollupHidesPages, type RelationProperty, type RollupProperty } from '@orbit/database/relations'
import type { DatabaseRelatedPage, Row } from '@orbit/database/types'
import { cellOf, displayText } from '@orbit/database/values'
import { rememberRelatedPages, useDatabase } from '../api'
import { useDatabaseEnv } from '../context'
import { usePopoverClose, type CellEditorProps } from './CellEditor'
import { Pill } from './Pill'

/** Rows the picker lists under "Add" at once; typing narrows them. */
const PICKER_ROWS = 50

/** A row of this database opens in the peek; a row of another database opens as its page. */
function useOpenRelated() {
  const { databaseId, ctx, openRow } = useDatabaseEnv()
  const navigate = useNavigate()
  return (pageId: string) => {
    if (openRow && ctx.relatedPages?.[pageId]?.database_id === databaseId) openRow(pageId)
    else void navigate(`/docs/${pageId}`)
  }
}

function PageIcon({ icon }: { icon: string | null | undefined }) {
  return icon ? <Emoji value={icon} className="size-3.5 shrink-0" /> : <DocumentText className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
}

/** A relation cell (spec P3 11.8): a pill per related page; links the viewer cannot read show as "+N without access". */
export function RelationPills({ property, row, ids, wrap }: { property: RelationProperty; row: Row; ids: string[]; wrap: boolean }) {
  const { ctx } = useDatabaseEnv()
  const open = useOpenRelated()
  const hidden = hiddenLinks(row, property.id)
  return (
    <span className={cn('flex min-w-0 items-center gap-1', wrap ? 'flex-wrap py-1' : 'overflow-hidden')}>
      {ids.map((id) => (
        <button
          key={id}
          type="button"
          className="flex min-w-0 shrink-0 rounded-[4px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          onClick={(event) => {
            event.stopPropagation()
            open(id)
          }}
        >
          <Pill color="default" className="cursor-pointer hover:bg-foreground/[0.12]">
            <span className="flex min-w-0 items-center gap-1">
              <PageIcon icon={ctx.relatedPages?.[id]?.icon} />
              <span className="truncate">{relatedTitle(ctx, id) ?? 'Untitled'}</span>
            </span>
          </Pill>
        </button>
      ))}
      {hidden > 0 ? <span className="shrink-0 text-xs whitespace-nowrap text-muted-foreground">+{hidden} without access</span> : null}
    </span>
  )
}

/** A rollup cell: a number or a date as text, a list as pills. Pages the viewer cannot read are left out (tooltip). */
export function RollupView({ property, row, wrap }: { property: RollupProperty; row: Row; wrap: boolean }) {
  const { ctx } = useDatabaseEnv()
  const value = cellOf(row, property)
  const content =
    value?.kind === 'list' ? (
      <span className={cn('flex min-w-0 items-center gap-1', wrap ? 'flex-wrap py-1' : 'overflow-hidden')}>
        {value.items.map((item, index) => (
          <Pill key={index} color="default">
            {item}
          </Pill>
        ))}
      </span>
    ) : (
      <span
        data-number={value?.kind === 'number' || undefined}
        className={cn('min-w-0 text-muted-foreground data-number:w-full data-number:text-right data-number:tabular-nums', wrap ? 'break-words whitespace-pre-wrap' : 'truncate')}
      >
        {displayText(property, value, ctx)}
      </span>
    )
  if (!rollupHidesPages(row, property)) return content
  return (
    <Tip label="Some pages are hidden">
      <span className="flex w-full min-w-0">{content}</span>
    </Tip>
  )
}

/**
 * The relation picker (spec P3 11.8): searches the target database's rows by title, with "Linked" and "Add" sections.
 * Each pick saves the cell; a one-page relation replaces its page and closes.
 */
export function RelationEditor({ property, row, anchor, onCommit, onClose }: CellEditorProps & { property: RelationProperty }) {
  const { workspaceId, databaseId, ctx } = useDatabaseEnv()
  const queryClient = useQueryClient()
  const target = useDatabase(workspaceId, property.config.database_id)
  const value = cellOf(row, property)
  const [ids, setIds] = useState(value?.kind === 'pages' ? value.ids : [])
  const [query, setQuery] = useState('')
  const onOpenChange = usePopoverClose(anchor, onClose)
  const one = property.config.limit === 'one'
  const rows = target.data?.rows ?? []
  const byId = new Map(rows.map((candidate) => [candidate.id, candidate]))
  const titleOf = (id: string) => {
    const known = byId.get(id)
    return known ? known.title || 'Untitled' : (relatedTitle(ctx, id) ?? 'Untitled')
  }
  const needle = query.trim().toLowerCase()
  const fits = (id: string) => needle === '' || titleOf(id).toLowerCase().includes(needle)
  const linked = ids.filter(fits)
  const candidates = rows.filter((candidate) => !ids.includes(candidate.id) && fits(candidate.id)).slice(0, PICKER_ROWS)
  const hidden = hiddenLinks(row, property.id)

  const commit = (next: string[]) => {
    const pages: Record<string, DatabaseRelatedPage> = {}
    for (const id of next) {
      const known = byId.get(id)
      if (known) pages[id] = { title: known.title, icon: known.icon, database_id: property.config.database_id }
    }
    rememberRelatedPages(queryClient, workspaceId, databaseId, pages)
    setIds(next)
    onCommit(next.length > 0 ? { kind: 'pages', ids: next } : null)
  }
  const toggle = (id: string) => {
    if (one) {
      commit(ids[0] === id ? [] : [id])
      onClose('stay')
    } else if (ids.includes(id)) commit(ids.filter((other) => other !== id))
    else if (ids.length >= MAX_RELATION_LINKS) toast.error(`A relation links at most ${MAX_RELATION_LINKS} pages.`)
    else {
      commit([...ids, id])
      setQuery('')
    }
  }
  const empty = target.isError ? 'You cannot open the related database.' : target.isPending ? 'Loading…' : 'No pages'

  return (
    <Popover open onOpenChange={onOpenChange}>
      <PopoverContent anchor={anchor} align="start" className="w-72 gap-0 p-0">
        <Command label={property.name} shouldFilter={false} loop>
          <CommandInput
            autoFocus
            aria-label={`Search ${property.name}`}
            placeholder={`Search ${target.data?.database.title || 'pages'}…`}
            value={query}
            onValueChange={setQuery}
            onKeyDown={(event) => {
              event.stopPropagation()
              if (event.key === 'Tab') {
                event.preventDefault()
                onClose(event.shiftKey ? 'previous' : 'next')
              }
            }}
          />
          <CommandList>
            <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">{empty}</CommandEmpty>
            {linked.length > 0 ? (
              <CommandGroup heading={one ? 'Linked page' : 'Linked'}>
                {linked.map((id) => (
                  <CommandItem key={id} value={id} data-checked onSelect={() => toggle(id)}>
                    <PageIcon icon={byId.get(id)?.icon ?? ctx.relatedPages?.[id]?.icon} />
                    <span className="min-w-0 flex-1 truncate">{titleOf(id)}</span>
                    <Check className="text-muted-foreground" aria-label="Linked" />
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : null}
            {candidates.length > 0 ? (
              <CommandGroup heading="Add">
                {candidates.map((candidate) => (
                  <CommandItem key={candidate.id} value={candidate.id} onSelect={() => toggle(candidate.id)}>
                    <PageIcon icon={candidate.icon} />
                    <span className="min-w-0 flex-1 truncate">{candidate.title || 'Untitled'}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : null}
          </CommandList>
          {hidden > 0 ? (
            <p className="border-t border-border px-3 py-2 text-xs text-muted-foreground">
              {hidden === 1 ? '1 linked page you cannot open stays linked.' : `${hidden} linked pages you cannot open stay linked.`}
            </p>
          ) : null}
        </Command>
      </PopoverContent>
    </Popover>
  )
}
