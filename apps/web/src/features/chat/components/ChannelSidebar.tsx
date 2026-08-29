// Port of the chat reference ChannelSidebar: resizable width, collapsible categories, context menus,
// and hand-rolled mouse drag-and-drop reorder for categories and channels (no DnD library).
import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router'
import { Add, ChevronDown, Edit, FolderAdd, Hashtag, Setting2, Trash } from 'reicon-react'
import { deleteChatMessage, followThread, markChannelRead, renameThread, reorderChannels, reorderChatCategories } from '../../../mock/actions'
import { FollowIcon } from '../../../components/ui/icons/FollowIcon'
import { threadTitleOf } from '../chatLib'
import type { AppState, Channel, ChatCategory, ChatMessage } from '../../../mock/types'
import { ChannelModals, ConfirmDeleteModal, type ChannelModalState } from './ChannelModals'

const WIDTH_KEY = 'orbit:channel_sidebar_width'
const COLLAPSE_KEY = 'orbit:category_collapsed'
const MIN_WIDTH = 220
const MAX_WIDTH = 420
const DEFAULT_WIDTH = 240

function clampWidth(width: number) {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width))
}

function storedWidth(): number {
  const value = Number(window.localStorage.getItem(WIDTH_KEY))
  return Number.isFinite(value) && value > 0 ? clampWidth(value) : DEFAULT_WIDTH
}

function storedCollapsed(): Set<string> {
  try {
    const value = JSON.parse(window.localStorage.getItem(COLLAPSE_KEY) ?? '[]')
    return Array.isArray(value) ? new Set(value.filter((v): v is string => typeof v === 'string')) : new Set()
  } catch {
    return new Set()
  }
}

interface ContextMenuState {
  x: number
  y: number
  target:
    | { kind: 'channel'; channel: Channel }
    | { kind: 'category'; category: ChatCategory }
    | { kind: 'thread'; thread: ChatMessage }
}

/* ---------- the chat reference drag helpers ---------- */

type DropPosition = 'before' | 'after'
type ChannelDropIndicator = { channelId: string; categoryId: string; position: DropPosition }
type ChannelDragState = { channelId: string; categoryId: string }
type CategoryDropIndicator = { categoryId: string; position: DropPosition }

function getDropPosition(element: HTMLElement, clientY: number): DropPosition {
  const rect = element.getBoundingClientRect()
  return clientY < rect.top + rect.height / 2 ? 'before' : 'after'
}

function getChannelDropTarget(clientX: number, clientY: number): ChannelDropIndicator | null {
  const element = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-channel-drop-id]')
  const channelId = element?.getAttribute('data-channel-drop-id')
  const categoryId = element?.getAttribute('data-channel-category-id')
  if (!element || !channelId || !categoryId) return null
  return { channelId, categoryId, position: getDropPosition(element, clientY) }
}

function getCategoryDropTarget(clientX: number, clientY: number): CategoryDropIndicator | null {
  const element = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-category-drop-id]')
  const categoryId = element?.getAttribute('data-category-drop-id')
  if (!element || !categoryId) return null
  return { categoryId, position: getDropPosition(element, clientY) }
}

function moveItem<T extends { id: string }>(items: T[], draggedId: string, targetId: string, position: DropPosition): T[] {
  const next = [...items]
  const fromIndex = next.findIndex((item) => item.id === draggedId)
  const targetIndex = next.findIndex((item) => item.id === targetId)
  if (fromIndex === -1 || targetIndex === -1) return next
  const [dragged] = next.splice(fromIndex, 1)
  const adjustedTargetIndex = next.findIndex((item) => item.id === targetId)
  if (adjustedTargetIndex === -1) return items
  next.splice(position === 'after' ? adjustedTargetIndex + 1 : adjustedTargetIndex, 0, dragged)
  return next
}

function DropLine({ position }: { position: DropPosition | null }) {
  if (!position) return null
  return <span aria-hidden="true" className="fc-drop-line" data-position={position} />
}

