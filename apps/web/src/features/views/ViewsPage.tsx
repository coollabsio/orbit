import { useMemo, useState, type DragEvent, type KeyboardEvent } from 'react'
import { Link, useNavigate } from 'react-router'
import { Filter, Layer, Lock, Menu, People, Add as Plus, Star, type IconComponent } from 'reicon-react'
import { cn } from 'cn'
import { EmptyState } from '@/components/common/EmptyState'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { TaskSearchBox } from '@/features/tasks/components/TaskSearchBox'
import type { User } from '@/features/workspaces/models'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useReorderFavorites, useSavedViews, type SavedView } from './api/views'
import { FavoriteStar } from './components/FavoriteStar'
import { SaveViewDialog } from './components/SaveViewDialog'
import { ViewActionsMenu } from './components/ViewActionsMenu'
import { ViewIcon } from './components/ViewIcon'
import { GROUP_LABEL, LAYOUTS } from './displayMeta'
import { FIELD_META } from './filterFields'
import { rebaseViewSessionEdit } from './useViewState'
import { viewPath } from './viewActions'
import { defaultViewState, isGroup, normalizeViewState, type FilterField, type FilterGroup } from './viewState'

type RowDialog = { mode: 'edit' | 'duplicate'; view: SavedView }
type Section = { key: string; label: string; icon: IconComponent; views: SavedView[]; reorderable?: boolean }

/** `/views`: one list of the task views the user can see, favorites first, then their own, then the workspace's. */
export function ViewsPage() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const viewsQuery = useSavedViews(workspace.id)
  const members = useMembers(workspace.id).data ?? []
  const [search, setSearch] = useState('')
  const [dialog, setDialog] = useState<RowDialog | null>(null)
  const views = useMemo(() => (Array.isArray(viewsQuery.data) ? viewsQuery.data : []), [viewsQuery.data])
  const query = search.trim().toLowerCase()
  const sections = useMemo<Section[]>(() => {
    const shown = query ? views.filter((view) => `${view.name} ${view.description}`.toLowerCase().includes(query)) : views
    const favorites = shown.filter((view) => view.is_favorite).sort((a, b) => (a.favorite_position ?? 0) - (b.favorite_position ?? 0))
    // a favorite lists once, under Favorites; the star on it says it is starred
    const rest = shown.filter((view) => !view.is_favorite)
    return [
      // reordering a filtered list would send only part of the order, so a search turns it off
      { key: 'favorites', label: 'Favorites', icon: Star, views: favorites, reorderable: !query },
      { key: 'mine', label: 'Your views', icon: Lock, views: rest.filter((view) => view.visibility === 'personal') },
      { key: 'workspace', label: 'Workspace views', icon: People, views: rest.filter((view) => view.visibility === 'workspace') },
    ].filter((section) => section.views.length > 0)
  }, [views, query])
  const newView = () => navigate('/tasks?save_view=1')
  const rowActions: RowActions = {
    members,
    workspaceId: workspace.id,
    onEdit: (item) => setDialog({ mode: 'edit', view: item }),
    onDuplicate: (item) => setDialog({ mode: 'duplicate', view: item }),
  }

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      {/* the shell's top bar is hidden on /views, so this header carries the mobile menu button */}
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:h-auto max-[899px]:min-h-11 max-[899px]:flex-wrap max-[899px]:px-2 max-[899px]:py-1.5">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="hidden shrink-0 text-muted-foreground/70 max-[899px]:inline-flex"
          aria-label="Menu"
          onClick={() => window.dispatchEvent(new CustomEvent('open-sidebar'))}
        >
          <Menu className="size-[18px]" />
        </Button>
        {/* views are a Tasks feature: the header says so and leads back */}
        <nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-2 pl-1 text-[13px]">
          <Link to="/tasks" className="font-medium text-muted-foreground transition-colors hover:text-foreground">Tasks</Link>
          <span aria-hidden="true" className="text-muted-foreground/70 select-none">/</span>
          <h1 className="text-[13px] font-semibold text-foreground">Views</h1>
        </nav>
        <div className="flex-1" />
        {views.length > 0 ? <TaskSearchBox value={search} onChange={setSearch} label="Search views" /> : null}
        <Button type="button" className="max-[899px]:w-8 max-[899px]:px-0" aria-label="New view" onClick={newView}>
          <Plus className="size-4" />
          <span className="max-[899px]:hidden">New view</span>
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {viewsQuery.isPending ? (
          <p className="p-4 text-[13px] text-muted-foreground">Loading views…</p>
        ) : viewsQuery.isError ? (
          <div className="flex h-full flex-col p-2 *:flex-1">
            <EmptyState icon={Layer} title="Views unavailable" description="The server could not load views." />
          </div>
        ) : views.length === 0 ? (
          <div className="flex h-full flex-col p-2 *:flex-1">
            <EmptyState
              icon={Layer}
              title="No views yet"
              description="Filter or rearrange any task page, then save it as a view to come back to it."
              action={
                <Button type="button" onClick={newView}>
                  <Plus className="size-4" />
                  New view
                </Button>
              }
            />
          </div>
        ) : sections.length === 0 ? (
          <p className="px-4 py-6 text-[13px] text-muted-foreground">No views match “{search.trim()}”.</p>
        ) : (
          sections.map((section) => <ViewSection key={section.key} section={section} {...rowActions} />)
        )}
      </div>
      <SaveViewDialog
        open={dialog !== null}
        onOpenChange={(open) => {
          if (!open) setDialog(null)
        }}
        mode={dialog?.mode ?? 'edit'}
        workspaceId={workspace.id}
        state={dialog?.view.state ?? defaultViewState()}
        view={dialog?.view}
        onSaved={(saved) => {
          // our own rename moved the version: unsaved edits on the view now build on it (as on the view's page)
          if (dialog?.mode === 'edit') rebaseViewSessionEdit(workspace.id, saved.id, dialog.view.version, saved.version)
        }}
      />
    </section>
  )
}

