// Port of the chat reference ChannelSidebar (the chat reference frontend/src/components/layout/ChannelSidebar.tsx)
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { Add, ArrowSwapHorizontal, ChevronDown, Edit, FolderAdd, Hashtag, Setting2, Trash } from 'reicon-react'
import { markChannelRead } from '../../../mock/actions'
import type { AppState, Channel } from '../../../mock/types'
import { ChannelModals, type ChannelModalState } from './ChannelModals'

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
  target: { kind: 'channel'; channel: Channel } | { kind: 'category'; id: string; name: string }
}

export function ChannelSidebar({ state, activeChannelId }: { state: AppState; activeChannelId: string | null }) {
  const navigate = useNavigate()
  const [width, setWidth] = useState(storedWidth)
  const [collapsed, setCollapsed] = useState<Set<string>>(storedCollapsed)
  const [serverMenuOpen, setServerMenuOpen] = useState(false)
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null)
  const [modal, setModal] = useState<ChannelModalState>(null)
  const serverMenuRef = useRef<HTMLDivElement>(null)
  const contextRef = useRef<HTMLDivElement>(null)

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

  return (
    <div className="fc-sidebar" style={{ width }}>
      <div className="fc-sidebar-resize" onPointerDown={handleResizeStart} title="Resize channel sidebar" />

      {/* server header + dropdown */}
      <div style={{ position: 'relative' }} ref={serverMenuRef}>
        <button className="fc-server-header" onClick={() => setServerMenuOpen((prev) => !prev)}>
          <span style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
            <span className="fc-server-icon">O</span>
            <span className="fc-server-name">Orbit Chat</span>
          </span>
          <ArrowSwapHorizontal size={16} style={{ transform: 'rotate(90deg)', flexShrink: 0 }} color="var(--text-muted)" />
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
                navigate('/settings')
              }}
            >
              <Setting2 size={16} />
              Workspace Settings
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
                    setContextMenu({ x: e.clientX, y: e.clientY, target: { kind: 'category', id: cat.id, name: cat.name } })
                  }}
                >
                  <button
                    type="button"
                    className="fc-category-toggle"
                    data-collapsed={isCollapsed ? 'true' : undefined}
                    onClick={() => toggleCategory(cat.id)}
                  >
                    <span className="truncate">{cat.name}</span>
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
                      const isActive = ch.id === activeChannelId
                      return (
                        <button
                          key={ch.id}
                          type="button"
                          className="fc-channel-row"
                          data-active={isActive ? 'true' : undefined}
                          data-unread={ch.unreadCount > 0 ? 'true' : undefined}
                          onClick={() => selectChannel(ch.id)}
                          onContextMenu={(e) => {
                            e.preventDefault()
                            setContextMenu({ x: e.clientX, y: e.clientY, target: { kind: 'channel', channel: ch } })
                          }}
                        >
                          <Hashtag size={16} />
                          <span className="fc-channel-row-name">{ch.name}</span>
                          {ch.unreadCount > 0 && !isActive ? (
                            <span className="fc-unread-badge">{ch.unreadCount}</span>
                          ) : null}
                        </button>
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
      {contextMenu ? (
        <div ref={contextRef} className="fc-menu fc-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
          {contextMenu.target.kind === 'channel' ? (
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
                  const target = contextMenu.target as { kind: 'category'; id: string; name: string }
                  setContextMenu(null)
                  setModal({ kind: 'create-channel', categoryId: target.id, categoryName: target.name })
                }}
              >
                <Add size={16} />
                Create Channel
              </button>
              <button
                className="fc-menu-item"
                data-danger="true"
                onClick={() => {
                  const target = contextMenu.target as { kind: 'category'; id: string; name: string }
                  setContextMenu(null)
                  setModal({ kind: 'delete-category', id: target.id, name: target.name })
                }}
              >
                <Trash size={16} />
                Delete Category
              </button>
            </>
          )}
        </div>
      ) : null}

      <ChannelModals modal={modal} onClose={() => setModal(null)} activeChannelId={activeChannelId} />
    </div>
  )
}
