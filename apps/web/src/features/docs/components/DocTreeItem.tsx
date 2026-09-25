import { ChevronRight, MoreH as Ellipsis, DocumentText as FileText, Add as Plus, Star, Trash as Trash2 } from 'reicon-react'
import type { PageSummary } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Emoji } from '@/components/common/Emoji'
import { childrenOf, pageTitle, type DropZone } from '@/features/docs/pageTree'

// data-danger (not variant="destructive"): the preset menu popup forces destructive items to the accent color.
export const menuItemClass =
  'min-h-8 gap-2 rounded-md px-2 py-1.5 text-sm leading-5 text-foreground focus:bg-muted data-[danger=true]:text-destructive data-[danger=true]:focus:bg-muted data-[danger=true]:focus:text-destructive'

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
  onToggle: (id: string, favorite: boolean) => void
}

interface DocTreeItemProps {
  page: PageSummary
  pages: PageSummary[]
  depth: number
  activeId: string | null
  dnd: DocTreeDnd
  isExpanded: (id: string) => boolean
  onToggle: (id: string) => void
  onOpen: (id: string) => void
  onCreateChild: (parentId: string) => void
  onTrash: (id: string) => void
  favorites?: DocTreeFavorites
  /** Rows that can only be reordered (Favorites): no "inside" drop zone. */
  reorderOnly?: boolean
  /** Marks the rows of a sidebar section (`data-section`), e.g. "favorites". */
  section?: string
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
}: DocTreeItemProps) {
  const children = childrenOf(pages, page.id)
  const expanded = isExpanded(page.id)
  const favorite = favorites?.isFavorite(page.id) ?? false

  return (
    <>
      {/* The drag source stays mounted while dragging (faded via data-dragging); the browser cancels a drag whose
          source leaves the DOM. */}
      <div
        className="group/row relative flex h-8 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors select-none hover:bg-sidebar-accent/50 data-[active]:bg-sidebar-accent data-[active]:text-sidebar-accent-foreground data-[dragging]:bg-foreground/5 data-[dragging]:text-foreground data-[dragging]:opacity-60 data-[drop=inside]:bg-primary/10 data-[drop=inside]:ring-1 data-[drop=inside]:ring-primary/25 data-[drop=inside]:ring-inset data-[drop=before]:before:absolute data-[drop=before]:before:inset-x-1.5 data-[drop=before]:before:-top-px data-[drop=before]:before:h-0.5 data-[drop=before]:before:rounded-[1px] data-[drop=before]:before:bg-primary data-[drop=before]:before:content-[''] data-[drop=after]:after:absolute data-[drop=after]:after:inset-x-1.5 data-[drop=after]:after:-bottom-px data-[drop=after]:after:h-0.5 data-[drop=after]:after:rounded-[1px] data-[drop=after]:after:bg-primary data-[drop=after]:after:content-['']"
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
        onClick={() => onOpen(page.id)}
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
          dnd.onDragOver(page.id, zoneAt(e.currentTarget, e.clientY, reorderOnly))
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
            className="inline-flex size-5 shrink-0 items-center justify-center rounded-md border-0 text-muted-foreground/70 hover:bg-muted hover:text-foreground dark:hover:bg-muted [&>svg]:transition-transform [&>svg]:duration-[120ms] data-[expanded=true]:[&>svg]:rotate-90"
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
          {page.icon ? <Emoji value={page.icon} size={15} /> : <FileText className="size-[15px]" />}
        </span>
        <span className="min-w-0 flex-1 truncate">{pageTitle(page)}</span>
        <span
          className="invisible relative z-20 flex shrink-0 items-center gap-0.5 group-hover/row:visible group-focus-within/row:visible"
          onClick={(e) => e.stopPropagation()}
        >
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="size-[22px] text-muted-foreground/70"
            aria-label="Add child page"
            onClick={() => onCreateChild(page.id)}
          >
            <Plus className="size-[13px]" />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button type="button" variant="ghost" size="icon-sm" className="size-[22px] text-muted-foreground/70" aria-label="Page options" />
              }
            >
              <Ellipsis className="size-[13px]" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-auto min-w-36">
              {favorites ? (
                <DropdownMenuItem className={menuItemClass} onClick={() => favorites.onToggle(page.id, !favorite)}>
                  <Star className="size-[14px]" weight={favorite ? 'Filled' : 'Outline'} />
                  {favorite ? 'Remove from favorites' : 'Add to favorites'}
                </DropdownMenuItem>
              ) : null}
              <DropdownMenuItem className={menuItemClass} data-danger="true" onClick={() => onTrash(page.id)}>
                <Trash2 className="size-[14px]" />
                Move to trash
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
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