interface RowActions {
  members: User[]
  workspaceId: string
  onEdit: (view: SavedView) => void
  onDuplicate: (view: SavedView) => void
}

/** A sticky header and its rows. Favorites reorder by drag, or Alt+Arrow on a focused row. */
function ViewSection({ section, ...actions }: RowActions & { section: Section }) {
  const reorder = useReorderFavorites(actions.workspaceId)
  const [order, setOrder] = useState<string[] | null>(null)
  const [dragId, setDragId] = useState<string | null>(null)
  const [overId, setOverId] = useState<string | null>(null)
  // the dropped order shows at once and yields to server data once the PUT settles
  const rows = order ? order.flatMap((id) => section.views.filter((view) => view.id === id)) : section.views

  const move = (id: string, to: number) => {
    const ids = rows.map((view) => view.id)
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
  const dragFor = (item: SavedView, index: number): RowDrag => {
    const target = overId === item.id && dragId !== null && dragId !== item.id
    return {
      dragging: dragId === item.id,
      target,
      // dropping takes the target's slot, so a downward move lands below it and an upward one above
      edge: target ? (rows.findIndex((row) => row.id === dragId) < index ? 'after' : 'before') : undefined,
      onDragStart: (event) => {
        setDragId(item.id)
        event.dataTransfer.effectAllowed = 'move'
        event.dataTransfer.setData('text/plain', item.id)
      },
      onDragOver: (event) => {
        if (!dragId) return
        event.preventDefault()
        event.dataTransfer.dropEffect = 'move'
        setOverId(item.id)
      },
      onDrop: (event) => {
        event.preventDefault()
        if (dragId) move(dragId, index)
        endDrag()
      },
      onDragEnd: endDrag,
      onKeyDown: (event) => {
        if (!event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return
        event.preventDefault()
        move(item.id, event.key === 'ArrowUp' ? index - 1 : index + 1)
        // React may move the focused node while reordering; keep focus on it for the next press
        const link = event.currentTarget
        requestAnimationFrame(() => link.focus())
      },
    }
  }

  return (
    <section>
      {/* same sticky header as a task list group, so the page reads as part of Tasks */}
      <div className="sticky top-0 z-[5] flex h-9 items-center gap-2 border-b bg-card px-3 text-xs font-semibold text-muted-foreground max-[899px]:px-2">
        <section.icon className="size-3.5 shrink-0" aria-hidden="true" />
        <h2 className="text-xs font-semibold">{section.label}</h2>
        <span className="font-normal text-muted-foreground/70 tabular-nums">{section.views.length}</span>
      </div>
      <ul aria-label={section.label} className="flex flex-col">
        {rows.map((item, index) => (
          <ViewRow key={item.id} view={item} {...actions} drag={section.reorderable ? dragFor(item, index) : undefined} />
        ))}
      </ul>
    </section>
  )
}

interface RowDrag {
  dragging: boolean
  target: boolean
  edge: 'before' | 'after' | undefined
  onDragStart: (event: DragEvent<HTMLLIElement>) => void
  onDragOver: (event: DragEvent<HTMLLIElement>) => void
  onDrop: (event: DragEvent<HTMLLIElement>) => void
  onDragEnd: () => void
  onKeyDown: (event: KeyboardEvent<HTMLAnchorElement>) => void
}

/** The fields a filter tree tests, in the order they first appear. */
function filterFieldsOf(group: FilterGroup, fields: FilterField[] = []): FilterField[] {
  for (const child of group.children) {
    if (isGroup(child)) filterFieldsOf(child, fields)
    else if (!fields.includes(child.field)) fields.push(child.field)
  }
  return fields
}

/** "Priority, Assignee +2": what the view narrows by, without loading every value's name. */
function filterSummary(group: FilterGroup): string {
  const labels = filterFieldsOf(group).map((field) => FIELD_META[field].label)
  if (labels.length === 0) return 'All tasks'
  return labels.slice(0, 2).join(', ') + (labels.length > 2 ? ` +${labels.length - 2}` : '')
}

/**
 * One 44px row: a tinted icon tile, the name, then what the view shows (its filter and layout) in aligned columns.
 * The name's stretched `after:` overlay makes the whole row the link.
 */
function ViewRow({ view, members, workspaceId, onEdit, onDuplicate, drag }: RowActions & { view: SavedView; drag?: RowDrag }) {
  const owner = members.find((person) => person.id === view.owner.user_id)
  const { filter, display } = normalizeViewState(view.state ?? defaultViewState())
  const layout = LAYOUTS.find((item) => item.value === display.layout) ?? LAYOUTS[0]
  const LayoutIcon = layout.icon
  return (
    <li
      className={cn(
        'group/row relative flex h-11 items-center gap-3 border-b px-3 transition-colors duration-150 ease-out hover-fine:hover:bg-foreground/[0.02] has-[a:focus-visible]:bg-muted/40 has-[a:focus-visible]:ring-2 has-[a:focus-visible]:ring-ring/50 has-[a:focus-visible]:ring-inset max-[899px]:gap-2 max-[899px]:px-2',
        // the dragged row fades and stays mounted (a drag cancels if its source leaves the DOM); a 2px pink line marks the drop
        'data-[dragging=true]:opacity-50 data-[drop-target=true]:before:absolute data-[drop-target=true]:before:inset-x-3 data-[drop-target=true]:before:h-0.5 data-[drop-target=true]:before:rounded-full data-[drop-target=true]:before:bg-primary data-[drop-edge=before]:before:-top-px data-[drop-edge=after]:before:-bottom-px',
      )}
      draggable={drag !== undefined}
      data-dragging={drag?.dragging || undefined}
      data-drop-target={drag?.target || undefined}
      data-drop-edge={drag?.edge}
      onDragStart={drag?.onDragStart}
      onDragOver={drag?.onDragOver}
      onDrop={drag?.onDrop}
      onDragEnd={drag?.onDragEnd}
    >
      <span
        className={cn('flex size-6 shrink-0 items-center justify-center rounded-md', !view.color && 'bg-muted')}
        // the view's own color, faint, so a list of views scans by color before name
        style={view.color ? { background: `${view.color}24` } : undefined}
      >
        <ViewIcon icon={view.icon} color={view.color} className="size-3.5" />
      </span>
      <div className="flex min-w-0 flex-1 items-baseline gap-2">
        <Link
          to={viewPath(view.id)}
          // the row is the drag source, not the link (a dragged link carries its URL and a text drag image)
          draggable={drag ? false : undefined}
          aria-keyshortcuts={drag ? 'Alt+ArrowUp Alt+ArrowDown' : undefined}
          onKeyDown={drag?.onKeyDown}
          className="min-w-0 shrink-0 truncate text-[13px] font-medium text-foreground outline-none after:absolute after:inset-0 max-[640px]:shrink"
        >
          {view.name}
        </Link>
        {view.description ? <span className="min-w-0 truncate text-[13px] text-muted-foreground max-[640px]:hidden">{view.description}</span> : null}
      </div>
      <span className="flex w-44 shrink-0 items-center gap-1.5 text-xs text-muted-foreground max-[899px]:hidden" title="Filters">
        <Filter className="size-3.5 shrink-0 opacity-70" aria-hidden="true" />
        <span className="truncate">{filterSummary(filter)}</span>
      </span>
      <span className="flex w-36 shrink-0 items-center gap-1.5 text-xs text-muted-foreground max-[1099px]:hidden" title="Layout">
        <LayoutIcon className="size-3.5 shrink-0 opacity-70" aria-hidden="true" />
        <span className="truncate">{layout.label}{display.group_by !== 'none' ? ` · ${GROUP_LABEL[display.group_by]}` : ''}</span>
      </span>
      <span className="relative z-10 flex shrink-0 items-center" title={view.owner.display_name}>
        <UserAvatar user={owner} name={view.owner.display_name} size={20} />
      </span>
      <div className="relative z-10 flex shrink-0 items-center gap-0.5">
        {/* row actions stay out of the way until the row is hovered or focused (touch always shows them);
            a favorite keeps its star lit */}
        <FavoriteStar
          workspaceId={workspaceId}
          view={view}
          named
          className={cn(!view.is_favorite && 'transition-opacity duration-150 hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100 hover-fine:group-focus-within/row:opacity-100')}
        />
        <ViewActionsMenu
          workspaceId={workspaceId}
          view={view}
          onEdit={() => onEdit(view)}
          onDuplicate={() => onDuplicate(view)}
          named
          align="end"
          className="transition-opacity duration-150 hover-fine:opacity-0 hover-fine:group-hover/row:opacity-100 hover-fine:group-focus-within/row:opacity-100 hover-fine:aria-expanded:opacity-100"
        />
      </div>
    </li>
  )
}
