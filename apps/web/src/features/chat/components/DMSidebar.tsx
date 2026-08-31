import { useState } from 'react'
import { Edit } from 'reicon-react'
import { useNavigate } from 'react-router'
import { Avatar } from '../../../components/ui/Avatar'
import type { AppState } from '../../../mock/types'
import { relativeTime } from '../../../lib/format'
import { extractPreview } from '../chatLib'
import { NewDMModal } from './NewDMModal'

export function DMSidebar({ state, activeId }: { state: AppState; activeId: string | null }) {
  const navigate = useNavigate()
  const [creating, setCreating] = useState(false)

  return (
    <>
      <aside className="pane dm-sidebar">
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
                <Avatar user={user} size={34} showOnline />
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
