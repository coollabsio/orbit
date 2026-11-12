import { ChevronRight, Copy, MoreH as Ellipsis, DocumentText as FileText, Grid, Star, Trash as Trash2 } from 'reicon-react'
import type { PageKind, PageSummary } from '@orbit/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Emoji } from '@/components/common/Emoji'
import { useDocPageActions } from '@/features/docs/pageActions'
import { childrenOf, pageTitle, type DropZone } from '@/features/docs/pageTree'
import { TreeCreateMenu, TreeIconButton, TreeRowActions } from './TreeRow'

export type DocDropZone = DropZone

export interface DocTreeDnd {
  dragId: string | null
  dropAt: { id: string; zone: DocDropZone } | null
  canDropOn: (targetId: string) => boolean
  onDragStart: (id: string) => void
  onDragEnd: () => void
  onDragOver: (id: string, zone: DocDropZone) => void
  onDragLeave: (id: string) => void
  onDrop: (id: string) => void
}

/** The row menu's "Add to / Remove from favorites" item. */
export interface DocTreeFavorites {
  isFavorite: (id: string) => boolean
  onToggle: (page: DocTreeItemPage, favorite: boolean) => void
}

/** What a row shows of its page: a favorite outside the tree (a database row) has no tree summary. */
export type DocTreeItemPage = Pick<PageSummary, 'id' | 'kind' | 'title' | 'icon'>

interface DocTreeItemProps {
  page: DocTreeItemPage
  pages: PageSummary[]
  depth: number
  activeId: string | null
  dnd: DocTreeDnd
  isExpanded: (id: string) => boolean
  onToggle: (id: string) => void
  onOpen: (id: string) => void
  onCreateChild: (parentId: string, kind: PageKind) => void
  onTrash: (id: string) => void
  favorites?: DocTreeFavorites
  /** Rows that can only be reordered (Favorites): no "inside" drop zone. */
  reorderOnly?: boolean
  /** Marks the rows of a sidebar section (`data-section`), e.g. "favorites". */
  section?: string
  /** Muted text after the title (the database of a favorite row). */
  hint?: string | null
}

/** Pointer position → drop zone: edges reorder among siblings, the middle nests inside. */
function zoneAt(element: HTMLElement, clientY: number, reorderOnly = false): DocDropZone {
  const rect = element.getBoundingClientRect()
  const y = clientY - rect.top
  if (reorderOnly) return y < rect.height / 2 ? 'before' : 'after'
  if (y < rect.height * 0.25) return 'before'
  if (y > rect.height * 0.75) return 'after'
  return 'inside'
}

