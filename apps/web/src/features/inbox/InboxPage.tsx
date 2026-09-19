import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Inbox } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { relativeTime } from '../../lib/format'
import { EmptyState } from '../../components/ui/EmptyState'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useMembers } from '../workspaces/api'
import { notificationCopy, useMarkAllNotificationsRead, useMarkNotificationRead, useNotifications } from './api'

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
    return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><EmptyState icon={Inbox} title="Loading inbox" description="Loading your notifications." /></div>
  }
  if (allQuery.isError) {
    return (
      <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
        <EmptyState icon={Inbox} title="Inbox unavailable" description="Notifications could not be loaded." />
        <Button variant="outline" type="button" onClick={() => void allQuery.refetch()}>Retry</Button>
      </div>
    )
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
        <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2 max-[899px]:border-b-0">
          <span className="truncate text-[13px] font-semibold text-foreground">Inbox</span>
          {unreadCount > 0 ? <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold text-primary-foreground">{unreadCount}</span> : null}
          <span className="flex-1" />
          <Button
            variant="ghost"
            type="button"
            disabled={unreadCount === 0 || markAll.isPending}
            onClick={() => markAll.mutate()}
          >
            {markAll.isPending ? 'Marking…' : 'Mark all read'}
          </Button>
        </div>
        <div className="flex min-h-10 shrink-0 items-center gap-1.5 overflow-x-auto border-b border-border px-3 py-1 [scrollbar-width:none]">
          {(['all', 'unread', 'mentions'] as const).map((item) => (
            <button
              key={item}
              type="button"
              className="inline-flex h-7 shrink-0 items-center gap-1 rounded-md px-2.5 text-[13px] font-medium whitespace-nowrap text-muted-foreground transition-colors hover:bg-accent hover:text-foreground data-[active]:bg-primary/10 data-[active]:text-primary data-[active]:ring-1 data-[active]:ring-primary/25 data-[active]:ring-inset"
              data-active={tab === item || undefined}
              onClick={() => setTab(item)}
            >
              {item === 'all' ? 'All' : item === 'unread' ? 'Unread' : 'Mentions'}
            </button>
          ))}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {items.length === 0 ? (
            <EmptyState icon={Inbox} title="You're all caught up" description="Assignments and mentions appear here." />
          ) : (
            items.map((notification) => {
              const actor = members.data?.find((member) => member.id === notification.actor_user_id)
              const copy = notificationCopy(notification)
              return (
                <button
                  key={notification.id}
                  type="button"
                  className="flex min-h-14 w-full min-w-0 cursor-pointer items-center gap-2.5 border-b border-border px-3 py-1.5 text-left transition-colors hover:bg-foreground/[0.02]"
                  onClick={() => {
                    if (!notification.read_at) markRead.mutate(notification.id)
                    navigate(`/tasks/${notification.task_id}`)
                  }}
                >
                  <span className="flex w-2 shrink-0 justify-center">
                    {!notification.read_at ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
                  </span>
                  <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">{(actor?.name ?? '?').charAt(0).toUpperCase()}</span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[13px] text-foreground data-[unread]:font-semibold" data-unread={!notification.read_at || undefined}>
                      {copy.title}
                    </span>
                    <span className="truncate text-xs text-muted-foreground/70">{actor ? `${actor.name} · ${copy.body}` : copy.body}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground/70">
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
