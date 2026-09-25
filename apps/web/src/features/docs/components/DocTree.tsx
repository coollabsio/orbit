import { useEffect, useRef, useState, type DragEvent, type FormEvent, type ReactNode } from 'react'
import { NavLink, useNavigate } from 'react-router'
import { toast } from 'sonner'
import {
  ArrowDown,
  ArrowUp,
  ChevronRight,
  EmojiHappy as Smile,
  MoreH as Ellipsis,
  DocumentText as FileText,
  Lock,
  Import,
  Edit as Pencil,
  Add as Plus,
  Star,
  Trash as Trash2,
  People as Users,
} from 'reicon-react'
import type { PageSummary, Teamspace } from '@/api/generated/types.gen'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Spinner } from '@/components/ui/spinner'
import { Emoji } from '@/components/common/Emoji'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { EmptyState } from '@/components/common/EmptyState'
import { confirmAction } from '@/components/common/confirmAction'
import {
  PRIVATE_SPACE,
  ancestorsOf,
  canDropOn,
  dropOnSpace,
  dropToMove,
  moveSpace,
  revealPage,
  rootsOf,
  spaceKey,
  spaceLabel,
  spaceRequest,
  teamspaceSpace,
  type MoveTarget,
  type SpaceKey,
} from '@/features/docs/pageTree'
import { favoriteDropIndex, useMoveFavorite, usePageFavorites, useToggleFavorite } from '@/features/docs/api/favorites'
import { isPageVersionConflict, useCreatePage, useMovePage } from '@/features/docs/api/pages'
import {
  teamspaceDeleteError,
  teamspaceDropIndex,
  useCreateTeamspace,
  useDeleteTeamspace,
  useMoveTeamspace,
  useTeamspaces,
  useUpdateTeamspace,
} from '@/features/docs/api/teamspaces'
import { DocTreeItem, menuItemClass, type DocDropZone, type DocTreeDnd, type DocTreeFavorites } from './DocTreeItem'

interface DocTreeProps {
  workspaceId: string
  pages: PageSummary[] | undefined
  isPending: boolean
  isError: boolean
  onRetry: () => void
  activeId: string | null
  trashActive: boolean
  /** Owners and admins may delete teamspaces; members do not see the action. */
  canDeleteTeamspaces: boolean
  /** Asks for confirmation and trashes the page (shared with the page header menu). */
  onTrash: (pageId: string) => void
}

const collapsedKey = (workspaceId: string) => `orbit:docs_collapsed_spaces:${workspaceId}`
/** Collapse key of the Favorites section (stored with the space keys). */
const FAVORITES_SECTION = 'favorites'
/** dataTransfer type of a dragged teamspace header; page rows use `text/page-id`, so drops never mix them up. */
export const TEAMSPACE_DRAG_TYPE = 'application/x-orbit-teamspace'

function readCollapsed(workspaceId: string): ReadonlySet<string> {
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(collapsedKey(workspaceId)) ?? '[]')
    return new Set(Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [])
  } catch {
    return new Set()
  }
}

/** Header row of a space: a drop target ("move to the end of this space") that collapses its pages. */
const spaceRowClass =
  "group/space relative flex w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md whitespace-nowrap transition-colors select-none hover:bg-sidebar-accent/50 data-[drop=inside]:bg-primary/10 data-[drop=inside]:ring-1 data-[drop=inside]:ring-primary/25 data-[drop=inside]:ring-inset"
/** Before/after line on a teamspace header while another teamspace is dragged over it. */
const reorderLineClass =
  "data-[reorder=before]:before:absolute data-[reorder=before]:before:inset-x-1.5 data-[reorder=before]:before:-top-px data-[reorder=before]:before:h-0.5 data-[reorder=before]:before:rounded-[1px] data-[reorder=before]:before:bg-primary data-[reorder=before]:before:content-[''] data-[reorder=after]:after:absolute data-[reorder=after]:after:inset-x-1.5 data-[reorder=after]:after:-bottom-px data-[reorder=after]:after:h-0.5 data-[reorder=after]:after:rounded-[1px] data-[reorder=after]:after:bg-primary data-[reorder=after]:after:content-[''] data-[dragging]:opacity-60"
const hoverActionsClass = 'invisible relative z-20 flex shrink-0 items-center gap-0.5 group-hover/space:visible group-focus-within/space:visible'
const smallIconButton = 'size-[22px] text-muted-foreground/70'

