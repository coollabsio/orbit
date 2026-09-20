import { ChevronRight, Ellipsis, FileText, Plus, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Emoji } from '@/components/common/Emoji'
import type { Doc } from '@/mock/types'
import { childrenOf } from '@/features/docs/docsLib'

// data-danger (not variant="destructive"): the preset menu popup forces destructive items to the accent color.
const menuItemClass =
  'min-h-8 gap-2 rounded-md px-2 py-1.5 text-sm leading-5 text-foreground focus:bg-muted data-[danger=true]:text-destructive data-[danger=true]:focus:bg-muted data-[danger=true]:focus:text-destructive'

export type DocDropZone = 'before' | 'after' | 'inside'

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

interface DocTreeItemProps {
  doc: Doc
  docs: Doc[]
  depth: number
  activeId: string | null
  dnd: DocTreeDnd
  isExpanded: (id: string) => boolean
  onToggle: (id: string) => void
  onOpen: (id: string) => void
  onCreateChild: (parentId: string) => void
  onDelete: (id: string) => void
}

/** Pointer position → drop zone: edges reorder among siblings, the middle nests inside. */
function zoneAt(element: HTMLElement, clientY: number): DocDropZone {
  const rect = element.getBoundingClientRect()
  const y = clientY - rect.top
  if (y < rect.height * 0.25) return 'before'
  if (y > rect.height * 0.75) return 'after'
  return 'inside'
}

export function DocTreeItem({
  doc,
  docs,
  depth,
  activeId,
  dnd,
  isExpanded,
  onToggle,
  onOpen,
  onCreateChild,
  onDelete,
}: DocTreeItemProps) {
  const children = childrenOf(docs, doc.id)
  const expanded = isExpanded(doc.id)

  return (
    <>
      <div
        className="group/row relative flex h-8 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors select-none hover:bg-sidebar-accent/50 data-[active]:bg-sidebar-accent data-[active]:text-sidebar-accent-foreground data-[dragging]:bg-foreground/5 data-[dragging]:text-foreground data-[drop=inside]:bg-primary/10 data-[drop=inside]:ring-1 data-[drop=inside]:ring-primary/25 data-[drop=inside]:ring-inset data-[drop=before]:before:absolute data-[drop=before]:before:inset-x-1.5 data-[drop=before]:before:-top-px data-[drop=before]:before:h-0.5 data-[drop=before]:before:rounded-[1px] data-[drop=before]:before:bg-primary data-[drop=before]:before:content-[''] data-[drop=after]:after:absolute data-[drop=after]:after:inset-x-1.5 data-[drop=after]:after:-bottom-px data-[drop=after]:after:h-0.5 data-[drop=after]:after:rounded-[1px] data-[drop=after]:after:bg-primary data-[drop=after]:after:content-['']"
        data-active={doc.id === activeId || undefined}
        data-dragging={dnd.dragId === doc.id || undefined}
        data-drop={dnd.dropAt?.id === doc.id ? dnd.dropAt.zone : undefined}
        style={{ paddingLeft: 6 + depth * 16 }}
        draggable
        onClick={() => onOpen(doc.id)}
        onDragStart={(e) => {
          e.dataTransfer.effectAllowed = 'move'
          e.dataTransfer.setData('text/doc-id', doc.id)
          dnd.onDragStart(doc.id)
        }}
        onDragEnd={dnd.onDragEnd}
        onDragOver={(e) => {
          if (!dnd.dragId || !dnd.canDropOn(doc.id)) return
          e.preventDefault()
          e.stopPropagation()
          e.dataTransfer.dropEffect = 'move'
          dnd.onDragOver(doc.id, zoneAt(e.currentTarget, e.clientY))
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) dnd.onDragLeave(doc.id)
        }}
        onDrop={(e) => {
          e.preventDefault()
          e.stopPropagation()
          dnd.onDrop(doc.id)
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
              onToggle(doc.id)
            }}
          >
            <ChevronRight className="size-3" />
          </Button>
        ) : (
          <span className="inline-flex size-5 shrink-0" aria-hidden="true" />
        )}
        <span className="inline-flex w-[18px] shrink-0 items-center justify-center text-sm leading-none text-muted-foreground/70">
          {doc.icon ? <Emoji value={doc.icon} size={15} /> : <FileText className="size-[15px]" />}
        </span>
        <span className="min-w-0 flex-1 truncate">{doc.title || 'Untitled'}</span>
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
            onClick={() => onCreateChild(doc.id)}
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
            <DropdownMenuContent align="end" className="w-auto min-w-32">
              <DropdownMenuItem className={menuItemClass} data-danger="true" onClick={() => onDelete(doc.id)}>
                <Trash2 className="size-[14px]" />
                Delete
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
      </div>
      {expanded
        ? children.map((child) => (
            <DocTreeItem
              key={child.id}
              doc={child}
              docs={docs}
              depth={depth + 1}
              activeId={activeId}
              dnd={dnd}
              isExpanded={isExpanded}
              onToggle={onToggle}
              onOpen={onOpen}
              onCreateChild={onCreateChild}
              onDelete={onDelete}
            />
          ))
        : null}
    </>
  )
}