export function ChannelSidebar({
  state,
  activeChannelId,
  activeThreadId = null,
  onOpenThread,
}: {
  state: AppState
  activeChannelId: string | null
  /** Open thread root id: the thread row is active and the channel row is not. */
  activeThreadId?: string | null
  onOpenThread?: (channelId: string, rootId: string) => void
}) {
  const navigate = useNavigate()
  const [width, setWidth] = useState(storedWidth)
  const [collapsed, setCollapsed] = useState<Set<string>>(storedCollapsed)
  const [serverMenuOpen, setServerMenuOpen] = useState(false)
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null)
  const [modal, setModal] = useState<ChannelModalState>(null)
  const [deleteThread, setDeleteThread] = useState<ChatMessage | null>(null)
  const [renamingThread, setRenamingThread] = useState<ChatMessage | null>(null)
  const [threadNameDraft, setThreadNameDraft] = useState('')
  const serverMenuRef = useRef<HTMLDivElement>(null)
  const contextRef = useRef<HTMLDivElement>(null)

  const [dragChannel, setDragChannel] = useState<ChannelDragState | null>(null)
  const [dropIndicator, setDropIndicator] = useState<ChannelDropIndicator | null>(null)
  const [dragCategoryId, setDragCategoryId] = useState<string | null>(null)
  const [categoryDropIndicator, setCategoryDropIndicator] = useState<CategoryDropIndicator | null>(null)

  useEffect(() => {
    window.localStorage.setItem(WIDTH_KEY, String(width))
  }, [width])

  useEffect(() => {
    window.localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...collapsed]))
  }, [collapsed])

  useEffect(() => {
    if (!serverMenuOpen && !contextMenu) return
    function onPointerDown(e: MouseEvent) {
      if (serverMenuRef.current && !serverMenuRef.current.contains(e.target as Node)) setServerMenuOpen(false)
      if (contextRef.current && !contextRef.current.contains(e.target as Node)) setContextMenu(null)
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        setServerMenuOpen(false)
        setContextMenu(null)
      }
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [serverMenuOpen, contextMenu])

  function handleResizeStart(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault()
    const startX = event.clientX
    const startWidth = width
    function onMove(moveEvent: PointerEvent) {
      setWidth(clampWidth(startWidth + (moveEvent.clientX - startX)))
    }
    function onUp() {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  function toggleCategory(catId: string) {
    setCollapsed((prev) => {
      const next = new Set(prev)
      if (next.has(catId)) next.delete(catId)
      else next.add(catId)
      return next
    })
  }

  function selectChannel(channelId: string) {
    markChannelRead(channelId)
    navigate(`/chat/${channelId}`)
  }

  /** Followed threads only: non-reply roots with replies or a thread start, oldest first. */
  function threadsForChannel(channelId: string): ChatMessage[] {
    return state.chatMessages
      .filter(
        (m) =>
          m.channelId === channelId &&
          !m.threadRootId &&
          m.threadFollowed &&
          (m.startsThread || state.chatMessages.some((r) => r.threadRootId === m.id)),
      )
      .sort((a, b) => a.createdAt.localeCompare(b.createdAt))
  }

  function commitThreadRename() {
    if (!renamingThread) return
    const name = threadNameDraft.trim()
    if (!name) return
    renameThread(renamingThread.id, name)
    setRenamingThread(null)
    setThreadNameDraft('')
  }

  /* ---- channel drag (the chat reference: channels cannot move between categories) ---- */

  function startChannelDrag(channelId: string, categoryId: string) {
    setDragChannel({ channelId, categoryId })
    setDropIndicator(null)
    setDragCategoryId(null)
    setCategoryDropIndicator(null)
    setContextMenu(null)

    function handleMouseUp(event: MouseEvent) {
      const target = getChannelDropTarget(event.clientX, event.clientY)
      if (target && target.channelId !== channelId && target.categoryId === categoryId) {
        const inCategory = state.channels.filter((c) => c.categoryId === categoryId)
        const next = moveItem(inCategory, channelId, target.channelId, target.position)
        reorderChannels(categoryId, next.map((c) => c.id))
      }
      setDragChannel(null)
      setDropIndicator(null)
    }

    document.addEventListener('mouseup', handleMouseUp, { once: true })
  }

  function handleChannelDragOver(channelId: string, categoryId: string, element: HTMLElement, clientY: number) {
    if (!dragChannel || dragChannel.channelId === channelId || dragChannel.categoryId !== categoryId) {
      setDropIndicator(null)
      return
    }
    const position = getDropPosition(element, clientY)
    setDropIndicator((current) =>
      current?.channelId === channelId && current.position === position ? current : { channelId, categoryId, position },
    )
  }

  function handleChannelDragLeave(channelId: string) {
    setDropIndicator((current) => (current?.channelId === channelId ? null : current))
  }

  /* ---- category drag ---- */

  function startCategoryDrag(categoryId: string) {
    setDragCategoryId(categoryId)
    setCategoryDropIndicator(null)
    setDragChannel(null)
    setDropIndicator(null)
    setContextMenu(null)

    function handleMouseUp(event: MouseEvent) {
      const target = getCategoryDropTarget(event.clientX, event.clientY)
      if (target && target.categoryId !== categoryId) {
        const next = moveItem(state.chatCategories, categoryId, target.categoryId, target.position)
        reorderChatCategories(next.map((c) => c.id))
      }
      setDragCategoryId(null)
      setCategoryDropIndicator(null)
    }

    document.addEventListener('mouseup', handleMouseUp, { once: true })
  }

  function handleCategoryDragOver(categoryId: string, element: HTMLElement, clientY: number) {
    if (!dragCategoryId || dragCategoryId === categoryId) {
      setCategoryDropIndicator(null)
      return
    }
    const position = getDropPosition(element, clientY)
    setCategoryDropIndicator((current) =>
      current?.categoryId === categoryId && current.position === position ? current : { categoryId, position },
    )
  }

  function handleCategoryDragLeave(categoryId: string) {
    setCategoryDropIndicator((current) => (current?.categoryId === categoryId ? null : current))
  }

  return (
    <div className="fc-sidebar" style={{ width }}>
      <div className="fc-sidebar-resize" onPointerDown={handleResizeStart} title="Resize channel sidebar" />

      {/* server header + dropdown */}
      <div style={{ position: 'relative' }} ref={serverMenuRef}>
        <button className="fc-server-header" onClick={() => setServerMenuOpen((prev) => !prev)}>
          <span className="fc-server-name">Chat</span>
          <ChevronDown size={14} style={{ flexShrink: 0 }} color="var(--text-muted)" />
        </button>
        {serverMenuOpen ? (
          <div className="fc-menu fc-server-menu">
            <button
              className="fc-menu-item"
              onClick={() => {
                setServerMenuOpen(false)
                const first = state.chatCategories[0]
                if (first) setModal({ kind: 'create-channel', categoryId: first.id, categoryName: first.name })
              }}
            >
              <Add size={16} />
              Create channel
            </button>
            <button
              className="fc-menu-item"
              onClick={() => {
                setServerMenuOpen(false)
                setModal({ kind: 'create-category' })
              }}
            >
              <FolderAdd size={16} />
              Create category
            </button>
            <div className="fc-menu-separator" />
            <button
              className="fc-menu-item"
              onClick={() => {
                setServerMenuOpen(false)
                navigate('/chat/settings')
              }}
            >
              <Setting2 size={16} />
              Chat Settings
            </button>
          </div>
        ) : null}
      </div>

      {/* channel list */}
      <div className="fc-channel-scroll">
        <div className="fc-channel-list">
          {state.chatCategories.map((cat) => {
            const isCollapsed = collapsed.has(cat.id)
            const channels = state.channels.filter((c) => c.categoryId === cat.id)
            return (
              <div key={cat.id} className="fc-category">
                <div
                  className="fc-category-row"
                  onContextMenu={(e) => {
                    e.preventDefault()
                    setContextMenu({ x: e.clientX, y: e.clientY, target: { kind: 'category', category: cat } })
                  }}
                >
                  <button
                    type="button"
                    className="fc-category-toggle"
                    data-category-drop-id={cat.id}
                    data-collapsed={isCollapsed ? 'true' : undefined}
                    data-dragging={dragCategoryId === cat.id ? 'true' : undefined}
                    onClick={() => toggleCategory(cat.id)}
                    onMouseMove={(event: ReactMouseEvent<HTMLButtonElement>) =>
                      handleCategoryDragOver(cat.id, event.currentTarget, event.clientY)
                    }
                    onMouseLeave={() => handleCategoryDragLeave(cat.id)}
                    onMouseDown={(event) => {
                      if (event.button !== 0) return
                      startCategoryDrag(cat.id)
                    }}
                  >
                    <DropLine
                      position={categoryDropIndicator?.categoryId === cat.id ? categoryDropIndicator.position : null}
                    />
                    <span className="fc-category-label">
                      {cat.emoji ? <span className="fc-emoji-icon" data-size="sm">{cat.emoji}</span> : null}
                      <span className="truncate">{cat.name}</span>
                    </span>
                    <ChevronDown className="fc-chevron" />
                  </button>
                  <button
                    type="button"
                    className="fc-category-add"
                    title="Create Channel"
                    onClick={() => setModal({ kind: 'create-channel', categoryId: cat.id, categoryName: cat.name })}
                  >
                    <Add />
                  </button>
                </div>
                {!isCollapsed ? (
                  <div className="fc-category-channels">
                    {channels.map((ch) => {
                      const isActive = ch.id === activeChannelId && !activeThreadId
                      const threads = threadsForChannel(ch.id)
                      return (
                        <div key={ch.id} style={{ minWidth: 0 }}>
                        <button
                          type="button"
                          className="fc-channel-row"
                          data-channel-drop-id={ch.id}
                          data-channel-category-id={cat.id}
                          data-active={isActive ? 'true' : undefined}
                          data-unread={ch.unreadCount > 0 ? 'true' : undefined}
                          data-dragging={dragChannel?.channelId === ch.id ? 'true' : undefined}
                          onClick={() => selectChannel(ch.id)}
                          onMouseMove={(event: ReactMouseEvent<HTMLButtonElement>) =>
                            handleChannelDragOver(ch.id, cat.id, event.currentTarget, event.clientY)
                          }
                          onMouseLeave={() => handleChannelDragLeave(ch.id)}
                          onMouseDown={(event) => {
                            if (event.button !== 0) return
                            startChannelDrag(ch.id, cat.id)
                          }}
                          onContextMenu={(e) => {
                            e.preventDefault()
                            setContextMenu({ x: e.clientX, y: e.clientY, target: { kind: 'channel', channel: ch } })
                          }}
                        >
                          <DropLine position={dropIndicator?.channelId === ch.id ? dropIndicator.position : null} />
                          {ch.emoji ? <span className="fc-emoji-icon">{ch.emoji}</span> : <Hashtag size={16} />}
                          <span className="fc-channel-row-name">{ch.name}</span>
                          {ch.unreadCount > 0 && !isActive ? (
                            <span className="fc-unread-badge">{ch.unreadCount}</span>
                          ) : null}
                        </button>
                        {threads.length > 0 ? (
                          <div className="fc-thread-rows">
                            {threads.map((thread, index) => {
                              const isThreadActive = activeChannelId === ch.id && activeThreadId === thread.id
                              return (
                                <button
                                  key={thread.id}
                                  type="button"
                                  className="fc-thread-row"
                                  data-active={isThreadActive ? 'true' : undefined}
                                  onClick={() => onOpenThread?.(ch.id, thread.id)}
                                  onContextMenu={(e) => {
                                    e.preventDefault()
                                    setContextMenu({ x: e.clientX, y: e.clientY, target: { kind: 'thread', thread } })
                                  }}
                                >
                                  <span aria-hidden className="fc-thread-row-connector" data-last={index === threads.length - 1 ? 'true' : undefined} />
                                  <span aria-hidden className="fc-thread-row-bg" />
                                  <span className="fc-thread-row-label">{threadTitleOf(thread)}</span>
                                </button>
                              )
                            })}
                          </div>
                        ) : null}
                        </div>
                      )
                    })}
                  </div>
                ) : null}
              </div>
            )
          })}
        </div>
      </div>

      {/* context menus */}
      {contextMenu
        ? createPortal(
        <div ref={contextRef} className="fc-menu fc-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
          {contextMenu.target.kind === 'thread' ? (
            <>
              <button
                className="fc-menu-item"
                onClick={() => {
                  const thread = (contextMenu.target as { kind: 'thread'; thread: ChatMessage }).thread
                  followThread(thread.id, !thread.threadFollowed)
                  setContextMenu(null)
                }}
              >
                <FollowIcon size={16} />
                {(contextMenu.target as { kind: 'thread'; thread: ChatMessage }).thread.threadFollowed ? 'Unfollow Thread' : 'Follow Thread'}
              </button>
              <button
                className="fc-menu-item"
                onClick={() => {
                  const thread = (contextMenu.target as { kind: 'thread'; thread: ChatMessage }).thread
                  setRenamingThread(thread)
                  setThreadNameDraft(threadTitleOf(thread))
                  setContextMenu(null)
                }}
              >
                <Edit size={16} />
                Rename Thread
              </button>
              <button
                className="fc-menu-item"
                data-danger="true"
                onClick={() => {
                  setDeleteThread((contextMenu.target as { kind: 'thread'; thread: ChatMessage }).thread)
                  setContextMenu(null)
                }}
              >
                <Trash size={16} />
                Delete Thread
              </button>
            </>
          ) : contextMenu.target.kind === 'channel' ? (
            <>
              <button
                className="fc-menu-item"
                onClick={() => {
                  const channel = (contextMenu.target as { kind: 'channel'; channel: Channel }).channel
                  setContextMenu(null)
                  setModal({ kind: 'edit-channel', channel })
                }}
              >
                <Edit size={16} />
                Edit Channel
              </button>
              <button
                className="fc-menu-item"
                data-danger="true"
                onClick={() => {
                  const channel = (contextMenu.target as { kind: 'channel'; channel: Channel }).channel
                  setContextMenu(null)
                  setModal({ kind: 'delete-channel', channel })
                }}
              >
                <Trash size={16} />
                Delete Channel
              </button>
            </>
          ) : (
            <>
              <button
                className="fc-menu-item"
                onClick={() => {
                  const category = (contextMenu.target as { kind: 'category'; category: ChatCategory }).category
                  setContextMenu(null)
                  setModal({ kind: 'edit-category', category })
                }}
              >
                <Edit size={16} />
                Edit Category
              </button>
              <button
                className="fc-menu-item"
                onClick={() => {
                  const category = (contextMenu.target as { kind: 'category'; category: ChatCategory }).category
                  setContextMenu(null)
                  setModal({ kind: 'create-channel', categoryId: category.id, categoryName: category.name })
                }}
              >
                <Add size={16} />
                Create Channel
              </button>
              <button
                className="fc-menu-item"
                data-danger="true"
                onClick={() => {
                  const category = (contextMenu.target as { kind: 'category'; category: ChatCategory }).category
                  setContextMenu(null)
                  setModal({ kind: 'delete-category', id: category.id, name: category.name })
                }}
              >
                <Trash size={16} />
                Delete Category
              </button>
            </>
          )}
        </div>,
            document.body,
          )
        : null}

      <ChannelModals modal={modal} onClose={() => setModal(null)} activeChannelId={activeChannelId} />
      {deleteThread ? (
        <ConfirmDeleteModal
          title="Delete thread?"
          description={`This will permanently delete the thread "${threadTitleOf(deleteThread)}".`}
          onClose={() => setDeleteThread(null)}
          onConfirm={() => {
            deleteChatMessage(deleteThread.id)
            setDeleteThread(null)
          }}
        />
      ) : null}
      {renamingThread ? (
        <div className="fc-rename-overlay">
          <div className="fc-rename-card">
            <h2>Rename thread</h2>
            <label className="fc-rename-label">
              <span>Thread name</span>
              <input
                className="fc-rename-input"
                value={threadNameDraft}
                autoFocus
                onChange={(e) => setThreadNameDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitThreadRename()
                  if (e.key === 'Escape') setRenamingThread(null)
                }}
              />
            </label>
            <div className="fc-rename-actions">
              <button type="button" className="button" onClick={() => setRenamingThread(null)}>
                Cancel
              </button>
              <button type="button" className="button button-primary" disabled={!threadNameDraft.trim()} onClick={commitThreadRename}>
                Save
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}
