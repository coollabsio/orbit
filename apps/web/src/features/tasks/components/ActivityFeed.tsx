import { Avatar } from '../../../components/ui/Avatar'
import { relativeTime } from '../../../lib/format'
import type { Task, User } from '../../../mock/types'
import { buildFeed } from '../tasksLib'

interface ActivityFeedProps {
  task: Task
  users: User[]
}

export function ActivityFeed({ task, users }: ActivityFeedProps) {
  const feed = buildFeed(task)
  const userById = (id: string) => users.find((u) => u.id === id)

  return (
    <div>
      <h3 className="tasks-activity-heading">Activity</h3>
      <div className="tasks-feed">
        {feed.map((item) => {
          const author = userById(item.authorId)
          if (item.kind === 'activity') {
            return (
              <div key={item.id} className="tasks-feed-activity">
                <Avatar user={author} size={16} />
                <span className="truncate">
                  <span style={{ color: 'var(--text-secondary)', fontWeight: 500 }}>
                    {author?.name ?? 'Someone'}
                  </span>{' '}
                  {item.text}
                </span>
                <span className="text-faint" style={{ flexShrink: 0 }}>
                  {relativeTime(item.createdAt)}
                </span>
              </div>
            )
          }
          return (
            <div key={item.id}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <Avatar user={author} size={20} />
                <span style={{ fontSize: 13, fontWeight: 500 }}>{author?.name ?? 'Someone'}</span>
                <span className="text-faint text-xs">{relativeTime(item.createdAt)}</span>
              </div>
              <div className="tasks-comment-body">{item.text}</div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