export function DocTree({
  workspaceId,
  pages = [],
  isPending,
  isError,
  onRetry,
  activeId,
  trashActive,
  canDeleteTeamspaces,
  onTrash,
}: DocTreeProps) {
  const navigate = useNavigate()
  const teamspacesQuery = useTeamspaces(workspaceId)
  const teamspaces = teamspacesQuery.data ?? []
  const createPage = useCreatePage(workspaceId)
  const movePage = useMovePage(workspaceId)
  const createTeamspace = useCreateTeamspace(workspaceId)
  const updateTeamspace = useUpdateTeamspace(workspaceId)
  const deleteTeamspace = useDeleteTeamspace(workspaceId)
  const moveTeamspace = useMoveTeamspace(workspaceId)
  /** Explicit user toggles; anything not present falls back to "ancestor of the active page". */
  const [overrides, setOverrides] = useState<ReadonlyMap<string, boolean>>(new Map())
  const [collapsed, setCollapsed] = useState(() => readCollapsed(workspaceId))
  const [dragId, setDragId] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<{ id: string; zone: DocDropZone } | null>(null)
  const [dropSpace, setDropSpace] = useState<SpaceKey | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [renamingId, setRenamingId] = useState<string | null>(null)
  const [iconPickerId, setIconPickerId] = useState<string | null>(null)
  const [teamspaceDragId, setTeamspaceDragId] = useState<string | null>(null)
  const [teamspaceDropAt, setTeamspaceDropAt] = useState<{ id: string; zone: 'before' | 'after' } | null>(null)
  const headerRefs = useRef(new Map<string, HTMLElement>())
  const scrollRef = useRef<HTMLDivElement>(null)
  /** The last active page made visible in its space; later collapses by the user are left alone until it changes. */
  const [revealedId, setRevealedId] = useState<string | null>(null)
  const favoritesQuery = usePageFavorites(workspaceId)
  const toggleFavorite = useToggleFavorite(workspaceId)
  const moveFavorite = useMoveFavorite(workspaceId)
  /** Favorites keep their own expansion state; a page's row in its space is independent. */
  const [favoriteOverrides, setFavoriteOverrides] = useState<ReadonlyMap<string, boolean>>(new Map())
  const [favoriteDragId, setFavoriteDragId] = useState<string | null>(null)
  const [favoriteDropAt, setFavoriteDropAt] = useState<{ id: string; zone: DocDropZone } | null>(null)
  const [favoritesDrop, setFavoritesDrop] = useState(false)

  const ancestorIds = new Set(ancestorsOf(pages, activeId).map((page) => page.id))
  const isExpanded = (id: string) => overrides.get(id) ?? ancestorIds.has(id)

  const expand = (id: string) => {
    const next = new Map(overrides)
    next.set(id, true)
    setOverrides(next)
  }

  const toggle = (id: string) => {
    const next = new Map(overrides)
    next.set(id, !isExpanded(id))
    setOverrides(next)
  }

  const setSpaceCollapsed = (space: SpaceKey | typeof FAVORITES_SECTION, value: boolean) => {
    if (collapsed.has(space) === value) return
    const next = new Set(collapsed)
    if (value) next.add(space)
    else next.delete(space)
    setCollapsed(next)
  }

  useEffect(() => {
    try {
      window.localStorage.setItem(collapsedKey(workspaceId), JSON.stringify([...collapsed]))
    } catch {
      // Storage full or disabled: the section state just does not persist.
    }
  }, [collapsed, workspaceId])

  // Reveal the active page in its space (not in Favorites) whenever it changes: open its section and its closed
  // ancestor rows (only ever opening). Waits until the page is in the tree. Adjusted during render (no collapsed flash);
  // later collapses by the user are left alone until the active page changes again.
  if (activeId && activeId !== revealedId) {
    const reveal = revealPage(pages, activeId, (id) => overrides.get(id) === true)
    if (reveal) {
      setRevealedId(activeId)
      if (reveal.expand.length > 0) {
        const next = new Map(overrides)
        for (const id of reveal.expand) next.set(id, true)
        setOverrides(next)
      }
      setSpaceCollapsed(reveal.space, false)
    }
  }

  const loading = isPending || teamspacesQuery.isPending
  // Scroll the revealed row (in its space, not Favorites) into view once it is rendered.
  useEffect(() => {
    if (!revealedId || loading) return
    const row = scrollRef.current?.querySelector<HTMLElement>(`[data-page-id="${CSS.escape(revealedId)}"]:not([data-section])`)
    row?.scrollIntoView?.({ block: 'nearest' })
  }, [revealedId, loading])

  /** A sub-page (`parentId`), a root page of `space`, or (neither) a root page of the default teamspace. */
  const handleCreate = (parentId: string | null, space?: SpaceKey) => {
    if (createPage.isPending) return
    createPage.mutate(parentId ? { parent_id: parentId } : space ? spaceRequest(space) : {}, {
      onSuccess: (page) => {
        if (parentId) expand(parentId)
        setSpaceCollapsed(spaceKey(page), false)
        navigate(`/docs/${page.id}`)
      },
      onError: () => toast.error('Could not create the page. Try again.'),
    })
  }

  const submitMove = (dragged: PageSummary, move: MoveTarget) => {
    const from = spaceKey(dragged)
    const to = moveSpace(pages, dragged, move)
    movePage.mutate(
      { pageId: dragged.id, version: dragged.version, ...move },
      {
        onSuccess: () => {
          if (from !== to) toast.success(`Moved to ${spaceLabel(to, teamspaces)}`)
        },
        onError: (error) => {
          if (!isPageVersionConflict(error)) toast.error('Could not move the page.')
        },
      },
    )
  }

  const endDrag = () => {
    setDragId(null)
    setDropAt(null)
    setDropSpace(null)
  }

  const handleDrop = (targetId: string) => {
    const zone = dropAt?.id === targetId ? dropAt.zone : null
    const dragged = pages.find((page) => page.id === dragId)
    const move = dragged && zone ? dropToMove(pages, dragged.id, targetId, zone) : null
    if (dragged && move) {
      if (zone === 'inside') expand(targetId)
      submitMove(dragged, move)
    }
    endDrag()
  }

  const handleSpaceDrop = (space: SpaceKey) => {
    const dragged = pages.find((page) => page.id === dragId)
    const move = dragged ? dropOnSpace(pages, dragged.id, space) : null
    if (dragged && move) {
      setSpaceCollapsed(space, false)
      submitMove(dragged, move)
    }
    endDrag()
  }

  const dnd = {
    dragId,
    dropAt,
    canDropOn: (targetId: string) => Boolean(dragId) && canDropOn(pages, dragId!, targetId),
    onDragStart: setDragId,
    onDragEnd: endDrag,
    onDragOver: (id: string, zone: DocDropZone) => {
      if (dropSpace) setDropSpace(null)
      if (dropAt?.id !== id || dropAt.zone !== zone) setDropAt({ id, zone })
    },
    onDragLeave: (id: string) => {
      if (dropAt?.id === id) setDropAt(null)
    },
    onDrop: handleDrop,
  }

  // Favorites: only pages still in the tree (a trashed page drops out before the refetch lands).
  const pageById = new Map(pages.map((page) => [page.id, page]))
  const favoriteIds = (favoritesQuery.data ?? []).filter((id) => pageById.has(id))
  const favoriteSet = new Set(favoritesQuery.data ?? [])
  const favorites: DocTreeFavorites = {
    isFavorite: (id) => favoriteSet.has(id),
    onToggle: (id, favorite) =>
      toggleFavorite.mutate(
        { pageId: id, favorite },
        { onError: () => toast.error(favorite ? 'Could not add the page to favorites.' : 'Could not remove the page from favorites.') },
      ),
  }

  const endFavoriteDrag = () => {
    setFavoriteDragId(null)
    setFavoriteDropAt(null)
  }

  /** Favorites only reorder among themselves; sub-page rows and drags from other sections are ignored. */
  const favoriteDnd: DocTreeDnd = {
    dragId: favoriteDragId,
    dropAt: favoriteDropAt,
    canDropOn: (targetId) => favoriteDragId !== null && favoriteIds.includes(targetId),
    onDragStart: (id) => setFavoriteDragId(favoriteIds.includes(id) ? id : null),
    onDragEnd: endFavoriteDrag,
    onDragOver: (id, zone) => {
      if (favoriteDropAt?.id !== id || favoriteDropAt.zone !== zone) setFavoriteDropAt({ id, zone })
    },
    onDragLeave: (id) => {
      if (favoriteDropAt?.id === id) setFavoriteDropAt(null)
    },
    onDrop: (targetId) => {
      const zone = favoriteDropAt?.id === targetId ? favoriteDropAt.zone : null
      const position =
        favoriteDragId && (zone === 'before' || zone === 'after') ? favoriteDropIndex(favoriteIds, favoriteDragId, targetId, zone) : null
      if (favoriteDragId && position !== null) {
        moveFavorite.mutate({ pageId: favoriteDragId, position }, { onError: () => toast.error('Could not reorder favorites.') })
      }
      endFavoriteDrag()
    },
  }

  /** A page dragged from another section onto the Favorites header becomes a favorite (it does not move). */
  const favoritesHeaderDropProps = {
    'data-drop': favoritesDrop ? 'inside' : undefined,
    onDragOver: (e: DragEvent<HTMLElement>) => {
      if (!dragId) return
      e.preventDefault()
      e.stopPropagation()
      e.dataTransfer.dropEffect = 'move'
      if (dropAt) setDropAt(null)
      if (!favoritesDrop) setFavoritesDrop(true)
    },
    onDragLeave: (e: DragEvent<HTMLElement>) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFavoritesDrop(false)
    },
    onDrop: (e: DragEvent<HTMLElement>) => {
      e.preventDefault()
      e.stopPropagation()
      const id = dragId
      setFavoritesDrop(false)
      endDrag()
      if (id && !favoriteSet.has(id)) favorites.onToggle(id, true)
    },
  }

  /** Drag handlers for a space header or its empty row. */
  const spaceDropProps = (space: SpaceKey) => ({
    'data-drop': dropSpace === space ? 'inside' : undefined,
    onDragOver: (e: DragEvent<HTMLElement>) => {
      if (!dragId) return
      e.preventDefault()
      e.stopPropagation()
      e.dataTransfer.dropEffect = 'move'
      if (dropAt) setDropAt(null)
      if (dropSpace !== space) setDropSpace(space)
    },
    onDragLeave: (e: DragEvent<HTMLElement>) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null) && dropSpace === space) setDropSpace(null)
    },
    onDrop: (e: DragEvent<HTMLElement>) => {
      e.preventDefault()
      e.stopPropagation()
      handleSpaceDrop(space)
    },
  })

  const renderPages = (space: SpaceKey, depth: number) => {
    const roots = rootsOf(pages, space)
    if (roots.length === 0) {
      return (
        <div
          className="flex h-8 min-w-0 items-center rounded-md pr-2 text-[13px] text-muted-foreground/60 data-[drop=inside]:bg-primary/10 data-[drop=inside]:ring-1 data-[drop=inside]:ring-primary/25 data-[drop=inside]:ring-inset"
          style={{ paddingLeft: 6 + depth * 16 + 26 }}
          data-empty-space={space}
          {...spaceDropProps(space)}
        >
          No pages inside
        </div>
      )
    }
    return roots.map((page) => (
      <DocTreeItem
        key={page.id}
        page={page}
        pages={pages}
        depth={depth}
        activeId={activeId}
        dnd={dnd}
        isExpanded={isExpanded}
        onToggle={toggle}
        onOpen={(id) => navigate(`/docs/${id}`)}
        onCreateChild={(parentId) => handleCreate(parentId)}
        onTrash={onTrash}
        favorites={favorites}
      />
    ))
  }

  const favoritesOpen = !collapsed.has(FAVORITES_SECTION)
  const isFavoriteExpanded = (id: string) => favoriteOverrides.get(id) ?? false
  const toggleFavoriteRow = (id: string) => {
    const next = new Map(favoriteOverrides)
    next.set(id, !isFavoriteExpanded(id))
    setFavoriteOverrides(next)
  }

  const requestDelete = async (teamspace: Teamspace) => {
    const confirmed = await confirmAction({
      title: `Delete “${teamspace.name}”?`,
      description: 'Only an empty teamspace can be deleted. Pages in its trash are deleted with it.',
      confirmLabel: 'Delete teamspace',
      danger: true,
    })
    if (!confirmed) return
    deleteTeamspace.mutate(
      { teamspaceId: teamspace.id, version: teamspace.version },
      {
        onSuccess: () => toast.success(`Deleted “${teamspace.name}”`),
        onError: (error) => toast.error(teamspaceDeleteError(error)),
      },
    )
  }

  const commitRename = (teamspace: Teamspace, value: string) => {
    setRenamingId(null)
    const name = value.trim()
    if (!name || name === teamspace.name) return
    updateTeamspace.mutate(
      { teamspaceId: teamspace.id, version: teamspace.version, name },
      { onError: () => toast.error('Could not rename the teamspace.') },
    )
  }

  const changeIcon = (teamspace: Teamspace, icon: string | null) => {
    setIconPickerId(null)
    if (icon === teamspace.icon) return
    updateTeamspace.mutate(
      { teamspaceId: teamspace.id, version: teamspace.version, icon },
      { onError: () => toast.error('Could not change the teamspace icon.') },
    )
  }

  /** Moves a teamspace to `position` (index without it); the first teamspace is the default. */
  const submitTeamspaceMove = (teamspace: Teamspace, position: number) => {
    if (moveTeamspace.isPending) return
    moveTeamspace.mutate(
      { teamspaceId: teamspace.id, version: teamspace.version, position },
      { onError: () => toast.error('Could not reorder the teamspaces.') },
    )
  }

  const endTeamspaceDrag = () => {
    setTeamspaceDragId(null)
    setTeamspaceDropAt(null)
  }

  /** Header drag handlers: a dragged teamspace reorders (before/after this header); a dragged page moves into the space. */
  const teamspaceHeaderProps = (teamspace: Teamspace) => {
    const pageDrop = spaceDropProps(teamspaceSpace(teamspace.id))
    const reorderable = teamspaceDragId !== null && teamspaceDragId !== teamspace.id
    return {
      ...pageDrop,
      draggable: renamingId !== teamspace.id,
      'data-dragging': teamspaceDragId === teamspace.id || undefined,
      'data-reorder': teamspaceDropAt?.id === teamspace.id ? teamspaceDropAt.zone : undefined,
      onDragStart: (e: DragEvent<HTMLElement>) => {
        e.dataTransfer.effectAllowed = 'move'
        e.dataTransfer.setData(TEAMSPACE_DRAG_TYPE, teamspace.id)
        setTeamspaceDragId(teamspace.id)
      },
      onDragEnd: endTeamspaceDrag,
      onDragOver: (e: DragEvent<HTMLElement>) => {
        if (teamspaceDragId === null) return pageDrop.onDragOver(e)
        if (!reorderable) return
        e.preventDefault()
        e.stopPropagation()
        e.dataTransfer.dropEffect = 'move'
        const rect = e.currentTarget.getBoundingClientRect()
        const zone = e.clientY - rect.top < rect.height / 2 ? 'before' : 'after'
        if (teamspaceDropAt?.id !== teamspace.id || teamspaceDropAt.zone !== zone) setTeamspaceDropAt({ id: teamspace.id, zone })
      },
      onDragLeave: (e: DragEvent<HTMLElement>) => {
        if (teamspaceDragId === null) return pageDrop.onDragLeave(e)
        if (!e.currentTarget.contains(e.relatedTarget as Node | null) && teamspaceDropAt?.id === teamspace.id) setTeamspaceDropAt(null)
      },
      onDrop: (e: DragEvent<HTMLElement>) => {
        if (teamspaceDragId === null) return pageDrop.onDrop(e)
        e.preventDefault()
        e.stopPropagation()
        const dragged = teamspaces.find((item) => item.id === teamspaceDragId)
        const zone = teamspaceDropAt?.id === teamspace.id ? teamspaceDropAt.zone : null
        const position =
          dragged && zone
            ? teamspaceDropIndex(
                teamspaces.map((item) => item.id),
                dragged.id,
                teamspace.id,
                zone,
              )
            : null
        endTeamspaceDrag()
        if (dragged && position !== null) submitTeamspaceMove(dragged, position)
      },
    }
  }

  const iconPickerTeamspace = teamspaces.find((teamspace) => teamspace.id === iconPickerId) ?? null
  const failed = isError || teamspacesQuery.isError

  return (
    <section className="flex h-full min-h-0 w-[260px] min-w-0 shrink-0 flex-col border-r border-border bg-background max-[899px]:w-full max-[899px]:border-r-0 max-[899px]:group-data-[view=doc]/docs:hidden">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:border-b-0">
        <span className="truncate text-[13px] font-semibold text-foreground">Documents</span>
        <span className="flex-1" />
        <DropdownMenu>
          <DropdownMenuTrigger
            render={<Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Documents options" />}
          >
            <Ellipsis className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-auto min-w-44">
            <DropdownMenuItem className={menuItemClass} onClick={() => navigate('/docs/import')}>
              <Import className="size-[14px]" />
              Import from Notion
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label="New page"
          disabled={createPage.isPending}
          onClick={() => handleCreate(null)}
        >
          <Plus className="size-4" />
        </Button>
      </div>
      <div ref={scrollRef} className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {loading ? (
          <div className="flex flex-1 items-center justify-center p-6">
            <Spinner className="text-muted-foreground" />
          </div>
        ) : failed ? (
          <div className="flex flex-1 flex-col p-2">
            <EmptyState
              icon={FileText}
              size="sm"
              title="Pages unavailable"
              description="The server could not load the page tree."
              action={
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => {
                    onRetry()
                    if (teamspacesQuery.isError) void teamspacesQuery.refetch()
                  }}
                >
                  Retry
                </Button>
              }
            />
          </div>
        ) : (
          <nav className="flex flex-col gap-px p-2" role="tree" aria-label="Pages">
            {favoriteIds.length > 0 ? (
              <div className="mb-3 flex flex-col gap-px" role="group" aria-label="Favorites">
                <SectionLabel
                  label="Favorites"
                  icon={<Star className="size-3.5" />}
                  section={FAVORITES_SECTION}
                  expanded={favoritesOpen}
                  onToggle={() => setSpaceCollapsed(FAVORITES_SECTION, favoritesOpen)}
                  dropProps={favoritesHeaderDropProps}
                  action={null}
                />
                {favoritesOpen
                  ? favoriteIds.map((id) => (
                      <DocTreeItem
                        key={id}
                        page={pageById.get(id)!}
                        pages={pages}
                        depth={0}
                        activeId={activeId}
                        dnd={favoriteDnd}
                        isExpanded={isFavoriteExpanded}
                        onToggle={toggleFavoriteRow}
                        onOpen={(pageId) => navigate(`/docs/${pageId}`)}
                        onCreateChild={(parentId) => handleCreate(parentId)}
                        onTrash={onTrash}
                        favorites={favorites}
                        reorderOnly
                        section={FAVORITES_SECTION}
                      />
                    ))
                  : null}
              </div>
            ) : null}
            <SectionLabel
              label="Teamspaces"
              action={
                <Button type="button" variant="ghost" size="icon-sm" className={smallIconButton} aria-label="New teamspace" onClick={() => setCreateOpen(true)}>
                  <Plus className="size-[13px]" />
                </Button>
              }
            />
            {teamspaces.map((teamspace, index) => {
              const space = teamspaceSpace(teamspace.id)
              const open = !collapsed.has(space)
              return (
                <div key={teamspace.id} className="flex flex-col gap-px" role="group" aria-label={teamspace.name}>
                  {/* The dragged header stays mounted (faded via data-dragging); the browser cancels a drag whose source
                      leaves the DOM. */}
                  <div
                    ref={(element) => {
                      if (element) headerRefs.current.set(teamspace.id, element)
                      else headerRefs.current.delete(teamspace.id)
                    }}
                    className={cn(spaceRowClass, reorderLineClass, 'h-8 px-1.5 text-[13px] font-medium text-sidebar-foreground')}
                    role="treeitem"
                    aria-level={1}
                    aria-expanded={open}
                    aria-selected={false}
                    data-space={space}
                    onClick={() => setSpaceCollapsed(space, open)}
                    {...teamspaceHeaderProps(teamspace)}
                  >
                    <span className="relative inline-flex size-5 shrink-0 items-center justify-center text-muted-foreground/70">
                      <span className="inline-flex group-hover/space:invisible">
                        {teamspace.icon ? <Emoji value={teamspace.icon} size={15} /> : <Users className="size-[15px]" />}
                      </span>
                      <ChevronRight
                        className={cn('invisible absolute size-3 transition-transform duration-[120ms] group-hover/space:visible', open && 'rotate-90')}
                        aria-hidden="true"
                      />
                    </span>
                    {renamingId === teamspace.id ? (
                      <RenameInput value={teamspace.name} onCommit={(value) => commitRename(teamspace, value)} onCancel={() => setRenamingId(null)} />
                    ) : (
                      <span className="flex min-w-0 flex-1 items-center gap-1.5">
                        <span className="min-w-0 truncate">{teamspace.name}</span>
                        {teamspace.is_default && teamspaces.length > 1 ? (
                          <span
                            className="shrink-0 rounded-sm bg-muted px-1 text-[10px] leading-4 font-medium text-muted-foreground"
                            title="New pages go to this teamspace by default. Move another teamspace to the top to change it."
                          >
                            Default
                          </span>
                        ) : null}
                      </span>
                    )}
                    <span className={hoverActionsClass} onClick={(e) => e.stopPropagation()}>
                      <TeamspaceMenu
                        teamspace={teamspace}
                        canDelete={canDeleteTeamspaces}
                        canMoveUp={index > 0}
                        canMoveDown={index < teamspaces.length - 1}
                        onRename={() => setRenamingId(teamspace.id)}
                        onChangeIcon={() => setIconPickerId(teamspace.id)}
                        onMove={(offset) => submitTeamspaceMove(teamspace, index + offset)}
                        onDelete={() => void requestDelete(teamspace)}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        className={smallIconButton}
                        aria-label={`Add page to ${teamspace.name}`}
                        onClick={() => handleCreate(null, space)}
                      >
                        <Plus className="size-[13px]" />
                      </Button>
                    </span>
                  </div>
                  {open ? renderPages(space, 1) : null}
                </div>
              )
            })}
            <div className="mt-3 flex flex-col gap-px" role="group" aria-label="Private">
              <SectionLabel
                label="Private"
                icon={<Lock className="size-3.5" />}
                space={PRIVATE_SPACE}
                expanded={!collapsed.has(PRIVATE_SPACE)}
                onToggle={() => setSpaceCollapsed(PRIVATE_SPACE, !collapsed.has(PRIVATE_SPACE))}
                dropProps={spaceDropProps(PRIVATE_SPACE)}
                action={
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    className={smallIconButton}
                    aria-label="Add private page"
                    onClick={() => handleCreate(null, PRIVATE_SPACE)}
                  >
                    <Plus className="size-[13px]" />
                  </Button>
                }
              />
              {collapsed.has(PRIVATE_SPACE) ? null : renderPages(PRIVATE_SPACE, 0)}
            </div>
          </nav>
        )}
      </div>
      <div className="shrink-0 border-t border-border p-2">
        <NavLink
          to="/docs/trash"
          className={cn(
            'flex h-8 w-full min-w-0 items-center gap-2 rounded-md px-2 text-[13px] font-medium text-muted-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-foreground',
            trashActive && 'bg-sidebar-accent text-sidebar-accent-foreground',
          )}
        >
          <Trash2 className="size-[15px] shrink-0" />
          Trash
        </NavLink>
      </div>
      {iconPickerTeamspace ? (
        <Popover open modal={false} onOpenChange={(open) => (open ? undefined : setIconPickerId(null))}>
          <PopoverContent
            align="start"
            anchor={() => headerRefs.current.get(iconPickerTeamspace.id) ?? null}
            className="w-auto gap-0 rounded-xl border border-border bg-popover p-0 text-foreground shadow-xl ring-0"
            aria-label={`Icon for ${iconPickerTeamspace.name}`}
          >
            <EmojiPicker
              onPick={(emoji) => changeIcon(iconPickerTeamspace, emoji)}
              onRemove={iconPickerTeamspace.icon ? () => changeIcon(iconPickerTeamspace, null) : undefined}
            />
          </PopoverContent>
        </Popover>
      ) : null}
      {createOpen ? (
        <CreateTeamspaceDialog
          pending={createTeamspace.isPending}
          onClose={() => setCreateOpen(false)}
          onCreate={(name) =>
            createTeamspace.mutate(
              { name },
              {
                onSuccess: (teamspace) => {
                  setCreateOpen(false)
                  setSpaceCollapsed(teamspaceSpace(teamspace.id), false)
                },
                onError: () => toast.error('Could not create the teamspace.'),
              },
            )
          }
        />
      ) : null}
    </section>
  )
}

