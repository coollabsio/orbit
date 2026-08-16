import { CaretRight, MoreH, Note2, Plus, Trash } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import type { Doc } from '../../../mock/types'
import { childrenOf } from '../lib'

interface DocTreeItemProps {
  doc: Doc
  docs: Doc[]
  depth: number
  activeId: string | null
  isExpanded: (id: string) => boolean
  onToggle: (id: string) => void
  onOpen: (id: string) => void
  onCreateChild: (parentId: string) => void
  onDelete: (id: string) => void
}

export function DocTreeItem({
  doc,
  docs,
  depth,
  activeId,
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
        style={{ paddingLeft: 6 + depth * 16 }}
        onClick={() => onOpen(doc.id)}
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
        <span className="doc-tree-icon">{doc.icon ?? <Note2 size={15} />}</span>
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
