import { CaretRight, MoreH, Note2, Plus, Trash } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import { Emoji } from '../../../components/ui/Emoji'
import type { Doc } from '../../../mock/types'
import { childrenOf } from '../lib'

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
        className="menu-item doc-tree-row"
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
          <button
            type="button"
            className="doc-tree-chevron"
            data-expanded={expanded}
            aria-label={expanded ? 'Collapse' : 'Expand'}
            onClick={(e) => {
              e.stopPropagation()
              onToggle(doc.id)
            }}
          >
            <CaretRight size={12} />
          </button>
        ) : (
          <span className="doc-tree-chevron" aria-hidden="true" />
        )}
        <span className="doc-tree-icon">{doc.icon ? <Emoji value={doc.icon} size={15} /> : <Note2 size={15} />}</span>
        <span className="menu-item-label truncate">{doc.title || 'Untitled'}</span>
        <span className="doc-tree-actions" onClick={(e) => e.stopPropagation()}>
          <button
            type="button"
            className="icon-button"
            aria-label="Add child page"
            onClick={() => onCreateChild(doc.id)}
          >
            <Plus size={13} />
          </button>
          <Dropdown
            align="right"
            trigger={() => (
              <button type="button" className="icon-button" aria-label="Page options">
                <MoreH size={13} />
              </button>
            )}
          >
            {(close) => (
              <button
                type="button"
                className="popover-option"
                style={{ color: 'var(--danger)' }}
                onClick={() => {
                  close()
                  onDelete(doc.id)
                }}
              >
                <Trash size={14} />
                Delete
              </button>
            )}
          </Dropdown>
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
