import { Hashtag } from 'reicon-react'
import { useNavigate } from 'react-router'
import { markChannelRead } from '../../../mock/actions'
import type { AppState } from '../../../mock/types'

interface ChannelRailProps {
  state: AppState
  activeChannelId: string | null
}

export function ChannelRail({ state, activeChannelId }: ChannelRailProps) {
  const navigate = useNavigate()
  return (
    <div className="pane chat-rail">
      <div className="pane-body chat-rail-body">
        <div className="nav-section">Channels</div>
        {state.channels.map((channel) => (
          <button
            key={channel.id}
            type="button"
            className="chat-channel-row"
            data-active={channel.id === activeChannelId ? 'true' : undefined}
            data-unread={channel.unreadCount > 0 ? 'true' : undefined}
            onClick={() => {
              markChannelRead(channel.id)
              navigate(`/chat/${channel.id}`)
            }}
          >
            <Hashtag size={16} />
            <span className="chat-channel-name">{channel.name}</span>
            {channel.unreadCount > 0 && channel.id !== activeChannelId ? (
              <span className="chat-unread-badge">{channel.unreadCount}</span>
            ) : null}
          </button>
        ))}
      </div>
    </div>
  )
}
