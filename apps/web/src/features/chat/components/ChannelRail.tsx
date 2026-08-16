import { Hashtag } from 'reicon-react'
import { useNavigate } from 'react-router'
import { Avatar } from '../../../components/ui/Avatar'
import { cx } from '../../../lib/cx'
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
      <div className="pane-header">
        <span className="pane-title">Chat</span>
      </div>
      <div className="pane-body chat-rail-body">
        <div className="nav-section">Channels</div>
        {state.channels.map((channel) => (
          <button
            key={channel.id}
            type="button"
            className={cx('menu-item', channel.unreadCount > 0 && 'chat-channel-unread')}
            data-active={channel.id === activeChannelId ? 'true' : undefined}
            onClick={() => {
              markChannelRead(channel.id)
              navigate(`/chat/${channel.id}`)
            }}
          >
            <Hashtag size={16} />
            <span className="menu-item-label">{channel.name}</span>
            {channel.unreadCount > 0 ? (
              <span className="count-badge">{channel.unreadCount}</span>
            ) : null}
          </button>
        ))}
        <div className="nav-section">Members</div>
        {state.users.map((user) => (
          <div key={user.id} className="menu-item">
            <Avatar user={user} size={16} showOnline />
            <span className="menu-item-label">{user.name}</span>
            {user.role === 'Owner' || user.role === 'Admin' ? (
              <span className="chat-member-role">{user.role}</span>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  )
}