export function DocTreeItem({
  page,
  pages,
  depth,
  activeId,
  dnd,
  isExpanded,
  onToggle,
  onOpen,
  onCreateChild,
  onTrash,
  favorites,
  reorderOnly = false,
  section,
  hint,
}: DocTreeItemProps) {
  const isDatabase = page.kind === 'database'
  // A database's rows list in the database, not in the tree.
  const children = isDatabase ? [] : childrenOf(pages, page.id)
  const expanded = isExpanded(page.id)
  const favorite = favorites?.isFavorite(page.id) ?? false
  const actions = useDocPageActions()

  return (
    <>
      {/* The drag source stays mounted while dragging (faded via data-dragging); the browser cancels a drag whose
          source leaves the DOM. */}
      <div
        className="group/row relative flex h-8 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-[0.8125rem] font-medium whitespace-nowrap text-sidebar-foreground transition-colors outline-none select-none hover:bg-sidebar-accent/50 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset data-[active]:bg-sidebar-accent data-[active]:text-sidebar-accent-foreground data-[dragging]:bg-foreground/5 data-[dragging]:text-foreground data-[dragging]:opacity-60 data-[drop=inside]:bg-primary/10 data-[drop=inside]:ring-1 data-[drop=inside]:ring-primary/25 data-[drop=inside]:ring-inset data-[drop=before]:before:absolute data-[drop=before]:before:inset-x-1.5 data-[drop=before]:before:-top-px data-[drop=before]:before:h-0.5 data-[drop=before]:before:rounded-[1px] data-[drop=before]:before:bg-primary data-[drop=before]:before:content-[''] data-[drop=after]:after:absolute data-[drop=after]:after:inset-x-1.5 data-[drop=after]:after:-bottom-px data-[drop=after]:after:h-0.5 data-[drop=after]:after:rounded-[1px] data-[drop=after]:after:bg-primary data-[drop=after]:after:content-['']"
        data-active={page.id === activeId || undefined}
        data-dragging={dnd.dragId === page.id || undefined}
        data-drop={dnd.dropAt?.id === page.id ? dnd.dropAt.zone : undefined}
        data-page-id={page.id}
        data-section={section}
        style={{ paddingLeft: 6 + depth * 16 }}
        draggable
        role="treeitem"
        aria-selected={page.id === activeId}
        aria-expanded={children.length > 0 ? expanded : undefined}
        aria-level={depth + 1}
        tabIndex={0}
        onClick={() => onOpen(page.id)}
        onKeyDown={(e) => {
          // Enter/Space open the page; keys from the row's own buttons (and their menus) stay theirs.
          if (e.target !== e.currentTarget || (e.key !== 'Enter' && e.key !== ' ')) return
          e.preventDefault()
          onOpen(page.id)
        }}
        onDragStart={(e) => {
          e.dataTransfer.effectAllowed = 'move'
          e.dataTransfer.setData('text/page-id', page.id)
          dnd.onDragStart(page.id)
        }}
        onDragEnd={dnd.onDragEnd}
        onDragOver={(e) => {
          if (!dnd.dragId || !dnd.canDropOn(page.id)) return
          e.preventDefault()
          e.stopPropagation()
          e.dataTransfer.dropEffect = 'move'
          // A page dropped inside a database becomes a row; a database cannot.
          const draggedDatabase = pages.find((item) => item.id === dnd.dragId)?.kind === 'database'
          dnd.onDragOver(page.id, zoneAt(e.currentTarget, e.clientY, reorderOnly || (isDatabase && draggedDatabase)))
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) dnd.onDragLeave(page.id)
        }}
        onDrop={(e) => {
          e.preventDefault()
          e.stopPropagation()
          dnd.onDrop(page.id)
        }}
      >
        {children.length > 0 ? (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="size-5 text-muted-foreground/70 [&>svg]:transition-transform [&>svg]:duration-[120ms] data-[expanded=true]:[&>svg]:rotate-90"
            data-expanded={expanded}
            aria-label={expanded ? 'Collapse' : 'Expand'}
            onClick={(e) => {
              e.stopPropagation()
              onToggle(page.id)
            }}
          >
            <ChevronRight className="size-3" />
          </Button>
        ) : (
          <span className="inline-flex size-5 shrink-0" aria-hidden="true" />
        )}
        <span className="inline-flex w-[18px] shrink-0 items-center justify-center text-sm leading-none text-muted-foreground/70">
          {page.icon ? <Emoji value={page.icon} /> : isDatabase ? <Grid className="size-[15px]" /> : <FileText className="size-[15px]" />}
        </span>
        <span className="min-w-0 flex-1 truncate">
          {pageTitle(page)}
          {hint ? <span className="ml-1.5 font-normal text-muted-foreground/70">{hint}</span> : null}
        </span>
        <TreeRowActions>
          <DropdownMenu>
            <DropdownMenuTrigger render={<TreeIconButton tip="Page options" aria-label="Page options" />}>
              <Ellipsis className="size-[13px]" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-auto min-w-36">
              {favorites ? (
                <DropdownMenuItem onClick={() => favorites.onToggle(page, !favorite)}>
                  <Star className="size-[14px]" weight={favorite ? 'Filled' : 'Outline'} />
                  {favorite ? 'Remove from favorites' : 'Add to favorites'}
                </DropdownMenuItem>
              ) : null}
              {actions ? (
                <DropdownMenuItem onClick={() => actions.duplicate(page.id, false)}>
                  <Copy className="size-[14px]" />
                  Duplicate
                </DropdownMenuItem>
              ) : null}
              {actions && children.length > 0 ? (
                <DropdownMenuItem onClick={() => actions.duplicate(page.id, true)}>
                  <Copy className="size-[14px]" />
                  Duplicate with sub-pages
                </DropdownMenuItem>
              ) : null}
              {/* Not variant="destructive": the preset menu popup recolors those items to the accent color. */}
              <DropdownMenuItem className="text-destructive focus:text-destructive" onClick={() => onTrash(page.id)}>
                <Trash2 className="size-[14px]" />
                Move to trash
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          {/* The server refuses pages under a database: its rows are created in the database. */}
          {isDatabase ? null : <TreeCreateMenu tip="Add child page" label="Add child page" onCreate={(kind) => onCreateChild(page.id, kind)} />}
        </TreeRowActions>
      </div>
      {expanded
        ? children.map((child) => (
            <DocTreeItem
              key={child.id}
              page={child}
              pages={pages}
              depth={depth + 1}
              activeId={activeId}
              dnd={dnd}
              isExpanded={isExpanded}
              onToggle={onToggle}
              onOpen={onOpen}
              onCreateChild={onCreateChild}
              onTrash={onTrash}
              favorites={favorites}
              reorderOnly={reorderOnly}
              section={section}
            />
          ))
        : null}
    </>
  )
}
