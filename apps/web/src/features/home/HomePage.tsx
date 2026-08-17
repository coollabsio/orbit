import { Link } from 'react-router'
import { Hashtag } from 'reicon-react'
import { useAppState } from '../../mock/store'
import { relativeTime } from '../../lib/format'
import { cx } from '../../lib/cx'
import { TaskStatusIcon } from '../../components/workspace/TaskStatusIcon'
import { PriorityIcon } from '../../components/workspace/PriorityIcon'
import { PRIORITY_ORDER } from '../../components/workspace/taskMeta'
import '../shared/cards.css'
import './home.css'

function greetingFor(hour: number): string {
  if (hour < 12) return 'Good morning'
  if (hour < 18) return 'Good afternoon'
  return 'Good evening'
}

function HomeCard({
  title,
  viewAllTo,
  children,
}: {
  title: string
  viewAllTo: string
  children: React.ReactNode
}) {
  return (
    <section className="layer-card">
      <header className="layer-card-header">
        {title}
        <span className="spacer" />
        <Link to={viewAllTo} className="button button-ghost">
          View all
        </Link>
      </header>
      <div className="layer-card-body flush">{children}</div>
    </section>
  )
}

export function HomePage() {
  const state = useAppState()
  const me = state.users.find((u) => u.id === state.currentUserId)
  const firstName = me?.name.split(' ')[0] ?? 'there'

  const myOpenTasks = state.tasks
    .filter(
      (t) =>
        t.assigneeId === state.currentUserId &&
        (t.status === 'todo' || t.status === 'in_progress'),
    )
    .sort((a, b) => {
      const p = PRIORITY_ORDER.indexOf(a.priority) - PRIORITY_ORDER.indexOf(b.priority)
      if (p !== 0) return p
      return b.updatedAt.localeCompare(a.updatedAt)
    })

  const unreadMail = state.mailThreads.filter((t) => t.unread && t.folderId === 'f_inbox').length
  const unreadChat = state.channels.reduce((sum, c) => sum + c.unreadCount, 0)
  const unreadNotifications = state.notifications.filter((n) => !n.readAt).length

  const sortedNotifications = [...state.notifications].sort((a, b) =>
    b.createdAt.localeCompare(a.createdAt),
  )
  const unreadFirst = sortedNotifications.filter((n) => !n.readAt).slice(0, 5)
  const inboxItems = unreadFirst.length > 0 ? unreadFirst : sortedNotifications.slice(0, 5)

  const recentMail = state.mailThreads
    .filter((t) => t.folderId === 'f_inbox')
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    .slice(0, 4)

  const channels = [...state.channels].sort((a, b) => b.unreadCount - a.unreadCount)

  return (
    <div className="page home-page">
      <div className="pane" style={{ flex: 1 }}>
        <div className="home-scroll">
          <div className="home-body">
            <div className="home-greeting">
              <h1>
                {greetingFor(new Date().getHours())}, {firstName}
              </h1>
              <p className="home-summary">
                <Link to="/tasks">
                  {myOpenTasks.length} open {myOpenTasks.length === 1 ? 'task' : 'tasks'} assigned to you
                </Link>
                {' · '}
                <Link to="/mail">
                  {unreadMail} unread {unreadMail === 1 ? 'email' : 'emails'}
                </Link>
                {' · '}
                <Link to="/chat">
                  {unreadChat} unread {unreadChat === 1 ? 'message' : 'messages'}
                </Link>
                {' · '}
                <Link to="/inbox">
                  {unreadNotifications} {unreadNotifications === 1 ? 'notification' : 'notifications'}
                </Link>
              </p>
            </div>
            <div className="home-grid">
              <HomeCard title="My tasks" viewAllTo="/tasks">
                {myOpenTasks.length === 0 ? (
                  <div className="home-card-empty">No open tasks — enjoy the calm.</div>
                ) : (
                  myOpenTasks.map((task) => (
                    <Link key={task.id} to={`/tasks/${task.id}`} className="list-row">
                      <TaskStatusIcon status={task.status} />
                      <span className="text-faint" style={{ fontSize: 12, flexShrink: 0 }}>
                        {task.identifier}
                      </span>
                      <span className="truncate" style={{ flex: 1, fontSize: 13 }}>
                        {task.title}
                      </span>
                      <PriorityIcon priority={task.priority} />
                      <span className="text-faint text-xs" style={{ flexShrink: 0 }}>
                        {relativeTime(task.updatedAt)}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>

              <HomeCard title="Inbox" viewAllTo="/inbox">
                {inboxItems.length === 0 ? (
                  <div className="home-card-empty">Nothing here yet.</div>
                ) : (
                  inboxItems.map((n) => (
                    <Link key={n.id} to="/inbox" className="list-row">
                      <span
                        className={cx(!n.readAt && 'unread-dot')}
                        style={{ width: 8, height: 8, flexShrink: 0 }}
                      />
                      <span className="truncate" style={{ flex: 1, fontSize: 13 }}>
                        {n.title}
                      </span>
                      <span className="text-faint text-xs" style={{ flexShrink: 0 }}>
                        {relativeTime(n.createdAt)}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>

              <HomeCard title="Recent mail" viewAllTo="/mail">
                {recentMail.length === 0 ? (
                  <div className="home-card-empty">No mail yet.</div>
                ) : (
                  recentMail.map((thread) => (
                    <Link key={thread.id} to={`/mail/${thread.id}`} className="list-row">
                      <span
                        className="truncate"
                        style={{
                          fontSize: 13,
                          fontWeight: thread.unread ? 600 : 400,
                          flexShrink: 0,
                          maxWidth: 140,
                        }}
                      >
                        {thread.messages[0]?.from.name ?? 'Unknown'}
                      </span>
                      <span className="truncate text-muted" style={{ flex: 1, fontSize: 13 }}>
                        {thread.subject}
                      </span>
                      <span className="text-faint text-xs" style={{ flexShrink: 0 }}>
                        {relativeTime(thread.updatedAt)}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>

              <HomeCard title="Active channels" viewAllTo="/chat">
                {channels.length === 0 ? (
                  <div className="home-card-empty">No channels yet.</div>
                ) : (
                  channels.map((channel) => (
                    <Link key={channel.id} to={`/chat/${channel.id}`} className="list-row">
                      <Hashtag size={16} style={{ flexShrink: 0, color: 'var(--text-faint)' }} />
                      <span
                        style={{
                          fontSize: 13,
                          fontWeight: channel.unreadCount > 0 ? 600 : 400,
                          flexShrink: 0,
                        }}
                      >
                        {channel.name}
                      </span>
                      {channel.unreadCount > 0 ? (
                        <span className="count-badge">{channel.unreadCount}</span>
                      ) : null}
                      <span className="truncate text-faint" style={{ flex: 1, fontSize: 12 }}>
                        {channel.description}
                      </span>
                    </Link>
                  ))
                )}
              </HomeCard>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
