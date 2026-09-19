import { useState } from 'react'
import { useNavigate } from 'react-router'
import { FileText, Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '../../../components/ui/EmptyState'
import { createDoc, deleteDoc, moveDoc } from '../../../mock/actions'
import type { Doc } from '../../../mock/types'
import { ConfirmDeleteModal } from '../../chat/components/ChannelModals'
import { ancestorsOf, childrenOf, descendantsOf } from '../lib'
import { DocTreeItem, type DocDropZone } from './DocTreeItem'

interface DocTreeProps {
  docs: Doc[]
  activeId: string | null
}

export function DocTree({ docs, activeId }: DocTreeProps) {
  const navigate = useNavigate()
  /** Explicit user toggles; anything not present falls back to "ancestor of active doc". */
  const [overrides, setOverrides] = useState<ReadonlyMap<string, boolean>>(new Map())
  const [deleteTarget, setDeleteTarget] = useState<Doc | null>(null)
  const [dragId, setDragId] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<{ id: string; zone: DocDropZone } | null>(null)

  const ancestorIds = new Set(ancestorsOf(docs, activeId).map((d) => d.id))
  const isExpanded = (id: string) => overrides.get(id) ?? ancestorIds.has(id)

  const expand = (id: string, next: Map<string, boolean> = new Map(overrides)) => {
    next.set(id, true)
    setOverrides(next)
  }

  const toggle = (id: string) => {
    const next = new Map(overrides)
    next.set(id, !isExpanded(id))
    setOverrides(next)
  }

  const openDoc = (id: string) => navigate(`/docs/${id}`)

  const handleCreate = (parentId: string | null) => {
    const doc = createDoc(parentId)
    if (parentId && !isExpanded(parentId)) expand(parentId)
    navigate(`/docs/${doc.id}`)
  }

  const confirmDelete = (doc: Doc) => {
    const gone = new Set([doc.id, ...descendantsOf(docs, doc.id).map((d) => d.id)])
    deleteDoc(doc.id)
    if (activeId && gone.has(activeId)) navigate('/docs')
    setDeleteTarget(null)
  }

  // a page cannot be dropped on itself or inside its own subtree
  const canDropOn = (targetId: string) => {
    if (!dragId || dragId === targetId) return false
    return !descendantsOf(docs, dragId).some((d) => d.id === targetId)
  }

  const endDrag = () => {
    setDragId(null)
    setDropAt(null)
  }

  const handleDrop = (targetId: string) => {
    const zone = dropAt?.id === targetId ? dropAt.zone : null
    const target = docs.find((d) => d.id === targetId)
    if (dragId && zone && target && canDropOn(targetId)) {
      if (zone === 'inside') {
        moveDoc(dragId, target.id, null)
        expand(target.id)
      } else {
        // reorder among the target's siblings; "after" inserts before the next sibling (or last)
        const siblings = childrenOf(docs, target.parentId).filter((d) => d.id !== dragId)
        const at = siblings.findIndex((d) => d.id === target.id)
        const beforeId = zone === 'before' ? target.id : (siblings[at + 1]?.id ?? null)
        moveDoc(dragId, target.parentId, beforeId)
      }
    }
    endDrag()
  }

  const dnd = {
    dragId,
    dropAt,
    canDropOn,
    onDragStart: setDragId,
    onDragEnd: endDrag,
    onDragOver: (id: string, zone: DocDropZone) => {
      if (dropAt?.id !== id || dropAt.zone !== zone) setDropAt({ id, zone })
    },
    onDragLeave: (id: string) => {
      if (dropAt?.id === id) setDropAt(null)
    },
    onDrop: handleDrop,
  }

  const roots = childrenOf(docs, null)

  return (
    <section className="flex h-full min-h-0 w-[260px] min-w-0 shrink-0 flex-col border-r border-border bg-background max-[899px]:w-full max-[899px]:border-r-0 max-[899px]:group-data-[view=doc]/docs:hidden">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:border-b-0">
        <span className="truncate text-[13px] font-semibold text-foreground">Documents</span>
        <span className="flex-1" />
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label="New page"
          onClick={() => handleCreate(null)}
        >
          <Plus className="size-4" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {roots.length === 0 ? (
          <EmptyState
            icon={FileText}
            title="No documents yet"
            description="Create your first page to start writing."
          />
        ) : (
          <nav className="flex flex-col gap-px p-2">
            {roots.map((doc) => (
              <DocTreeItem
                key={doc.id}
                doc={doc}
                docs={docs}
                depth={0}
                activeId={activeId}
                dnd={dnd}
                isExpanded={isExpanded}
                onToggle={toggle}
                onOpen={openDoc}
                onCreateChild={handleCreate}
                onDelete={(id) => setDeleteTarget(docs.find((d) => d.id === id) ?? null)}
              />
            ))}
          </nav>
        )}
      </div>

      {deleteTarget ? (
        <ConfirmDeleteModal
          title={`Delete "${deleteTarget.title || 'Untitled'}"?`}
          description={
            descendantsOf(docs, deleteTarget.id).length > 0
              ? `This will permanently delete the page and its ${descendantsOf(docs, deleteTarget.id).length} sub-page${descendantsOf(docs, deleteTarget.id).length === 1 ? '' : 's'}.`
              : 'This will permanently delete the page.'
          }
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => confirmDelete(deleteTarget)}
        />
      ) : null}
    </section>
  )
}