/** Small muted label row ("Favorites", "Teamspaces", "Private"); Favorites and Private collapse their pages and take drops. */
function SectionLabel({
  label,
  icon,
  action,
  space,
  section,
  expanded,
  onToggle,
  dropProps,
}: {
  label: string
  icon?: ReactNode
  action: ReactNode
  space?: SpaceKey
  section?: string
  expanded?: boolean
  onToggle?: () => void
  dropProps?: object
}) {
  const interactive = Boolean(onToggle)
  return (
    <div
      className={cn(spaceRowClass, 'h-7 px-2 text-xs font-medium text-muted-foreground', !interactive && 'cursor-default hover:bg-transparent')}
      role={interactive ? 'treeitem' : undefined}
      aria-level={interactive ? 1 : undefined}
      aria-expanded={interactive ? expanded : undefined}
      aria-selected={interactive ? false : undefined}
      data-space={space}
      data-section={section}
      onClick={onToggle}
      {...dropProps}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className={hoverActionsClass} onClick={(e) => e.stopPropagation()}>
        {action}
      </span>
    </div>
  )
}

function TeamspaceMenu({
  teamspace,
  canDelete,
  canMoveUp,
  canMoveDown,
  onRename,
  onChangeIcon,
  onMove,
  onDelete,
}: {
  teamspace: Teamspace
  canDelete: boolean
  canMoveUp: boolean
  canMoveDown: boolean
  onRename: () => void
  onChangeIcon: () => void
  /** -1 = up, +1 = down (keyboard alternative to dragging the header). */
  onMove: (offset: -1 | 1) => void
  onDelete: () => void
}) {
  // Rename and Change icon move focus into the input/picker; the closing menu must not pull it back to the trigger.
  const keepFocus = useRef(false)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={<Button type="button" variant="ghost" size="icon-sm" className={smallIconButton} aria-label={`${teamspace.name} options`} />}
      >
        <Ellipsis className="size-[13px]" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className="w-auto min-w-40"
        finalFocus={() => {
          const skip = keepFocus.current
          keepFocus.current = false
          return !skip
        }}
      >
        <DropdownMenuItem
          className={menuItemClass}
          onClick={() => {
            keepFocus.current = true
            onRename()
          }}
        >
          <Pencil className="size-[14px]" />
          Rename
        </DropdownMenuItem>
        <DropdownMenuItem
          className={menuItemClass}
          onClick={() => {
            keepFocus.current = true
            onChangeIcon()
          }}
        >
          <Smile className="size-[14px]" />
          Change icon
        </DropdownMenuItem>
        <DropdownMenuItem className={menuItemClass} disabled={!canMoveUp} onClick={() => onMove(-1)}>
          <ArrowUp className="size-[14px]" />
          Move up
        </DropdownMenuItem>
        <DropdownMenuItem className={menuItemClass} disabled={!canMoveDown} onClick={() => onMove(1)}>
          <ArrowDown className="size-[14px]" />
          Move down
        </DropdownMenuItem>
        {canDelete ? (
          <DropdownMenuItem className={menuItemClass} data-danger="true" onClick={onDelete}>
            <Trash2 className="size-[14px]" />
            Delete teamspace
          </DropdownMenuItem>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function RenameInput({ value, onCommit, onCancel }: { value: string; onCommit: (value: string) => void; onCancel: () => void }) {
  const done = useRef(false)
  const finish = (commit: boolean, next: string) => {
    if (done.current) return
    done.current = true
    if (commit) onCommit(next)
    else onCancel()
  }
  return (
    <Input
      autoFocus
      defaultValue={value}
      maxLength={100}
      aria-label="Teamspace name"
      className="h-6 min-w-0 flex-1 rounded-sm px-1.5 py-0 text-[13px] md:text-[13px]"
      onFocus={(e) => e.currentTarget.select()}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation()
        if (e.key === 'Enter') finish(true, e.currentTarget.value)
        if (e.key === 'Escape') finish(false, value)
      }}
      onBlur={(e) => finish(true, e.currentTarget.value)}
    />
  )
}

function CreateTeamspaceDialog({ pending, onClose, onCreate }: { pending: boolean; onClose: () => void; onCreate: (name: string) => void }) {
  const [name, setName] = useState('')
  const submit = (e: FormEvent) => {
    e.preventDefault()
    const trimmed = name.trim()
    if (trimmed && !pending) onCreate(trimmed)
  }
  return (
    <Dialog open onOpenChange={(open) => (open ? undefined : onClose())}>
      <DialogContent>
        <form className="grid gap-4" onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>New teamspace</DialogTitle>
            <DialogDescription>Everyone in this workspace can see and edit its pages.</DialogDescription>
          </DialogHeader>
          <Input autoFocus value={name} maxLength={100} placeholder="Teamspace name" aria-label="Teamspace name" onChange={(e) => setName(e.target.value)} />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim() || pending}>
              Create teamspace
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
