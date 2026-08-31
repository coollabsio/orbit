import { useState } from 'react'
import { useNavigate } from 'react-router'
import { ArrowLeft, MoreH, Trash } from 'reicon-react'
import { Dropdown } from '../../../components/ui/Dropdown'
import { EmojiPicker } from '../../../components/ui/EmojiPicker'
import { relativeTime } from '../../../lib/format'
import { createDoc, updateDocContent, updateDocCover, updateDocIcon, updateDocTitle } from '../../../mock/actions'
import { nextId } from '../../../mock/store'
import type { Doc, DocBlock, User } from '../../../mock/types'
import { ancestorsOf, numberedIndex } from '../lib'
import { ConfirmDeleteModal } from '../../chat/components/ChannelModals'
import { descendantsOf } from '../lib'
import { BlockEditor } from './BlockEditor'
import { CoverBanner } from './CoverBanner'
import { CoverSourcePanel } from './CoverSourcePanel'
import { BlockView } from './BlockView'
import { buildMentionTokens } from '../../chat/chatLib'
import { PageBlock } from './PageBlock'
import { PageLinkDialog } from './PageLinkDialog'

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
  // "Link a page" dialog opened from the slash menu; holds the block to replace.
  const [linkTarget, setLinkTarget] = useState<string | null>(null)
  // "Add cover" panel for a page without a cover (with a cover, the banner hosts its own panel)
  const [coverPanelOpen, setCoverPanelOpen] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const ancestors = ancestorsOf(docs, doc.id)
  const updatedBy = users.find((u) => u.id === doc.updatedBy)
  const mentionTokens = buildMentionTokens(users)

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
    if (doc.content.length === 1) return
    setEditingId(null)
    setBlocks(doc.content.filter((b) => b.id !== id))
  }

  const removeEmptyBlock = (id: string) => {
    const index = doc.content.findIndex((block) => block.id === id)
    const previous = doc.content
      .slice(0, index)
      .reverse()
      .find((block) => block.type !== 'page' && block.type !== 'divider')

    if (!previous) return

    setBlocks(doc.content.filter((block) => block.id !== id))
    setEditingId(previous.id)
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
    const last = doc.content.at(-1)
    if (last?.type === 'p' && last.text === '') {
      setEditingId(last.id)
      return
    }
    const fresh: DocBlock = { id: nextId('db'), type: 'p', text: '' }
    setBlocks([...doc.content, fresh])
    setEditingId(fresh.id)
  }

  /** The docs reference insertSubpage: new child page, current block becomes a page block, then open it. */
  const insertSubpage = (blockId: string) => {
    const child = createDoc(doc.id)
    setEditingId(null)
    setBlocks(
      doc.content.map((b) =>
        b.id === blockId ? { id: b.id, type: 'page' as const, text: 'New page', refId: child.id } : b,
      ),
    )
    navigate(`/docs/${child.id}`)
  }

  /** The docs reference "Link a page": remember the "/" block and open the picker. */
  const openLinkDialog = (blockId: string) => {
    setEditingId(null)
    setLinkTarget(blockId)
  }

  const pickLinkedPage = (picked: Doc) => {
    if (linkTarget) {
      setBlocks(
        doc.content.map((b) =>
          b.id === linkTarget ? { id: b.id, type: 'page' as const, text: picked.title, refId: picked.id } : b,
        ),
      )
    }
    setLinkTarget(null)
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
                setConfirmDelete(true)
              }}
            >
              <Trash size={14} />
              Delete
            </button>
          )}
        </Dropdown>
      </div>
      <div className="pane-body docs-editor-scroll">
        {doc.cover ? <CoverBanner key={`${doc.id}:${doc.cover}:${doc.coverPos ?? ''}`} doc={doc} /> : null}
        <div className="docs-editor-column">
          {doc.icon ? (
            <div className="doc-icon-row" data-cover={doc.cover ? 'true' : undefined}>
              <Dropdown
                className="emoji-dropdown"
                trigger={() => (
                  <button type="button" className="doc-icon-button" aria-label="Change icon">
                    {doc.icon}
                  </button>
                )}
              >
                {(close) => (
                  <EmojiPicker
                    onPick={(emoji) => {
                      updateDocIcon(doc.id, emoji)
                      close()
                    }}
                    onRemove={() => {
                      updateDocIcon(doc.id, null)
                      close()
                    }}
                  />
                )}
              </Dropdown>
            </div>
          ) : null}
          {!doc.icon || !doc.cover ? (
            <div className="doc-decor-actions">
              {!doc.icon ? (
                <Dropdown
                  className="emoji-dropdown"
                  trigger={() => (
                    <button type="button" className="button button-ghost doc-decor-btn">
                      😀 Add icon
                    </button>
                  )}
                >
                  {(close) => (
                    <EmojiPicker
                      onPick={(emoji) => {
                        updateDocIcon(doc.id, emoji)
                        close()
                      }}
                    />
                  )}
                </Dropdown>
              ) : null}
              {!doc.cover ? (
                <button type="button" className="button button-ghost doc-decor-btn" onClick={() => setCoverPanelOpen((o) => !o)}>
                  🖼️ Add cover
                </button>
              ) : null}
            </div>
          ) : null}
          {!doc.cover && coverPanelOpen ? (
            <div className="doc-cover-source-inline">
              <CoverSourcePanel
                onPicked={(url) => {
                  setCoverPanelOpen(false)
                  updateDocCover(doc.id, { cover: url, coverPos: null })
                }}
              />
            </div>
          ) : null}
          <input
            className="doc-title-input"
            value={title}
            placeholder="Untitled"
            onChange={(e) => setTitle(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                const firstWritable = doc.content.find(
                  (block, index) =>
                    !(index === 0 && block.type === 'h1' && block.text === doc.title) &&
                    block.type !== 'page' &&
                    block.type !== 'divider',
                )
                setEditingId(firstWritable?.id ?? null)
                e.currentTarget.blur()
              }
            }}
          />
          <div className="doc-blocks">
            {doc.content.map((block, index) =>
              index === 0 && block.type === 'h1' && block.text === doc.title ? null : block.type === 'page' ? (
                <PageBlock
                  key={block.id}
                  block={block}
                  docs={docs}
                  onOpen={(id) => navigate(`/docs/${id}`)}
                  onRemove={() => removeBlock(block.id)}
                />
              ) : block.type === 'divider' ? (
                <BlockView mentionTokens={mentionTokens}
                  key={block.id}
                  block={block}
                  blocks={doc.content}
                  index={index}
                  onEdit={() => undefined}
                  onToggleTodo={() => undefined}
                />
              ) : (
                <BlockEditor users={users}
                  key={block.id}
                  block={block}
                  autoFocus={block.id === editingId}
                  onCommit={(text, action) => commitBlock(block.id, text, action)}
                  onDelete={() => removeEmptyBlock(block.id)}
                  onChangeType={(type, text) => changeType(block.id, type, text)}
                  onToggleTodo={() => toggleTodo(block.id)}
                  marker={
                    block.type === 'bullet'
                      ? '•'
                      : block.type === 'numbered'
                        ? `${numberedIndex(doc.content, index)}.`
                        : undefined
                  }
                  onSubpage={() => insertSubpage(block.id)}
                  onLinkPage={() => openLinkDialog(block.id)}
                />
              ),
            )}
          </div>
          <button type="button" className="doc-editor-tail" aria-label="Continue writing" onClick={addBlock} />
        </div>
      </div>
      {linkTarget ? (
        <PageLinkDialog
          docs={docs}
          excludeId={doc.id}
          onPick={pickLinkedPage}
          onClose={() => setLinkTarget(null)}
        />
      ) : null}
      {confirmDelete ? (
        <ConfirmDeleteModal
          title={`Delete "${doc.title || 'Untitled'}"?`}
          description={
            descendantsOf(docs, doc.id).length > 0
              ? `This will permanently delete the page and its ${descendantsOf(docs, doc.id).length} sub-page${descendantsOf(docs, doc.id).length === 1 ? '' : 's'}.`
              : 'This will permanently delete the page.'
          }
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => {
            setConfirmDelete(false)
            onDelete()
          }}
        />
      ) : null}
    </section>
  )
}
