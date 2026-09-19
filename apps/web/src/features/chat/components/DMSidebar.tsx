import { useEffect, useState } from 'react'
import { Edit } from 'reicon-react'
import { useNavigate } from 'react-router'
import { UserAvatar } from '../../../components/ui/UserAvatar'
import type { AppState } from '../../../mock/types'
import { relativeTime } from '../../../lib/format'
import { extractPreview } from '../chatLib'
import { NewDMModal } from './NewDMModal'

const WIDTH_KEY = 'orbit:dm_sidebar_width'
const MIN_WIDTH = 220
const MAX_WIDTH = 420

function initialWidth() {
  const stored = Number(window.localStorage.getItem(WIDTH_KEY))
  return Number.isFinite(stored) && stored > 0 ? Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, stored)) : 240
}

export function DMSidebar({ state, activeId }: { state: AppState; activeId: string | null }) {
  const navigate = useNavigate()
  const [creating, setCreating] = useState(false)
  const [width, setWidth] = useState(initialWidth)

  useEffect(() => {
    window.localStorage.setItem(WIDTH_KEY, String(width))
  }, [width])

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    const startX = event.clientX
    const startWidth = width
    const move = (moveEvent: PointerEvent) => setWidth(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startWidth + moveEvent.clientX - startX)))
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return (
    <>
      <aside className="pane dm-sidebar" style={{ width }}>
        <div className="dm-sidebar-resize" onPointerDown={startResize} />
        <div className="pane-header">
          <span className="pane-title">Direct Messages</span>
          <span className="spacer" />
          <button type="button" className="icon-button" aria-label="New direct message" onClick={() => setCreating(true)}>
            <Edit size={16} />
          </button>
        </div>
        <div className="pane-body dm-list">
          {state.directMessages.map((dm) => {
            const user = state.users.find((candidate) => candidate.id === dm.participantId)
            if (!user) return null
            const last = state.chatMessages.filter((message) => message.channelId === dm.id && !message.threadRootId).at(-1)
            return (
              <button
                key={dm.id}
                type="button"
                className="dm-row"
                data-active={dm.id === activeId || undefined}
                data-unread={dm.unreadCount > 0 || undefined}
                onClick={() => navigate(`/dm/${dm.id}`)}
              >
                <span className="dm-avatar-wrap">
                  <UserAvatar user={user} size={34} />
                  <span className="dm-status-dot" data-online={user.online || undefined} />
                </span>
                <span className="dm-row-copy">
                  <span className="dm-row-head"><strong className="truncate">{user.name}</strong>{last ? <small>{relativeTime(last.createdAt)}</small> : null}</span>
                  <span className="dm-row-preview truncate">{last ? extractPreview(last.content) : `Start a conversation with ${user.name}`}</span>
                </span>
                {dm.unreadCount > 0 ? <span className="count-badge">{dm.unreadCount}</span> : null}
              </button>
            )
          })}
        </div>
      </aside>
      {creating ? (
        <NewDMModal
          state={state}
          onClose={() => setCreating(false)}
          onCreated={(id) => {
            setCreating(false)
            navigate(`/dm/${id}`)
          }}
        />
      ) : null}
    </>
  )
}
