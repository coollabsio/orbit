import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Add, Note2 } from 'reicon-react'
import { EmptyState } from '../../../components/ui/EmptyState'
import { createDoc, deleteDoc } from '../../../mock/actions'
import type { Doc } from '../../../mock/types'
import { ancestorsOf, childrenOf } from '../lib'
import { DocTreeItem } from './DocTreeItem'

interface DocTreeProps {
  docs: Doc[]
  activeId: string | null
}

export function DocTree({ docs, activeId }: DocTreeProps) {
  const navigate = useNavigate()
  /** Explicit user toggles; anything not present falls back to "ancestor of active doc". */
  const [overrides, setOverrides] = useState<ReadonlyMap<string, boolean>>(new Map())

  const ancestorIds = new Set(ancestorsOf(docs, activeId).map((d) => d.id))
  const isExpanded = (id: string) => overrides.get(id) ?? ancestorIds.has(id)

  const toggle = (id: string) => {
    const next = new Map(overrides)
    next.set(id, !isExpanded(id))
    setOverrides(next)
  }

  const openDoc = (id: string) => navigate(`/docs/${id}`)

  const handleCreate = (parentId: string | null) => {
    const doc = createDoc(parentId)
    if (parentId && !isExpanded(parentId)) {
      const next = new Map(overrides)
      next.set(parentId, true)
      setOverrides(next)
    }
    navigate(`/docs/${doc.id}`)
  }

  const handleDelete = (id: string) => {
    const activeDoc = docs.find((d) => d.id === activeId)
    deleteDoc(id)
    if (activeId === id || activeDoc?.parentId === id) navigate('/docs')
  }

  const roots = childrenOf(docs, null)

  return (
    <section className="pane docs-tree-pane">
      <div className="docs-tree-label">
        <span className="nav-section">Documents</span>
        <span className="spacer" />
        <button
          type="button"
          className="icon-button"
          aria-label="New page"
          onClick={() => handleCreate(null)}
        >
          <Add size={16} />
        </button>
      </div>
      <div className="pane-body">
        {roots.length === 0 ? (
          <EmptyState
            icon={Note2}
            title="No documents yet"
            description="Create your first page to start writing."
          />
        ) : (
          <nav className="docs-tree">
            {roots.map((doc) => (
              <DocTreeItem
                key={doc.id}
                doc={doc}
                docs={docs}
                depth={0}
                activeId={activeId}
                isExpanded={isExpanded}
                onToggle={toggle}
                onOpen={openDoc}
                onCreateChild={handleCreate}
                onDelete={handleDelete}
              />
            ))}
          </nav>
        )}
      </div>
    </section>
  )
}
