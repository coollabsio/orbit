import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Code, DirectInbox, InfoCircle } from 'reicon-react'
import { useAppState } from '../../mock/store'
import { markAllNotificationsRead, markNotificationRead } from '../../mock/actions'
import { relativeTime } from '../../lib/format'
import { Avatar } from '../../components/ui/Avatar'
import { EmptyState } from '../../components/ui/EmptyState'
import type { Notification } from '../../mock/types'
import './inbox.css'

type InboxTab = 'all' | 'unread' | 'mentions'

function resourcePath(n: Notification): string | null {
  if (!n.resourceType || !n.resourceId) return null
  switch (n.resourceType) {
    case 'task':
      return `/tasks/${n.resourceId}`
    case 'doc':
      return `/docs/${n.resourceId}`
    case 'channel':
      return `/chat/${n.resourceId}`
    case 'mail':
      return `/mail/${n.resourceId}`
  }
}

export function InboxPage() {
  const state = useAppState()
  const navigate = useNavigate()
  const [tab, setTab] = useState<InboxTab>('all')

  const unreadCount = state.notifications.filter((n) => !n.readAt).length

  const notifications = [...state.notifications]
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    .filter((n) => {
      if (tab === 'unread') return !n.readAt
      if (tab === 'mentions') return n.type === 'mention'
      return true
    })

  const handleClick = (n: Notification) => {
    markNotificationRead(n.id)
    const path = resourcePath(n)
    if (path) navigate(path)
  }

  return (
    <div className="page">
      <div className="pane" style={{ flex: 1 }}>
        <div className="pane-header">
          <span className="pane-title">Inbox</span>
          {unreadCount > 0 ? <span className="count-badge">{unreadCount}</span> : null}
          <span className="spacer" />
          <button
            type="button"
            className="button button-ghost"
            disabled={unreadCount === 0}
            onClick={markAllNotificationsRead}
          >
            Mark all read
          </button>
        </div>
        <div className="pane-toolbar">
          <button
            type="button"
            className="app-tab"
            data-active={tab === 'all' || undefined}
            onClick={() => setTab('all')}
          >
            All
          </button>
          <button
            type="button"
            className="app-tab"
            data-active={tab === 'unread' || undefined}
            onClick={() => setTab('unread')}
          >
            Unread
          </button>
          <button
            type="button"
            className="app-tab"
            data-active={tab === 'mentions' || undefined}
            onClick={() => setTab('mentions')}
          >
            Mentions
          </button>
        </div>
        <div className="pane-body inbox-list">
          {notifications.length === 0 ? (
            <EmptyState icon={DirectInbox} title="You're all caught up" />
          ) : (
            notifications.map((n) => {
              const actor = n.actorId ? state.users.find((u) => u.id === n.actorId) : null
              return (
                <button
                  key={n.id}
                  type="button"
                  className="list-row"
                  onClick={() => handleClick(n)}
                >
                  <span className="inbox-dot-slot">
                    {!n.readAt ? <span className="unread-dot" /> : null}
                  </span>
                  {actor ? (
                    <Avatar user={actor} size={28} />
                  ) : (
                    <span className="inbox-icon-circle">
                      {n.type === 'github' ? <Code size={16} /> : <InfoCircle size={16} />}
                    </span>
                  )}
                  <span className="inbox-row-main">
                    <span className="inbox-row-title" data-unread={!n.readAt || undefined}>
                      {n.title}
                    </span>
                    <span className="inbox-row-body">{n.body}</span>
                  </span>
                  <span className="text-faint text-xs" style={{ flexShrink: 0 }}>
                    {relativeTime(n.createdAt)}
                  </span>
                </button>
              )
            })
          )}
        </div>
      </div>
    </div>
  )
}
