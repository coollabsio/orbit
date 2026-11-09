import { useState } from 'react'
import { Add, Copy, Edit2, Grid, MoreH, Trash } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tip } from '@/components/common/Tip'
import { confirmAction } from '@/components/common/confirmAction'
import type { DatabaseView } from '@orbit/database/types'
import { useCreateView, useDeleteView, useUpdateView } from './api'
import { useDatabaseEnv } from './context'

const MAX_VIEWS = 50

/** The view tabs (spec 7.1): click selects; right-click or "⋯" renames, duplicates or deletes; "+" adds a table view. */
export function ViewTabs({ views, activeId, onSelect }: { views: DatabaseView[]; activeId: string; onSelect: (viewId: string) => void }) {
  const { workspaceId, databaseId, canEditSchema } = useDatabaseEnv()
  const createView = useCreateView(workspaceId, databaseId)
  const updateView = useUpdateView(workspaceId, databaseId)
  const deleteView = useDeleteView(workspaceId, databaseId)
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)

  const finishRename = () => {
    if (!renaming) return
    const view = views.find((candidate) => candidate.id === renaming.id)
    const name = renaming.name.trim().slice(0, 200)
    setRenaming(null)
    if (view && name && name !== view.name) updateView.mutate({ viewId: view.id, body: { name } })
  }
  const add = (duplicateOf?: DatabaseView) =>
    createView.mutate(
      duplicateOf ? { name: `${duplicateOf.name} (copy)`.slice(0, 200), type: 'table', duplicate_of: duplicateOf.id } : { name: 'Table', type: 'table' },
      { onSuccess: (view) => onSelect(view.id) },
    )
  const remove = async (view: DatabaseView) => {
    if (!(await confirmAction({ title: `Delete the view “${view.name}”?`, description: 'Its columns, sorts and filter go for everyone.', confirmLabel: 'Delete view', danger: true }))) return
    if (view.id === activeId) onSelect(views.find((other) => other.id !== view.id)?.id ?? activeId)
    deleteView.mutate(view.id)
  }

  return (
    <div role="tablist" aria-label="Views" className="flex min-w-0 items-center gap-0.5 overflow-x-auto">
      {views.map((view) => {
        const selected = view.id === activeId
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
              className={cn('text-muted-foreground data-active:text-foreground data-active:after:absolute data-active:after:inset-x-1.5 data-active:after:-bottom-[5px] data-active:after:h-0.5 data-active:after:bg-foreground', canEditSchema && 'pr-6')}
              onClick={() => onSelect(view.id)}
              onDoubleClick={() => canEditSchema && setRenaming({ id: view.id, name: view.name })}
              onContextMenu={(event) => {
                if (!canEditSchema) return
                event.preventDefault()
                setMenuFor(view.id)
              }}
            >
              <Grid className="size-3.5" aria-hidden="true" />
              <span className="max-w-40 truncate">{view.name}</span>
            </Button>
            {canEditSchema ? (
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
                  <DropdownMenuItem disabled={views.length >= MAX_VIEWS} onClick={() => add(view)}>
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
      {canEditSchema && views.length < MAX_VIEWS ? (
        <Tip label="Add a view">
          <Button type="button" variant="ghost" size="icon-sm" aria-label="Add a view" className="text-muted-foreground" disabled={createView.isPending} onClick={() => add()}>
            <Add />
          </Button>
        </Tip>
      ) : null}
    </div>
  )
}
