import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react'
import { Add, Copy, Edit2, MoreH, Trash } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverHeader, PopoverTitle, PopoverTrigger } from '@/components/ui/popover'
import { Tip } from '@/components/common/Tip'
import { confirmAction } from '@/components/common/confirmAction'
import type { DatabaseView } from '@orbit/database/types'
import { useCreateView, useDeleteView, useUpdateView } from './api'
import { useDatabaseEnv } from './context'
import { VIEW_KINDS, uniqueViewName, viewIcon } from './viewTypes'

const MAX_VIEWS = 50
const MAX_LINK_VIEWS = 20

/** The view tabs (spec 7.1): click selects; right-click or "⋯" renames, duplicates or deletes; "+" picks the type of a new view. */
export function ViewTabs({ views, activeId, onSelect }: { views: DatabaseView[]; activeId: string; onSelect: (viewId: string) => void }) {
  const { workspaceId, databaseId, canEditViews, linkId } = useDatabaseEnv()
  const createView = useCreateView(workspaceId, databaseId, linkId)
  const updateView = useUpdateView(workspaceId, databaseId, linkId)
  const deleteView = useDeleteView(workspaceId, databaseId, linkId)
  const maxViews = linkId ? MAX_LINK_VIEWS : MAX_VIEWS
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)
  const [picking, setPicking] = useState(false)
  const strip = useRef<HTMLDivElement>(null)
  // A narrow toolbar (a phone) scrolls the tabs: keep the active one in sight.
  useLayoutEffect(() => {
    const tab = strip.current?.querySelector('[role=tab][data-active]')
    if (!strip.current || !tab) return
    const box = strip.current.getBoundingClientRect()
    const rect = tab.getBoundingClientRect()
    if (rect.left < box.left) strip.current.scrollLeft += rect.left - box.left
    else if (rect.right > box.right) strip.current.scrollLeft += rect.right - box.right
  }, [activeId])

  const finishRename = () => {
    if (!renaming) return
    const view = views.find((candidate) => candidate.id === renaming.id)
    const name = renaming.name.trim().slice(0, 200)
    setRenaming(null)
    if (view && name && name !== view.name) updateView.mutate({ viewId: view.id, body: { name } })
  }
  const add = (type: DatabaseView['type'], name: string) => {
    setPicking(false)
    createView.mutate({ name: uniqueViewName(name, views), type }, { onSuccess: (view) => onSelect(view.id) })
  }
  // A copy keeps its source's type (the server reads it from `duplicate_of`).
  const duplicate = (view: DatabaseView) =>
    createView.mutate(
      { name: `${view.name} (copy)`.slice(0, 200), type: view.type, duplicate_of: view.id },
      { onSuccess: (copy) => onSelect(copy.id) },
    )
  const remove = async (view: DatabaseView) => {
    if (!(await confirmAction({ title: `Delete the view “${view.name}”?`, description: 'Its columns, sorts and filter go for everyone.', confirmLabel: 'Delete view', danger: true }))) return
    if (view.id === activeId) onSelect(views.find((other) => other.id !== view.id)?.id ?? activeId)
    deleteView.mutate(view.id)
  }

  return (
    // The first tab's icon starts at the text edge: the tabs hang their padding and border (11 px) into the margin.
    <div ref={strip} role="tablist" aria-label="Views" className="-ml-[11px] flex min-w-0 items-center gap-0.5 overflow-x-auto overflow-y-hidden -mb-[5px] pb-[5px]">
      {views.map((view) => {
        const selected = view.id === activeId
        const Icon = viewIcon(view.type)
        if (renaming?.id === view.id) {
          return (
            <input
              key={view.id}
              autoFocus
              aria-label="View name"
              className="h-7 w-36 rounded-md border border-ring bg-transparent px-2 text-sm outline-none"
              value={renaming.name}
              maxLength={200}
              onChange={(event) => setRenaming({ id: view.id, name: event.target.value })}
              onFocus={(event) => event.target.select()}
              onBlur={finishRename}
              onKeyDown={(event) => {
                if (event.key === 'Enter') finishRename()
                if (event.key === 'Escape') setRenaming(null)
              }}
            />
          )
        }
        return (
          <div key={view.id} className="group/tab relative flex shrink-0 items-center">
            <Button
              type="button"
              role="tab"
              variant="ghost"
              size="sm"
              aria-selected={selected}
              data-active={selected || undefined}
              className={cn('text-muted-foreground data-active:text-foreground data-active:after:absolute data-active:after:inset-x-1.5 data-active:after:-bottom-[5px] data-active:after:h-0.5 data-active:after:bg-foreground', canEditViews && 'pr-6')}
              onClick={() => onSelect(view.id)}
              onDoubleClick={() => canEditViews && setRenaming({ id: view.id, name: view.name })}
              onContextMenu={(event) => {
                if (!canEditViews) return
                event.preventDefault()
                setMenuFor(view.id)
              }}
            >
              <Icon className="size-3.5" aria-hidden="true" />
              <span className="max-w-40 truncate">{view.name}</span>
            </Button>
            {canEditViews ? (
              <DropdownMenu open={menuFor === view.id} onOpenChange={(open) => setMenuFor(open ? view.id : null)}>
                <DropdownMenuTrigger
                  render={
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-xs"
                      aria-label={`${view.name} view options`}
                      className="absolute right-0.5 size-5 text-muted-foreground hover-fine:opacity-0 hover-fine:group-hover/tab:opacity-100 aria-expanded:opacity-100"
                    />
                  }
                >
                  <MoreH />
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-44">
                  <DropdownMenuItem onClick={() => setRenaming({ id: view.id, name: view.name })}>
                    <Edit2 />
                    Rename
                  </DropdownMenuItem>
                  <DropdownMenuItem disabled={views.length >= maxViews} onClick={() => duplicate(view)}>
                    <Copy />
                    Duplicate
                  </DropdownMenuItem>
                  <DropdownMenuItem variant="destructive" disabled={views.length <= 1} onClick={() => void remove(view)}>
                    <Trash />
                    Delete
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
          </div>
        )
      })}
      {canEditViews && views.length < maxViews ? (
        <Popover open={picking} onOpenChange={setPicking}>
          <Tip label="Add a view">
            <PopoverTrigger render={<Button type="button" variant="ghost" size="icon-sm" aria-label="Add a view" className="text-muted-foreground" disabled={createView.isPending} />}>
              <Add />
            </PopoverTrigger>
          </Tip>
          <PopoverContent align="start" className="w-[316px] gap-2 p-2">
            <PopoverHeader className="px-1 pt-0.5">
              <PopoverTitle className="text-xs font-medium text-muted-foreground">Add a new view</PopoverTitle>
            </PopoverHeader>
            <ViewKindGrid onPick={add} />
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  )
}

const GRID_COLUMNS = 4

/** The view types as a grid of icon tiles; arrow keys move, Enter picks. Types that are not built yet say "Soon". */
function ViewKindGrid({ onPick }: { onPick: (type: DatabaseView['type'], name: string) => void }) {
  const tiles = useRef<(HTMLButtonElement | null)[]>([])
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: GRID_COLUMNS, ArrowUp: -GRID_COLUMNS }[event.key]
    if (step === undefined) return
    event.preventDefault()
    const from = tiles.current.indexOf(document.activeElement as HTMLButtonElement)
    const to = Math.min(Math.max((from === -1 ? 0 : from) + step, 0), VIEW_KINDS.length - 1)
    tiles.current[to]?.focus()
  }
  return (
    <div role="group" aria-label="View types" className="grid grid-cols-4 gap-1" onKeyDown={move}>
      {VIEW_KINDS.map((kind, index) => {
        const Icon = kind.icon
        return (
          <button
            key={kind.label}
            ref={(element) => {
              tiles.current[index] = element
            }}
            type="button"
            autoFocus={index === 0}
            aria-disabled={kind.type === null || undefined}
            className="flex h-[68px] flex-col items-center justify-center gap-1 rounded-md text-xs text-foreground outline-none hover:bg-muted/60 focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 aria-disabled:cursor-default aria-disabled:text-muted-foreground aria-disabled:hover:bg-transparent"
            onClick={() => {
              if (kind.type) onPick(kind.type, kind.label)
            }}
          >
            <Icon className="size-5" aria-hidden="true" />
            <span className="text-center leading-tight">
              {kind.label}
              {kind.type === null ? <span className="block text-[10px] text-muted-foreground/70">Soon</span> : null}
            </span>
          </button>
        )
      })}
    </div>
  )
}
