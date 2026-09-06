import { useState } from 'react'
import { useNavigate } from 'react-router'
import { DirectInbox } from 'reicon-react'
import { relativeTime } from '../../lib/format'
import { EmptyState } from '../../components/ui/EmptyState'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useMembers } from '../workspaces/api'
import { notificationCopy, useMarkAllNotificationsRead, useMarkNotificationRead, useNotifications } from './api'
import './inbox.css'

type InboxTab = 'all' | 'unread' | 'mentions'

export function InboxPage() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const [tab, setTab] = useState<InboxTab>('all')
  const allQuery = useNotifications(workspace.id, false)
  const unreadQuery = useNotifications(workspace.id, true)
  const members = useMembers(workspace.id)
  const markRead = useMarkNotificationRead(workspace.id)
  const markAll = useMarkAllNotificationsRead(workspace.id)
  const items = (allQuery.data ?? []).filter((notification) => {
    if (tab === 'unread') return !notification.read_at
    if (tab === 'mentions') return notification.kind === 'comment_mentioned'
    return true
  })
  const unreadCount = unreadQuery.data?.length ?? items.filter((notification) => !notification.read_at).length

  if (allQuery.isPending) {
    return <div className="page"><EmptyState icon={DirectInbox} title="Loading inbox" description="Loading your notifications." /></div>
  }
  if (allQuery.isError) {
    return (
      <div className="page">
        <EmptyState icon={DirectInbox} title="Inbox unavailable" description="Notifications could not be loaded." />
        <button type="button" className="button" onClick={() => void allQuery.refetch()}>Retry</button>
      </div>
    )
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
            disabled={unreadCount === 0 || markAll.isPending}
            onClick={() => markAll.mutate()}
          >
            {markAll.isPending ? 'Marking…' : 'Mark all read'}
          </button>
        </div>
        <div className="pane-toolbar">
          {(['all', 'unread', 'mentions'] as const).map((item) => (
            <button
              key={item}
              type="button"
              className="app-tab"
              data-active={tab === item || undefined}
              onClick={() => setTab(item)}
            >
              {item === 'all' ? 'All' : item === 'unread' ? 'Unread' : 'Mentions'}
            </button>
          ))}
        </div>
        <div className="pane-body inbox-list">
          {items.length === 0 ? (
            <EmptyState icon={DirectInbox} title="You're all caught up" description="Assignments and mentions appear here." />
          ) : (
            items.map((notification) => {
              const actor = members.data?.find((member) => member.id === notification.actor_user_id)
              const copy = notificationCopy(notification)
              return (
                <button
                  key={notification.id}
                  type="button"
                  className="list-row"
                  onClick={() => {
                    if (!notification.read_at) markRead.mutate(notification.id)
                    navigate(`/tasks/${notification.task_id}`)
                  }}
                >
                  <span className="inbox-dot-slot">
                    {!notification.read_at ? <span className="unread-dot" /> : null}
                  </span>
                  <span className="inbox-icon-circle">{(actor?.name ?? '?').charAt(0).toUpperCase()}</span>
                  <span className="inbox-row-main">
                    <span className="inbox-row-title" data-unread={!notification.read_at || undefined}>
                      {copy.title}
                    </span>
                    <span className="inbox-row-body">{actor ? `${actor.name} · ${copy.body}` : copy.body}</span>
                  </span>
                  <span className="text-faint text-xs" style={{ flexShrink: 0 }}>
                    {relativeTime(notification.created_at)}
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
