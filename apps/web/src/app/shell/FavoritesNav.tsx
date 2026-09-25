import { useMemo, useState } from 'react'
import { NavLink, useLocation } from 'react-router'
import { cn } from 'cn'
import { useReorderFavorites, useSavedViews } from '@/features/views/api/views'
import { ViewIcon } from '@/features/views/components/ViewIcon'
import { viewPath } from '@/features/views/viewActions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SidebarSection, sidebarItemClass, sidebarLabelClass } from './SidebarNav'

/** The dragged item fades and stays mounted (a drag cancels if its source leaves the DOM); a 2px pink line marks the drop. */
const DRAG_STATES =
  'data-[dragging=true]:opacity-50 data-[drop-target=true]:before:absolute data-[drop-target=true]:before:inset-x-1 data-[drop-target=true]:before:top-0 data-[drop-target=true]:before:h-0.5 data-[drop-target=true]:before:rounded-full data-[drop-target=true]:before:bg-primary data-[drop-edge=after]:before:top-auto data-[drop-edge=after]:before:bottom-0'

/** Sidebar section above Personal; hidden until the user favorites a view. Drag or Alt+Arrow to reorder. */
export function FavoritesNav({ collapsed = false, onNavigate }: { collapsed?: boolean; onNavigate?: () => void }) {
  const { workspace } = useWorkspace()
  const viewsQuery = useSavedViews(workspace.id)
  const reorder = useReorderFavorites(workspace.id)
  const location = useLocation()
  const [order, setOrder] = useState<string[] | null>(null)
  const [dragId, setDragId] = useState<string | null>(null)
  const [overId, setOverId] = useState<string | null>(null)
  const saved = useMemo(
    // the shell mounts this on every page, so a malformed body must not take the sidebar down
    () => (Array.isArray(viewsQuery.data) ? viewsQuery.data : [])
      .filter((view) => view.is_favorite)
      .sort((a, b) => (a.favorite_position ?? 0) - (b.favorite_position ?? 0)),
    [viewsQuery.data],
  )
  // the dropped order shows at once and yields to server data once the PUT settles
  const favorites = order ? order.flatMap((id) => saved.filter((view) => view.id === id)) : saved
  if (favorites.length === 0) return null

  const move = (id: string, to: number) => {
    const ids = favorites.map((view) => view.id)
    const from = ids.indexOf(id)
    if (from < 0 || to < 0 || to >= ids.length || from === to) return
    ids.splice(from, 1)
    ids.splice(to, 0, id)
    setOrder(ids)
    reorder.mutate(ids, { onSettled: () => setOrder(null) })
  }
  const endDrag = () => {
    setDragId(null)
    setOverId(null)
  }

  return (
    <>
      <SidebarSection label="Favorites" collapsed={collapsed} />
      {favorites.map((view, index) => (
        <NavLink
          key={view.id}
          to={viewPath(view.id)}
          draggable
          className={cn(sidebarItemClass(location.pathname === viewPath(view.id), collapsed), DRAG_STATES)}
          data-dragging={dragId === view.id || undefined}
          data-drop-target={(overId === view.id && dragId !== view.id) || undefined}
          // dropping takes the target's slot, so moving down lands below it
          data-drop-edge={overId === view.id && dragId && favorites.findIndex((item) => item.id === dragId) < index ? 'after' : undefined}
          aria-label={view.name}
          aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
          title={collapsed ? view.name : undefined}
          onClick={onNavigate}
          onDragStart={(event) => {
            setDragId(view.id)
            event.dataTransfer.effectAllowed = 'move'
            event.dataTransfer.setData('text/plain', view.id)
          }}
          onDragOver={(event) => {
            if (!dragId) return
            event.preventDefault()
            event.dataTransfer.dropEffect = 'move'
            setOverId(view.id)
          }}
          onDrop={(event) => {
            event.preventDefault()
            if (dragId) move(dragId, index)
            endDrag()
          }}
          onDragEnd={endDrag}
          onKeyDown={(event) => {
            if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return
            event.preventDefault()
            move(view.id, event.key === 'ArrowUp' ? index - 1 : index + 1)
            // React may move the focused node while reordering; keep focus on it for the next press
            const link = event.currentTarget
            requestAnimationFrame(() => link.focus())
          }}
        >
          <ViewIcon icon={view.icon} color={view.color} className="size-[18px] opacity-90" />
          <span className={sidebarLabelClass(collapsed)}>{view.name}</span>
        </NavLink>
      ))}
    </>
  )
}
