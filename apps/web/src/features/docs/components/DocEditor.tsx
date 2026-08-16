import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Add, ArrowLeft, MoreH, Trash } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import { relativeTime } from '../../../lib/format'
import { updateDocContent, updateDocTitle } from '../../../mock/actions'
import { nextId } from '../../../mock/store'
import type { Doc, DocBlock, User } from '../../../mock/types'
import { ancestorsOf } from '../lib'
import { BlockEditor } from './BlockEditor'
import { BlockView } from './BlockView'

interface DocEditorProps {
  doc: Doc
  docs: Doc[]
  users: User[]
  onDelete: () => void
}

export function DocEditor({ doc, docs, users, onDelete }: DocEditorProps) {
  const navigate = useNavigate()
  const [title, setTitle] = useState(doc.title)
  const [editingId, setEditingId] = useState<string | null>(null)

  const ancestors = ancestorsOf(docs, doc.id)
  const updatedBy = users.find((u) => u.id === doc.updatedBy)

  const commitTitle = () => {
    const next = title.trim() || 'Untitled'
    if (next !== doc.title) updateDocTitle(doc.id, next)
    if (next !== title) setTitle(next)
  }

  const setBlocks = (blocks: DocBlock[]) => updateDocContent(doc.id, blocks)

  const commitBlock = (id: string, text: string, action: 'close' | 'insert') => {
    let blocks = doc.content.map((b) => (b.id === id ? { ...b, text } : b))
    if (action === 'insert') {
      const fresh: DocBlock = { id: nextId('db'), type: 'p', text: '' }
      const at = blocks.findIndex((b) => b.id === id)
      blocks = [...blocks.slice(0, at + 1), fresh, ...blocks.slice(at + 1)]
      setEditingId(fresh.id)
    } else {
      setEditingId(null)
    }
    setBlocks(blocks)
  }

  const removeBlock = (id: string) => {
    setEditingId(null)
    setBlocks(doc.content.filter((b) => b.id !== id))
  }

  const changeType = (id: string, type: DocBlock['type'], text: string) => {
    setBlocks(
      doc.content.map((b) =>
        b.id === id ? { ...b, type, text, checked: type === 'todo' ? (b.checked ?? false) : undefined } : b,
      ),
    )
  }

  const toggleTodo = (id: string) => {
    setBlocks(doc.content.map((b) => (b.id === id ? { ...b, checked: !b.checked } : b)))
  }

  const addBlock = () => {
    const fresh: DocBlock = { id: nextId('db'), type: 'p', text: '' }
    setBlocks([...doc.content, fresh])
    setEditingId(fresh.id)
  }

  return (
    <section className="pane docs-editor-pane">
      <div className="pane-header">
        <button
          type="button"
          className="icon-button docs-back"
          aria-label="Back to docs"
          onClick={() => navigate('/docs')}
        >
          <ArrowLeft size={16} />
        </button>
        <nav className="docs-breadcrumbs truncate" aria-label="Breadcrumb">
          {ancestors.map((a) => (
            <span key={a.id} style={{ display: 'contents' }}>
              <button
                type="button"
                className="truncate text-faint"
                onClick={() => navigate(`/docs/${a.id}`)}
              >
                {a.title || 'Untitled'}
              </button>
              <span className="docs-crumb-sep">/</span>
            </span>
          ))}
          <span className="docs-crumb-current truncate">{doc.title || 'Untitled'}</span>
        </nav>
        <span className="spacer" />
        <span className="docs-updated-meta text-faint text-xs" style={{ whiteSpace: 'nowrap' }}>
          Updated {relativeTime(doc.updatedAt)} by {updatedBy?.name ?? 'Unknown'}
        </span>
        <Dropdown
          align="right"
          trigger={() => (
            <button type="button" className="icon-button" aria-label="Document options">
              <MoreH size={16} />
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
                onDelete()
              }}
            >
              <Trash size={14} />
              Delete
            </button>
          )}
        </Dropdown>
      </div>
      <div className="pane-body docs-editor-scroll">
        <div className="docs-editor-column">
          <input
            className="doc-title-input"
            value={title}
            placeholder="Untitled"
            onChange={(e) => setTitle(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
            }}
          />
          <div className="doc-blocks">
            {doc.content.map((block, index) =>
              index === 0 && block.type === 'h1' && block.text === doc.title ? null : block.id === editingId ? (
                <BlockEditor
                  key={block.id}
                  block={block}
                  onCommit={(text, action) => commitBlock(block.id, text, action)}
                  onDelete={() => removeBlock(block.id)}
                  onChangeType={(type, text) => changeType(block.id, type, text)}
                />
              ) : (
                <BlockView
                  key={block.id}
                  block={block}
                  blocks={doc.content}
                  index={index}
                  onEdit={() => setEditingId(block.id)}
                  onToggleTodo={() => toggleTodo(block.id)}
                />
              ),
            )}
          </div>
          <button type="button" className="button button-ghost doc-add-block" onClick={addBlock}>
            <Add size={14} />
            Add block
          </button>
        </div>
      </div>
    </section>
  )
}
