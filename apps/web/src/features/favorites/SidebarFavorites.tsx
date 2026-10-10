import { useState, type DragEvent } from 'react'
import { Link, useLocation } from 'react-router'
import { Signpost, TaskSquare } from 'reicon-react'
import { ColorDot } from '@/components/common/ColorDot'
import { ViewIcon } from '@/features/views/components/ViewIcon'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useFavorites, useReorderFavorites, type Favorite, type FavoriteKind } from './api/favorites'

const idOf = (item: Favorite) => `${item.kind}:${item.target_id}`

/** The "Favorites" section of the sidebar, below Tasks: the caller's starred tasks, views, projects and milestones. Nothing shows while it
 *  is empty. Rows reorder by drag; the dragged row stays mounted (a drag ends when its source leaves the DOM). */
export function SidebarFavorites({ onNavigate }: { onNavigate?: () => void }) {
  const { workspace } = useWorkspace()
  const location = useLocation()
  const favorites = useFavorites(workspace.id).data ?? []
  const reorder = useReorderFavorites(workspace.id)
  const [dragId, setDragId] = useState<string | null>(null)
  const [overId, setOverId] = useState<string | null>(null)
  if (favorites.length === 0) return null

  const endDrag = () => {
    setDragId(null)
    setOverId(null)
  }
  const drop = (index: number) => {
    const from = favorites.findIndex((item) => idOf(item) === dragId)
    if (from < 0 || from === index) return
    const next = [...favorites]
    const [moved] = next.splice(from, 1)
    next.splice(index, 0, moved!)
    reorder.mutate(next.map((item) => ({ kind: item.kind as FavoriteKind, targetId: item.target_id })))
  }

  return (
    <section aria-label="Favorites" className="flex flex-col gap-0.5">
      <div className="mt-2 px-2 pt-1 pb-[3px] text-[10px] leading-4 font-medium tracking-[0.02em] text-sidebar-foreground/60 select-none">
        Favorites
      </div>
      <ul className="flex flex-col gap-0.5">
        {favorites.map((item, index) => {
          const id = idOf(item)
          const target = overId === id && dragId !== null && dragId !== id
          const active = location.pathname === item.path
          return (
            <li
              key={id}
              draggable
              data-dragging={dragId === id || undefined}
              // dropping takes the target's slot, so a downward move lands below it and an upward one above
              data-drop={target ? (favorites.findIndex((row) => idOf(row) === dragId) < index ? 'after' : 'before') : undefined}
              className="rounded-md data-[dragging]:opacity-40 data-[drop=after]:shadow-[0_2px_0_0_var(--primary)] data-[drop=before]:shadow-[0_-2px_0_0_var(--primary)]"
              onDragStart={(event: DragEvent<HTMLLIElement>) => {
                setDragId(id)
                event.dataTransfer.effectAllowed = 'move'
                event.dataTransfer.setData('text/plain', id)
              }}
              onDragOver={(event) => {
                if (!dragId) return
                event.preventDefault()
                event.dataTransfer.dropEffect = 'move'
                setOverId(id)
              }}
              onDrop={(event) => {
                event.preventDefault()
                drop(index)
                endDrag()
              }}
              onDragEnd={endDrag}
            >
              <Link
                to={item.path}
                draggable={false}
                data-active={active}
                aria-current={active ? 'page' : undefined}
                className="flex h-7 min-w-0 items-center gap-2 rounded-md px-2 text-[13px] text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground"
                onClick={onNavigate}
              >
                {/* a view has its own icon; a milestone a signpost in the colour of its project; a task or a project the project's dot */}
                {item.kind === 'view'
                  ? <ViewIcon icon={item.icon} color={item.color} />
                  : item.kind === 'milestone'
                  ? <Signpost aria-hidden className="size-4 shrink-0" style={item.color ? { color: item.color } : undefined} />
                  : item.color ? <ColorDot color={item.color} className="mx-1 size-2 shrink-0" /> : <TaskSquare className="size-4 shrink-0 opacity-90" />}
                <span className="min-w-0 flex-1 truncate">{item.title}</span>
                {item.identifier ? <span className="shrink-0 text-[11px] text-muted-foreground tabular-nums">{item.identifier}</span> : null}
              </Link>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
